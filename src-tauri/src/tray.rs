//! 系统托盘与全局热键。
//!
//! 全局热键靠 Windows 的 RegisterHotKey 实现，必须有活着的进程去注册。
//! 所以「关闭到托盘」和「全局热键」是配套的：窗口关掉后进程仍在，
//! 热键才继续有效；从托盘菜单真正退出时，两者一起消失。
//!
//! 托盘用的是 Tauri 内置的 `tray-icon` feature，不额外引入 crate。

use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Runtime, WebviewWindow};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::config::Settings;

const MENU_TOGGLE_WINDOW: &str = "hymt2-toggle-window";
const MENU_TOGGLE_MINI: &str = "hymt2-toggle-mini";
const MENU_SELECTION: &str = "hymt2-selection";
const MENU_AUTOSTART: &str = "hymt2-autostart";
const MENU_SETTINGS: &str = "hymt2-settings";
const MENU_GLOSSARY: &str = "hymt2-glossary";
const MENU_QUIT: &str = "hymt2-quit";
/// 引擎与主题是动态的，id 带前缀，后端按前缀回分派。
const MENU_ENGINE_PREFIX: &str = "hymt2-engine:";
const MENU_THEME_PREFIX: &str = "hymt2-theme:";

const TRAY_ID: &str = "hymt2-tray";

/// 小窗是否被用户钉住。钉住后失焦不再自动收起，和划词弹窗同一套交互。
static MINI_PINNED: AtomicBool = AtomicBool::new(false);

pub fn set_mini_pinned(pinned: bool) {
    MINI_PINNED.store(pinned, Ordering::Relaxed);
}

pub fn mini_pinned() -> bool {
    MINI_PINNED.load(Ordering::Relaxed)
}

/// 主题偏好只活在前端的 localStorage 里（三个窗口共享同一份），
/// Rust 这边读不到，托盘菜单却要给对勾——所以由前端主动报过来缓存一份。
/// 冷启动时前端还没挂载，此时按「跟随系统」渲染，用户一改就会重建对齐。
static THEME: Mutex<&'static str> = Mutex::new("auto");

/// 记录主题偏好并重建菜单（对勾位置随之变化）。
pub fn set_theme_cache(app: &AppHandle, mode: &str) {
    let v = match mode {
        "light" => "light",
        "dark" => "dark",
        _ => "auto",
    };
    if let Ok(mut g) = THEME.lock() {
        *g = v;
    }
    rebuild_menu(app);
}

/// 热键种类。设置页有两个热键，注册结果要分开报告，不能混在一起。
pub const HOTKEY_TOGGLE: &str = "toggle";
pub const HOTKEY_MINI: &str = "mini";

/// 显示并把窗口拉到前台。托盘、热键、命令最终都走这里，保证行为一致。
pub fn show_window<R: Runtime>(app: &AppHandle<R>) {
    let Some(win) = app.get_webview_window("main") else {
        return;
    };
    let _ = win.unminimize();
    let _ = win.show();
    let _ = win.set_focus();
}

/// 窗口当前是否「可见着」（含最小化）。
fn is_foreground_visible<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.get_webview_window("main")
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false)
        && !app
            .get_webview_window("main")
            .and_then(|w| w.is_minimized().ok())
            .unwrap_or(false)
}

/// 建托盘菜单。内容会随设置变化（引擎列表、勾选项），所以要能重建。
///
/// 这里刻意用具体的 `AppHandle` 而不是泛型 `R`：菜单事件回调会调
/// `selection::configure` 等一批只收具体 handle 的函数，泛型化就得把
/// selection.rs 整条链一起改，代价远大于收益——运行时 R 恒为 Wry。
fn build_menu(
    app: &AppHandle,
    settings: &Settings,
    visible: bool,
    autostart: bool,
) -> tauri::Result<Menu<tauri::Wry>> {
    use tauri::menu::IsMenuItem;
    let theme = THEME.lock().map(|g| *g).unwrap_or("auto");

    let toggle_window = MenuItem::with_id(
        app,
        MENU_TOGGLE_WINDOW,
        if visible { "隐藏主窗口" } else { "显示主窗口" },
        true,
        None::<&str>,
    )?;
    let toggle_mini = MenuItem::with_id(app, MENU_TOGGLE_MINI, "快速翻译", true, None::<&str>)?;

    // 引擎级联：对勾标当前生效项，和系统里「输出设备/输入法」那类菜单一致
    let mut engines: Vec<CheckMenuItem<tauri::Wry>> = Vec::new();
    for b in &settings.backends {
        engines.push(CheckMenuItem::with_id(
            app,
            format!("{MENU_ENGINE_PREFIX}{}", b.id),
            &b.name,
            true,
            b.id == settings.active_id,
            None::<&str>,
        )?);
    }
    let engine_refs: Vec<&dyn IsMenuItem<tauri::Wry>> =
        engines.iter().map(|e| e as &dyn IsMenuItem<tauri::Wry>).collect();
    // 一个引擎都不剩时给个禁用占位，不然子菜单点开一片空白，
    // 看着像坏了而不是像没配置
    let engine_menu = if engines.is_empty() {
        let hint = MenuItem::with_id(
            app,
            "hymt2-engine-empty",
            "（尚未配置引擎）",
            false,
            None::<&str>,
        )?;
        let refs: Vec<&dyn IsMenuItem<tauri::Wry>> = vec![&hint];
        Submenu::with_items(app, "翻译接口", true, &refs)?
    } else {
        Submenu::with_items(app, "翻译接口", true, &engine_refs)?
    };

    let selection = CheckMenuItem::with_id(
        app,
        MENU_SELECTION,
        "划词翻译",
        true,
        settings.selection_enabled,
        None::<&str>,
    )?;
    let autostart_item = CheckMenuItem::with_id(
        app,
        MENU_AUTOSTART,
        "开机自启",
        true,
        autostart,
        None::<&str>,
    )?;

    let mut theme_items: Vec<CheckMenuItem<tauri::Wry>> = Vec::new();
    for (mode, label) in [("auto", "跟随系统"), ("light", "浅色"), ("dark", "深色")] {
        theme_items.push(CheckMenuItem::with_id(
            app,
            format!("{MENU_THEME_PREFIX}{mode}"),
            label,
            true,
            theme == mode,
            None::<&str>,
        )?);
    }
    let theme_refs: Vec<&dyn IsMenuItem<tauri::Wry>> =
        theme_items.iter().map(|e| e as &dyn IsMenuItem<tauri::Wry>).collect();
    let theme_menu = Submenu::with_items(app, "主题", true, &theme_refs)?;

    let settings_item = MenuItem::with_id(app, MENU_SETTINGS, "打开设置", true, None::<&str>)?;
    let glossary_item = MenuItem::with_id(app, MENU_GLOSSARY, "打开术语库", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, MENU_QUIT, "退出", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let sep3 = PredefinedMenuItem::separator(app)?;
    let sep4 = PredefinedMenuItem::separator(app)?;

    Menu::with_items(
        app,
        &[
            &toggle_window,
            &sep1,
            &toggle_mini,
            &engine_menu,
            &sep2,
            &selection,
            &autostart_item,
            &theme_menu,
            &sep3,
            &settings_item,
            &glossary_item,
            &sep4,
            &quit,
        ],
    )
}

/// 用当前设置重建托盘菜单。
///
/// 菜单内容全是从配置现算的（引擎列表、四个开关的对勾），所以任何改动
/// 设置的动作结束时都得重建一次，否则菜单会一直停在上一次的状态。
pub fn rebuild_menu(app: &AppHandle) {
    let tray = match app.tray_by_id(TRAY_ID) {
        Some(t) => t,
        None => return,
    };
    let st = match app.try_state::<crate::AppState>() {
        Some(st) => st,
        None => return,
    };
    let settings = { st.settings.lock().unwrap().clone() };
    let visible = is_foreground_visible(app);
    let autostart = {
        use tauri_plugin_autostart::ManagerExt;
        app.autolaunch().is_enabled().unwrap_or(false)
    };
    if let Ok(menu) = build_menu(app, &settings, visible, autostart) {
        let _ = tray.set_menu(Some(menu));
    }
}

/// 处理菜单点击。就地改设置，然后重建菜单让对勾跟上。
fn on_menu(app: &AppHandle, id: &str) {
    if let Some(bid) = id.strip_prefix(MENU_ENGINE_PREFIX) {
        set_active_backend(app, bid);
    } else if let Some(mode) = id.strip_prefix(MENU_THEME_PREFIX) {
        set_theme_cache(app, mode);
        // 三个窗口都靠这一条事件同步；前端收到后再把权威值回报一遍
        let _ = app.emit("hymt2:theme", serde_json::json!({ "mode": mode }));
    } else {
        match id {
            MENU_TOGGLE_WINDOW => toggle_window(app),
            MENU_TOGGLE_MINI => toggle_mini(app),
            MENU_SELECTION => toggle_selection(app),
            MENU_AUTOSTART => toggle_autostart(app),
            MENU_SETTINGS => open_tab(app, "settings"),
            MENU_GLOSSARY => open_tab(app, "glossary"),
            MENU_QUIT => {
                // 进程都要退了，别再去重建那个即将销毁的托盘
                app.exit(0);
                return;
            }
            _ => {}
        }
    }
    rebuild_menu(app);
}

/// 显示/隐藏主窗口。托盘左键只做「显示」，这里是完整的切换。
fn toggle_window(app: &AppHandle) {
    if is_foreground_visible(app) {
        if let Some(win) = app.get_webview_window("main") {
            let _ = win.hide();
        }
    } else {
        show_window(app);
    }
}

/// 切到指定页签并把窗口亮出来。
fn open_tab(app: &AppHandle, tab: &str) {
    show_window(app);
    let _ = app.emit_to("main", "hymt2:navigate", serde_json::json!({ "tab": tab }));
}

/// 切换生效引擎：落盘 + 通知前端刷新状态栏上的引擎胶囊。
fn set_active_backend(app: &AppHandle, id: &str) {
    let Some(st) = app.try_state::<crate::AppState>() else {
        return;
    };
    {
        let mut guard = st.settings.lock().unwrap();
        // 点到已被删掉的引擎上：忽略，别把 active_id 打成空
        if !guard.backends.iter().any(|b| b.id == id) {
            return;
        }
        guard.active_id = id.to_string();
        let _ = guard.save();
    }
    let _ = app.emit("hymt2:settings-changed", serde_json::json!({}));
}

fn toggle_selection(app: &AppHandle) {
    let Some(st) = app.try_state::<crate::AppState>() else {
        return;
    };
    let (enabled, modifier) = {
        let mut guard = st.settings.lock().unwrap();
        guard.selection_enabled = !guard.selection_enabled;
        let _ = guard.save();
        (guard.selection_enabled, guard.selection_modifier.clone())
    };
    crate::selection::configure(app, enabled, &modifier);
    let _ = app.emit("hymt2:settings-changed", serde_json::json!({}));
}

fn toggle_autostart(app: &AppHandle) {
    use tauri_plugin_autostart::ManagerExt;
    let m = app.autolaunch();
    let now = m.is_enabled().unwrap_or(false);
    // 结果回写配置里的开关位，设置页才能显示正确状态
    let r = if now { m.disable() } else { m.enable() };
    if r.is_err() {
        return;
    }
    let _ = app.emit("hymt2:settings-changed", serde_json::json!({}));
}

/// 建托盘：左键单击直接唤出，右键菜单给出各项快捷操作。
pub fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let settings = { app.state::<crate::AppState>().settings.lock().unwrap().clone() };
    let autostart = {
        use tauri_plugin_autostart::ManagerExt;
        app.autolaunch().is_enabled().unwrap_or(false)
    };
    let menu = build_menu(app, &settings, false, autostart)?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip("Zephyr Translator")
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| on_menu(app, event.id().as_ref()))
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_window(tray.app_handle());
            }
        });

    // 复用应用图标，避免托盘位置显示空白
    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }
    builder.build(app)?;
    Ok(())
}

/// 注册全局热键。
///
/// Windows 的 RegisterHotKey 在组合键被别的软件占用时是**静默失败**的，
/// 所以必须把结果返回给界面提示，不能默默吞掉——否则用户会以为功能
/// 没生效，一直找不到原因。
pub fn register_toggle_hotkey<R: Runtime>(app: &AppHandle<R>, accel: &str) -> Result<(), String> {
    let shortcut = Shortcut::from_str(accel).map_err(|e| format!("快捷键格式无法解析：{e}"))?;
    app.global_shortcut()
        .on_shortcut(shortcut, move |app, _sc, event| {
            if event.state() != ShortcutState::Pressed {
                return;
            }
            // 已显示则隐藏，实现「一键切换」
            if is_foreground_visible(app) {
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.hide();
                }
            } else {
                show_window(app);
            }
        })
        .map_err(|e| format!("注册失败（可能被其他软件占用）：{e}"))?;
    Ok(())
}

/// 切换小窗模式。
///
/// 小窗是「输入 + 译文」的轻量窗口，唤出它**不碰主窗口**——用户按这个热键
/// 就是想快速译一句，再弹一个 960×640 的主窗口反而碍事。想进主窗口，用小窗
/// 上的按钮。
pub fn toggle_mini<R: Runtime>(app: &AppHandle<R>) {
    let Some(win) = app.get_webview_window("mini") else {
        return;
    };
    if win.is_visible().unwrap_or(false) {
        let _ = win.hide();
        return;
    }
    let _ = win.show();
    let _ = win.set_focus();
    // 让前端复位输入框并聚焦（窗口是常驻的，不重启就收不到 onMounted）
    let _ = app.emit_to("mini", "hymt2:mini-shown", serde_json::json!({}));
}

/// 注册小窗模式的全局热键。
pub fn register_mini_hotkey<R: Runtime>(app: &AppHandle<R>, accel: &str) -> Result<(), String> {
    let shortcut = Shortcut::from_str(accel).map_err(|e| format!("快捷键格式无法解析：{e}"))?;
    app.global_shortcut()
        .on_shortcut(shortcut, move |app, _sc, event| {
            if event.state() != ShortcutState::Pressed {
                return;
            }
            toggle_mini(app);
        })
        .map_err(|e| format!("注册失败（可能被其他软件占用）：{e}"))?;
    Ok(())
}

/// 窗口关闭时按设置决定隐藏到托盘还是真退出。
pub fn on_close_requested<R: Runtime>(win: &WebviewWindow<R>, close_to_tray: bool) {
    if close_to_tray {
        let _ = win.hide();
    }
}

/// 把热键注册结果推给前端，界面据此提示用户。
///
/// `which` 区分是哪一个热键（`HOTKEY_TOGGLE` / `HOTKEY_MINI`），设置页
/// 有两个热键，结果不能混着报。
pub fn notify_hotkey<R: Runtime>(app: &AppHandle<R>, which: &str, ok: bool, message: String) {
    let _ = app.emit(
        "hymt2:hotkey",
        serde_json::json!({ "which": which, "ok": ok, "message": message }),
    );
}

/// 按配置重新注册**全部**热键，并把结果逐个通知前端。
///
/// 两个热键共用一次 `unregister_all`，所以必须一起重注册——只改其中一个
/// 也要把另一个带上，否则改一次就少一个。
pub fn apply_hotkeys<R: Runtime>(app: &AppHandle<R>, s: &Settings) {
    let _ = app.global_shortcut().unregister_all();

    let toggle = s.hotkey_toggle.trim();
    if toggle.is_empty() {
        notify_hotkey(app, HOTKEY_TOGGLE, true, "全局热键未启用".into());
    } else {
        match register_toggle_hotkey(app, toggle) {
            Ok(()) => notify_hotkey(app, HOTKEY_TOGGLE, true, format!("全局热键已注册：{toggle}")),
            Err(e) => notify_hotkey(app, HOTKEY_TOGGLE, false, e),
        }
    }

    let mini = s.hotkey_mini.trim();
    if mini.is_empty() {
        notify_hotkey(app, HOTKEY_MINI, true, "小窗热键未启用".into());
    } else {
        match register_mini_hotkey(app, mini) {
            Ok(()) => notify_hotkey(app, HOTKEY_MINI, true, format!("小窗热键已注册：{mini}")),
            Err(e) => notify_hotkey(app, HOTKEY_MINI, false, e),
        }
    }
}