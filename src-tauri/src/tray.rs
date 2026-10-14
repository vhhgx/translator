//! 系统托盘与全局热键。
//!
//! 全局热键靠 Windows 的 RegisterHotKey 实现，必须有活着的进程去注册。
//! 所以「关闭到托盘」和「全局热键」是配套的：窗口关掉后进程仍在，
//! 热键才继续有效；从托盘菜单真正退出时，两者一起消失。
//!
//! 托盘用的是 Tauri 内置的 `tray-icon` feature，不额外引入 crate。

use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Runtime, WebviewWindow};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::config::Settings;

const MENU_SHOW: &str = "hymt2-show";
const MENU_QUIT: &str = "hymt2-quit";

/// 小窗是否被用户钉住。钉住后失焦不再自动收起，和划词弹窗同一套交互。
static MINI_PINNED: AtomicBool = AtomicBool::new(false);

pub fn set_mini_pinned(pinned: bool) {
    MINI_PINNED.store(pinned, Ordering::Relaxed);
}

pub fn mini_pinned() -> bool {
    MINI_PINNED.load(Ordering::Relaxed)
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

/// 建托盘：左键单击直接唤出，右键菜单给出「显示窗口 / 退出」。
pub fn build_tray<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, MENU_SHOW, "显示窗口", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, MENU_QUIT, "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    let mut builder = TrayIconBuilder::with_id("hymt2-tray")
        .menu(&menu)
        .tooltip("Zephyr Translator")
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            MENU_SHOW => show_window(app),
            MENU_QUIT => app.exit(0),
            _ => {}
        })
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