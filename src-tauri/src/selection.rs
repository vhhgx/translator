//! 划词翻译：监听全局鼠标，选中文本后自动在光标处弹窗。
//!
//! 抓取选中文本用的是划词类工具的通用手法：先清空剪贴板当哨兵 →
//! 模拟 Ctrl+C → 读回剪贴板 → 还原原剪贴板；读回来还是空的，说明这次
//! 点击压根没选中东西。
//!
//! 直接用 `rdev::simulate` 发按键而不是引 enigo，是因为 enigo 在 Linux 上
//! 会硬链 `libxdo`，多一个系统依赖；rdev 走 XTest，够用且已经在了。
//!
//! 平台限制：整条链路依赖「模拟按键」，所以
//!   - **Wayland 下不可用**：普通客户端既不能模拟全局按键，也读不到别的应用的选区。
//!   - macOS 需要先授予「辅助功能」权限，本期未做引导。
//! 起不来时不静默失败，错误会通过 `selection_status` 暴露给设置页。

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use rdev::{Button, EventType, Key};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition};

const POPUP_LABEL: &str = "popup";
/// 两次触发的最小间隔。一次拖选可能抖出多个事件，去抖避免重复弹窗。
const DEBOUNCE: Duration = Duration::from_millis(400);
/// 弹窗相对光标的偏移。
const OFFSET: (i32, i32) = (14, 20);
/// 贴屏幕边缘时保留的空隙。
const MARGIN: i32 = 8;

/// 关闭时只是不再抓文本；监听线程一旦起来就不再卸载（rdev 没有 unlisten）。
static ENABLED: AtomicBool = AtomicBool::new(false);
static STARTED: AtomicBool = AtomicBool::new(false);
/// 0 = 不要求修饰键，1 = Ctrl，2 = Alt，3 = Shift
static NEED_MOD: AtomicU8 = AtomicU8::new(0);
static HELD_CTRL: AtomicBool = AtomicBool::new(false);
static HELD_ALT: AtomicBool = AtomicBool::new(false);
static HELD_SHIFT: AtomicBool = AtomicBool::new(false);
/// 光标位置由 MouseMove 事件持续更新——比事后查询更省一个依赖。
static CURSOR_X: AtomicI32 = AtomicI32::new(0);
static CURSOR_Y: AtomicI32 = AtomicI32::new(0);

static LAST_FIRE: Mutex<Option<Instant>> = Mutex::new(None);
/// `rdev::listen` 起不来时的原因（Wayland、无 X11 等），留给设置页展示。
static START_ERROR: Mutex<Option<String>> = Mutex::new(None);
/// 待弹窗处理的文本。
///
/// 弹窗窗口在 config 里是 `visible: false`，GTK 不会为「从未显示过」的窗口
/// 创建 X 窗口，里面的 webview 也就不会加载——所以第一次弹窗时，它还没来得
/// 及注册事件监听，我们发出去的事件它收不到。这里存一份，让它挂载后主动来取。
static PENDING: Mutex<Option<String>> = Mutex::new(None);

/// 由弹窗前端在挂载后调用一次。
pub fn take_pending() -> Option<String> {
    PENDING.lock().unwrap().take()
}

#[derive(Serialize)]
pub struct SelectionStatus {
    pub enabled: bool,
    /// 监听线程是否已启动（启动过就不再看开关）。
    pub started: bool,
    /// "" | "ctrl" | "alt" | "shift"
    pub modifier: String,
    /// 启动失败原因；None 表示正常。
    pub error: Option<String>,
}

fn modifier_key(n: u8) -> &'static str {
    match n {
        1 => "ctrl",
        2 => "alt",
        3 => "shift",
        _ => "",
    }
}

fn parse_modifier(s: &str) -> u8 {
    match s.trim().to_ascii_lowercase().as_str() {
        "ctrl" | "control" => 1,
        "alt" => 2,
        "shift" => 3,
        _ => 0,
    }
}

/// 按设置更新开关与修饰键要求。打开开关时才真正拉起监听线程。
pub fn configure(app: &AppHandle, enabled: bool, modifier: &str) {
    NEED_MOD.store(parse_modifier(modifier), Ordering::Relaxed);
    ENABLED.store(enabled, Ordering::Relaxed);
    if enabled {
        ensure_started(app);
    }
}

/// 幂等地启动全局监听线程。
pub fn ensure_started(app: &AppHandle) {
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        if let Err(e) = rdev::listen(move |ev| on_event(&app, ev.event_type)) {
            let msg = format!("{e:?}");
            eprintln!("[selection] 全局输入监听启动失败: {msg}");
            *START_ERROR.lock().unwrap() = Some(msg);
        }
    });
}

pub fn status() -> SelectionStatus {
    SelectionStatus {
        enabled: ENABLED.load(Ordering::Relaxed),
        started: STARTED.load(Ordering::Relaxed),
        modifier: modifier_key(NEED_MOD.load(Ordering::Relaxed)).to_string(),
        error: START_ERROR.lock().unwrap().clone(),
    }
}

pub fn hide(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(POPUP_LABEL) {
        if w.is_visible().unwrap_or(false) {
            let _ = w.hide();
        }
    }
}

fn set_mod(key: Key, down: bool) {
    match key {
        Key::ControlLeft | Key::ControlRight => HELD_CTRL.store(down, Ordering::Relaxed),
        Key::Alt | Key::AltGr => HELD_ALT.store(down, Ordering::Relaxed),
        Key::ShiftLeft | Key::ShiftRight => HELD_SHIFT.store(down, Ordering::Relaxed),
        _ => {}
    }
}

fn modifier_ok() -> bool {
    match NEED_MOD.load(Ordering::Relaxed) {
        1 => HELD_CTRL.load(Ordering::Relaxed),
        2 => HELD_ALT.load(Ordering::Relaxed),
        3 => HELD_SHIFT.load(Ordering::Relaxed),
        _ => true,
    }
}

fn debounce_ok() -> bool {
    let mut guard = LAST_FIRE.lock().unwrap();
    let now = Instant::now();
    if let Some(prev) = *guard {
        if now.duration_since(prev) < DEBOUNCE {
            return false;
        }
    }
    *guard = Some(now);
    true
}

fn cursor() -> (i32, i32) {
    (
        CURSOR_X.load(Ordering::Relaxed),
        CURSOR_Y.load(Ordering::Relaxed),
    )
}

/// 光标是否落在弹窗里。落在里面就不隐藏——否则点弹窗上的按钮时，
/// mousedown 会先把弹窗藏掉，click 落到别的窗口上了。
fn cursor_in_popup(app: &AppHandle) -> bool {
    let Some(win) = app.get_webview_window(POPUP_LABEL) else {
        return false;
    };
    if !win.is_visible().unwrap_or(false) {
        return false;
    }
    let (Ok(pos), Ok(size)) = (win.outer_position(), win.outer_size()) else {
        return false;
    };
    let (cx, cy) = cursor();
    cx >= pos.x && cx < pos.x + size.width as i32 && cy >= pos.y && cy < pos.y + size.height as i32
}

fn on_event(app: &AppHandle, event: EventType) {
    match event {
        EventType::MouseMove { x, y } => {
            CURSOR_X.store(x as i32, Ordering::Relaxed);
            CURSOR_Y.store(y as i32, Ordering::Relaxed);
        }
        EventType::KeyPress(k) => set_mod(k, true),
        EventType::KeyRelease(k) => set_mod(k, false),
        EventType::ButtonPress(Button::Left) => {
            if !cursor_in_popup(app) {
                hide(app);
            }
        }
        EventType::ButtonRelease(Button::Left) => {
            if !ENABLED.load(Ordering::Relaxed) || !modifier_ok() || !debounce_ok() {
                return;
            }
            // 抓取要模拟按键 + sleep，约 150ms，不能占着监听线程。
            let app = app.clone();
            std::thread::spawn(move || capture_and_show(&app));
        }
        _ => {}
    }
}

fn capture_and_show(app: &AppHandle) {
    let Some(text) = capture_selected_text() else {
        return;
    };
    let text = text.trim();
    // 空 = 这次点击没选中任何东西。
    if text.is_empty() {
        return;
    }
    show_popup(app, text);
}

fn clipboard_text() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok()
}

/// 抓取当前选中的文本；没选中或剪贴板不是文本时返回 None。
///
/// 剪贴板里放的不是文本（图片等）时**直接放弃**：那种内容没法安全地存下来
/// 再还原，宁可这次不划词，也不要把用户的东西弄丢。
fn capture_selected_text() -> Option<String> {
    let previous = clipboard_text()?;

    {
        let mut cb = arboard::Clipboard::new().ok()?;
        // 哨兵：清空。若这次 Ctrl+C 什么都没选中，读回来就还是空的。
        cb.set_text(String::new()).ok()?;
    }
    std::thread::sleep(Duration::from_millis(40));

    // 先把可能还按着的修饰键放掉，否则会拼成 Ctrl+Shift+C 之类，目标应用不认。
    for k in [
        Key::ControlLeft,
        Key::ControlRight,
        Key::ShiftLeft,
        Key::ShiftRight,
        Key::Alt,
        Key::MetaLeft,
    ] {
        let _ = rdev::simulate(&EventType::KeyRelease(k));
    }
    let _ = rdev::simulate(&EventType::KeyPress(Key::ControlLeft));
    let _ = rdev::simulate(&EventType::KeyPress(Key::KeyC));
    let _ = rdev::simulate(&EventType::KeyRelease(Key::KeyC));
    let _ = rdev::simulate(&EventType::KeyRelease(Key::ControlLeft));
    std::thread::sleep(Duration::from_millis(120));

    let captured = clipboard_text();

    // 把用户原来的剪贴板放回去
    if let Ok(mut cb) = arboard::Clipboard::new() {
        let _ = cb.set_text(previous);
    }

    let text = captured?;
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

fn show_popup(app: &AppHandle, text: &str) {
    let Some(win) = app.get_webview_window(POPUP_LABEL) else {
        return;
    };

    // 先存一份：首次弹出时弹窗的 webview 才刚开始加载，收不到下面这条事件
    *PENDING.lock().unwrap() = Some(text.to_string());

    if let Ok(size) = win.outer_size() {
        let (w, h) = (size.width as i32, size.height as i32);
        let (cx, cy) = cursor();
        let (mut x, mut y) = (cx + OFFSET.0, cy + OFFSET.1);

        // 夹到光标所在显示器的可用区域内
        if let Ok(Some(mon)) = app.monitor_from_point(cx as f64, cy as f64) {
            let pos = mon.position();
            let sz = mon.size();
            let (left, top) = (pos.x, pos.y);
            let (right, bottom) = (pos.x + sz.width as i32, pos.y + sz.height as i32);
            // 右边放不下就左移；下边放不下就翻到光标上方
            if x + w + MARGIN > right {
                x = right - w - MARGIN;
            }
            if y + h + MARGIN > bottom {
                y = cy - h - OFFSET.1;
            }
            x = x.max(left + MARGIN);
            y = y.max(top + MARGIN);
        }

        let _ = win.set_position(PhysicalPosition::new(x.max(0), y.max(0)));
    }

    // 先把文本推过去再显示，避免弹出一个空窗再闪内容
    let _ = app.emit_to(POPUP_LABEL, "hymt2:selection", serde_json::json!({ "text": text }));
    let _ = win.show();
}
