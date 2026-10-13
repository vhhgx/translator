//! 系统托盘与全局热键。
//!
//! 全局热键靠 Windows 的 RegisterHotKey 实现，必须有活着的进程去注册。
//! 所以「关闭到托盘」和「全局热键」是配套的：窗口关掉后进程仍在，
//! 热键才继续有效；从托盘菜单真正退出时，两者一起消失。
//!
//! 托盘用的是 Tauri 内置的 `tray-icon` feature，不额外引入 crate。

use std::str::FromStr;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Runtime, WebviewWindow};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::config::Settings;

const MENU_SHOW: &str = "hymt2-show";
const MENU_QUIT: &str = "hymt2-quit";

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
        .tooltip("Hy-MT2 翻译工具")
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

/// 重新绑定：先全部注销，再按新配置注册。
pub fn rebind_hotkey<R: Runtime>(app: &AppHandle<R>, accel: &str) -> Result<(), String> {
    let _ = app.global_shortcut().unregister_all();
    register_toggle_hotkey(app, accel)
}

/// 窗口关闭时按设置决定隐藏到托盘还是真退出。
pub fn on_close_requested<R: Runtime>(win: &WebviewWindow<R>, close_to_tray: bool) {
    if close_to_tray {
        let _ = win.hide();
    }
}

/// 把热键注册结果推给前端，界面据此提示用户。
pub fn notify_hotkey<R: Runtime>(app: &AppHandle<R>, ok: bool, message: String) {
    let _ = app.emit(
        "hymt2:hotkey",
        serde_json::json!({ "ok": ok, "message": message }),
    );
}

/// 按配置注册热键并把结果通知前端。
pub fn apply_hotkey_from_settings<R: Runtime>(app: &AppHandle<R>, s: &Settings) {
    if s.hotkey_toggle.trim().is_empty() {
        let _ = app.global_shortcut().unregister_all();
        notify_hotkey(app, true, "已关闭全局热键".to_string());
        return;
    }
    match rebind_hotkey(app, &s.hotkey_toggle) {
        Ok(()) => notify_hotkey(
            app,
            true,
            format!("全局热键已注册：{}", s.hotkey_toggle),
        ),
        Err(e) => notify_hotkey(app, false, e),
    }
}