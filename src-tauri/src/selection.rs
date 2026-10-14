//! 划词翻译：监听全局鼠标，选中文本后自动在光标处弹窗。
//!
//! 「怎么把选中的文字取出来」按平台分两套实现：
//!
//! - **Windows 走 UI Automation**（见 `selection_win`）。直接问目标应用
//!   「你当前选中了什么」，**完全不碰剪贴板**。
//! - **Linux / macOS 目前仍走剪贴板中转**（`capture_selected_text_clip`）：
//!   写哨兵 → 模拟 Ctrl+C → 读回 → 有条件地还原。这套做法会短暂占用用户
//!   剪贴板，早先在 Windows 上出过「复制什么都不进去了」的事故，所以
//!   **没有默认退回**，宁可取不到就不弹窗。后续给 macOS 接上
//!   Accessibility API（`AXUIElement`）后，这条路径就该退休了。
//!
//! 监听本身用 `rdev` 而不是 enigo，是因为 enigo 在 Linux 上会硬链
//! `libxdo`，多一个系统依赖；rdev 走 XTest，够用且已经在了。
//!
//! 平台限制：
//!   - **Wayland 下划词整体不可用**：普通客户端既不能模拟全局按键，也读不到别的应用的选区。
//!   - macOS 需要「辅助功能」权限，本期未做引导。
//!   - Windows 上未实现 UIA 的控件取不到选区，此时不弹窗、不碰剪贴板。
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

/// 取词实现：Windows 用 UIA，其它平台暂借剪贴板中转。
#[cfg(windows)]
fn capture_selected_text() -> Option<String> {
    crate::selection_win::selected_text()
}

#[cfg(not(windows))]
fn capture_selected_text() -> Option<String> {
    capture_selected_text_clip()
}

/// 清掉上一次残留的哨兵。
///
/// 只对走剪贴板的平台有意义——Windows 已经不写哨兵了。哨兵的生命周期只有
/// 捕获那 ~250ms，可进程要是正好死在窗口里（热重载、任务管理器强杀、崩溃），
/// 这次哨兵就没人还原了。更麻烦的是它会自我繁殖：下一次捕获读到 `old` 是
/// 哨兵，还原时又写回一个哨兵，剪贴板就被永久毒化。
#[cfg(not(windows))]
pub fn repair_clipboard() {
    if let Ok(mut cb) = arboard::Clipboard::new() {
        if let Ok(text) = cb.get_text() {
            if text.starts_with(SENTINEL_PREFIX) {
                let _ = cb.set_text(String::new());
            }
        }
    }
}

/// Windows 上没有哨兵，留一个空实现，免得调用点还要写条件编译。
#[cfg(windows)]
pub fn repair_clipboard() {}

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
/// 上一次推给弹窗的原文。用来识别「选区还在、又点了一下」的重复触发。
static LAST_SHOWN: Mutex<Option<String>> = Mutex::new(None);
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
            // 弹窗没了，下次同样的文字就是「重新选的」，该弹还得弹。
            LAST_SHOWN.lock().unwrap().take();
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

/// 本应用自己的窗口。鼠标落在这些窗口里时不能当成划词——
/// 抓取要往当前焦点窗口打一串模拟按键，打在自己界面上就是劫持自己的输入
/// （比如录制热键时，那串合成的 Ctrl+C 会被录制器当成用户按下的组合键收走）。
const OWN_LABELS: [&str; 3] = ["main", "mini", POPUP_LABEL];

/// 光标是否落在某个可见窗口的矩形内。
fn cursor_in_window(app: &AppHandle, label: &str) -> bool {
    let Some(win) = app.get_webview_window(label) else {
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

/// 光标是否落在弹窗里。落在里面就不隐藏——否则点弹窗上的按钮时，
/// mousedown 会先把弹窗藏掉，click 落到别的窗口上了。
fn cursor_in_popup(app: &AppHandle) -> bool {
    cursor_in_window(app, POPUP_LABEL)
}

/// 光标是否在本应用自己的某个窗口里。
fn cursor_in_own_window(app: &AppHandle) -> bool {
    OWN_LABELS.iter().any(|l| cursor_in_window(app, l))
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
            // 在本应用自己的窗口里点击是一次界面交互，不是划词，必须拦在前面。
            // 录制热键时设置页正收着 keydown，早先剪贴板路径往这里打进过
            // 合成按键，把热键收走并注册成全局热键，主窗口直接被 hide。
            //
            // 放在 debounce_ok() 之前判断：界面内的点击不应该消耗掉去抖窗口，
            // 否则紧接着的正常划词会被误判成频率过高而丢掉。
            if cursor_in_own_window(app) {
                return;
            }
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

    // 弹窗已经开着、内容又和上一次一模一样 —— 这不是用户重新选了一次，
    // 而是**旧选区还在**。选完文字后，点滚动条、点工具栏、点窗口里的非文本
    // 区域都不会清掉选区，UIA 焦点也仍在那个文本控件上，于是每次这样的点击
    // 都会把同一段文字再取一遍。再弹一次只会闪给用户看。
    if popup_is_showing(app) && last_shown() == text {
        return;
    }

    *LAST_SHOWN.lock().unwrap() = Some(text.to_string());
    show_popup(app, text);
}

fn last_shown() -> String {
    LAST_SHOWN.lock().unwrap().clone().unwrap_or_default()
}

fn popup_is_showing(app: &AppHandle) -> bool {
    app.get_webview_window(POPUP_LABEL)
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false)
}

/// 等模拟 Ctrl+C 之后剪贴板被改写的上限。超时就当这次没复制到东西。
#[cfg(not(windows))]
const CAPTURE_TIMEOUT: Duration = Duration::from_millis(200);
/// 哨兵前缀。既是哨兵的格式，也是启动自愈时识别残留的依据。
#[cfg(not(windows))]
const SENTINEL_PREFIX: &str = "hymt2-sentinel-";

#[cfg(not(windows))]
fn clipboard_text() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok()
}

/// 判断剪贴板里现在这份内容是不是我们写的，从而决定要不要还原。
///
/// 抽成纯函数是为了能单独测——这段要是判错，表现就是「用户刚复制的内容
/// 凭空消失」，而在真机上很难复现出来看。
#[cfg(not(windows))]
fn should_restore(current: Option<&str>, captured: Option<&str>, sentinel: &str) -> bool {
    match (current, captured) {
        // 模拟的 Ctrl+C 没复制到东西，剪贴板还是哨兵 → 我们弄的
        (Some(cur), _) if cur == sentinel => true,
        // 剪贴板内容等于我们模拟复制到的结果 → 也是我们弄的
        (Some(cur), Some(cap)) if cur == cap => true,
        // 读回失败、而剪贴板此时也读不出来：内容多半就是我们清掉后没人写
        (None, None) => true,
        _ => false,
    }
}

/// 抓取当前选中的文本；没选中或剪贴板不是文本时返回 None。
///
/// **仅限 Windows 以外的平台。** Windows 走 UIA（`selection_win`），
/// 这里根本不参与编译——那边一次剪贴板都不碰。
///
/// 剪贴板里放的不是文本（图片等）时**直接放弃**：那种内容没法安全地存下来
/// 再还原，宁可这次不划词，也不要把用户的东西弄丢。
///
/// 这里有两件事必须做对，否则用户会觉得「复制坏了」：
///
/// 1. **哨兵不能用空串。** 以前放的是空串，于是捕获那 ~160ms 里剪贴板
///    是真空的，用户这段时间复制的东西看起来就没进去，粘贴也是空的。
///    换成一段唯一文本，这段时间剪贴板始终有内容。
/// 2. **还原必须有条件。** 以前是无条件写回 old，于是「划词刚结束、用户
///    紧接着按的 Ctrl+C」会被 old 覆盖掉——用户刚复制的内容凭空消失。
///    现在交给 should_restore 判断：还是哨兵、或等于我们自己复制到的，
///    才认为是我们弄的、可以还原；期间用户自己复制过的，一律不碰。
#[cfg(not(windows))]
fn capture_selected_text_clip() -> Option<String> {
    let old = clipboard_text()?;

    // 唯一串：同一个进程连续触发也不会撞。
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let sentinel = format!("{SENTINEL_PREFIX}{}-{}", std::process::id(), stamp);

    {
        let mut cb = arboard::Clipboard::new().ok()?;
        // 哨兵。若这次 Ctrl+C 什么都没选中，读回来就还是它。
        cb.set_text(sentinel.clone()).ok()?;
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

    // 轮询等剪贴板被改写，而不是固定睡 120ms。
    //
    // 这里有两个互相拉扯的约束：
    //  - 轮询要**轻**。我们在抢剪贴板，4ms 一次等于和目标应用抢 OpenClipboard，
    //    抢输了它就复制不到东西，弹窗根本不弹——实测踩过这个坑。
    //  - 窗口要**短**。拖得越久，越容易把用户自己按的 Ctrl+C 混进来。
    // 所以先给目标应用 30ms 独自落盘，再以 12ms 的间隔轮询。
    std::thread::sleep(Duration::from_millis(30));
    let deadline = Instant::now() + CAPTURE_TIMEOUT;
    let mut captured: Option<String> = None;
    loop {
        match clipboard_text() {
            // 内容变了 → 这次 Ctrl+C 复制到东西了
            Some(t) if t != sentinel => {
                captured = Some(t);
                break;
            }
            // 还是哨兵 → 目标应用还没写进去，继续等
            Some(_) => {}
            // 读不到 = 剪贴板这会儿被别人占着。必须重试，绝不能当成
            // 「什么都没复制到」提前收工——之前 early-return 导致划词静默失效。
            None => {}
        }
        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(12));
    }

    // 有条件地还原：只回滚我们自己造成的改动。
    if let Ok(mut cb) = arboard::Clipboard::new() {
        let now = cb.get_text().ok();
        if should_restore(now.as_deref(), captured.as_deref(), &sentinel) {
            let _ = cb.set_text(old);
        }
    }

    let text = captured?;
    // 读回哨兵 = 这次点击压根没选中东西。
    if text == sentinel || text.trim().is_empty() {
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

// 这些用例测的是剪贴板路径的还原判定，Windows 不编译那条路径。
#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;

    const S: &str = "hymt2-sentinel-1-2";

    #[test]
    fn 没选中时剪贴板仍是哨兵_还原() {
        // 模拟 Ctrl+C 什么都没复制到，剪贴板还是我们放的哨兵。
        assert!(should_restore(Some(S), Some(S), S));
    }

    #[test]
    fn 复制到了东西_是我们写的_还原() {
        // 剪贴板内容等于模拟复制的结果，说明是我们造成的改动。
        assert!(should_restore(Some("划中的文字"), Some("划中的文字"), S));
    }

    #[test]
    fn 期间用户自己复制了_绝不还原() {
        // 这是本 bug 的核心：用户划完词紧接着按 Ctrl+C，
        // 那份内容不能被划词前的旧内容盖掉。
        assert!(!should_restore(Some("用户刚复制的"), Some("划中的文字"), S));
    }

    #[test]
    fn 读回失败且剪贴板为空_还原() {
        assert!(should_restore(None, None, S));
    }

    #[test]
    fn 读回失败但剪贴板有内容_不还原() {
        assert!(!should_restore(Some("别的进程写的"), None, S));
    }

    #[test]
    fn 哨兵每次都不同() {
        // 哨兵必须唯一，否则「用户复制的内容恰好等于上一轮哨兵」会被误判。
        let mk = || {
            format!(
                "hymt2-sentinel-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0)
            )
        };
        assert_ne!(mk(), mk());
    }
}
