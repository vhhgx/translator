//! 划词翻译：监听全局鼠标，选中文本后自动在光标处弹窗。
//!
//! 「怎么把选中的文字取出来」按平台分两套，**两套都不碰键盘**：
//!
//! - **Windows 走 UI Automation**（见 `selection_win`）。直接问目标应用
//!   「你当前选中了什么」。
//! - **Linux/X11 读 PRIMARY selection**。X11 的规矩是：鼠标选中即自动把这段
//!   文字放进 PRIMARY，供中键粘贴用。它和剪贴板（CLIPBOARD）是两个独立的
//!   通道，**用户主动复制的内容不受任何影响**。
//!
//! 早先 Linux/macOS 走的是「写哨兵 → 模拟 Ctrl+C → 读剪贴板 → 还原」。
//! **这条路已经废弃，不要再往回改。** 原因是致命的：合成的 Ctrl+C 会打进
//! 当时的前台窗口，而终端里 Ctrl+C 就是 SIGINT —— 用户在前台开着终端时划一次
//! 词，正在跑的命令就被打断了。除此之外它还会占用用户的剪贴板、和剪贴板管理器
//! 抢属主。读 PRIMARY 一次都不碰键盘，也不碰 CLIPBOARD。
//!
//! 监听本身用 `rdev` 而不是 enigo，是因为 enigo 在 Linux 上会硬链
//! `libxdo`，多一个系统依赖；rdev 走 XTest，够用且已经在了。
//!
//! 平台限制：
//!   - **Wayland 下划词整体不可用**：普通客户端既读不到别的应用的选区，
//!     也没有全局鼠标位置可监听。
//!   - macOS 暂未实现（需要一个不模拟按键的取词通道，如 Accessibility API），
//!     取词直接返回 None，不会弹窗、也不会动键盘。
//!   - Windows 上未实现 UIA 的控件取不到选区，此时不弹窗。
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

/// 取词实现：Windows 用 UIA，Linux/X11 读 PRIMARY selection。
///
/// **任何平台都不许用模拟按键取词**——见模块头部说明。
#[cfg(windows)]
fn capture_selected_text() -> Option<String> {
    crate::selection_win::selected_text()
}

#[cfg(target_os = "linux")]
fn capture_selected_text() -> Option<String> {
    capture_primary_selection()
}

/// 其它平台（macOS 等）暂未实现非侵入式取词，直接不弹。
///
/// 宁可这次不弹，也不能为了取词去模拟按键——那会把用户的键盘抢过来，
/// 在终端里就是一次 SIGINT。
#[cfg(all(not(windows), not(target_os = "linux")))]
fn capture_selected_text() -> Option<String> {
    None
}

/// 清掉上一次残留的哨兵。
///
/// 哨兵是旧的 Ctrl+C 取词方案留下的：捕获那 ~250ms 里剪贴板被写入一段
/// `hymt2-sentinel-*` 占位文本。现在不再写哨兵了，但**用户剪贴板里可能还留着
/// 上一个版本进程死掉时没来得及还原的那一条**，所以启动时仍然扫一次清掉。
/// 只读、且只在确认是哨兵时才写，不会动用户自己复制的内容。
#[cfg(not(windows))]
pub fn repair_clipboard() {
    if let Ok(mut cb) = arboard::Clipboard::new() {
        if let Ok(text) = cb.get_text() {
            if text.starts_with("hymt2-sentinel-") {
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
/// 弹窗是否被用户钉住。钉住期间点击别处不收起，新划词也不打扰当前内容。
static PINNED: AtomicBool = AtomicBool::new(false);
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

/// 设置/取消钉住。取消钉住时如果弹窗还开着，就保持现状（下次点击别处才收起）。
pub fn set_pinned(pinned: bool) {
    PINNED.store(pinned, Ordering::Relaxed);
}

fn pinned() -> bool {
    PINNED.load(Ordering::Relaxed)
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

/// 诊断开关：置 `HYMT2_SEL_TRACE=1` 时把判定链逐环打到 stderr。
///
/// 划词的每个卡点都是静默 `return`——「划了不弹」时没法知道是哪一环拦的。
/// 这个开关只服务于排查，默认关闭：正常使用时不产生任何输出。
/// 用 OnceLock 读一次，免得在鼠标事件热路径上反复查环境变量。
fn trace_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("HYMT2_SEL_TRACE").map(|v| v == "1").unwrap_or(false))
}

macro_rules! trace {
    ($($arg:tt)*) => {
        if trace_on() {
            eprintln!("[sel] {}", format!($($arg)*));
        }
    };
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
            // 钉住时点击别处不收起——这正是钉住的意义。用户会用关闭按钮收起。
            if !cursor_in_popup(app) && !pinned() {
                hide(app);
            }
        }
        EventType::ButtonRelease(Button::Left) => {
            // 在本应用自己的窗口里点击是一次界面交互，不是划词，必须拦在前面。
            // 尤其在设置页录制热键时，若在这里触发取词，会打断用户的录制。
            //
            // 放在 debounce_ok() 之前判断：界面内的点击不应该消耗掉去抖窗口，
            // 否则紧接着的正常划词会被误判成频率过高而丢掉。
            if cursor_in_own_window(app) {
                let (cx, cy) = cursor();
                trace!("拦：点击落在本应用自己的窗口内（{cx},{cy}）→ 当作界面交互");
                return;
            }
            let enabled = ENABLED.load(Ordering::Relaxed);
            let mod_ok = modifier_ok();
            let debounce = debounce_ok();
            if !enabled || !mod_ok || !debounce {
                trace!(
                    "拦：enabled={enabled} modifier_ok={mod_ok} debounce_ok={debounce}"
                );
                return;
            }
            trace!("左键抬起 → 开始抓取");
            // 抓取要模拟按键 + sleep，约 150ms，不能占着监听线程。
            let app = app.clone();
            std::thread::spawn(move || capture_and_show(&app));
        }
        _ => {}
    }
}

fn capture_and_show(app: &AppHandle) {
    // 钉住期间不打扰：新划词不覆盖当前内容，也不把窗口挪到光标处。
    if pinned() {
        if let Some(w) = app.get_webview_window(POPUP_LABEL) {
            if w.is_visible().unwrap_or(false) {
                return;
            }
        }
    }
    let Some(raw) = capture_selected_text() else {
        trace!("抓取失败：没取到文本（无选区，或 PRIMARY 读不到）");
        // 没选区 = 用户把选区清掉了。这之后重新选中同样的文字，是一次
        // 真正的重新划词，该弹还得弹。
        LAST_SHOWN.lock().unwrap().take();
        return;
    };
    let text = raw.trim();
    // 空 = 这次点击没选中任何东西。
    if text.is_empty() {
        return;
    }

    // 选中的还是同一段文字 → 不是新划词，是**旧选区还在**。
    //
    // 选完文字后点滚动条、工具栏、窗口里的非文本区域，都不会清掉选区，
    // UIA 焦点也仍停在那个文本控件上。此时若照常弹窗，用户看到的是
    // 「点哪儿窗口就跟到哪儿，还把刚才那段又翻了一遍」——因为
    // show_popup 会把窗口挪到光标处。
    //
    // 注意判据不能写成「弹窗可见且文字相同」：点空白处时 ButtonPress 已经
    // 先把弹窗藏了，那样这个条件永远不成立，等于没判（这正是上一版的 bug）。
    // 真正要问的是「用户有没有重新划词」，所以只看文字本身，不看弹窗状态。
    if last_shown().as_deref() == Some(text) {
        trace!("拦：与上次弹窗的文本完全相同（{} 字）→ 视为旧选区未变", text.chars().count());
        return;
    }

    *LAST_SHOWN.lock().unwrap() = Some(text.to_string());
    show_popup(app, text);
}

/// 上一次弹窗展示的原文（已 trim）。
fn last_shown() -> Option<String> {
    LAST_SHOWN.lock().unwrap().clone()
}

/// 读 PRIMARY selection，带重试。
///
/// X11 的 PRIMARY 由**属主进程**持有：选中文字的进程负责在别人来要时把内容
/// 发过去。请求要走一次 X 往返，属主忙（正在重绘、加载）时可能超时或返回空，
/// 所以重试几次。重试很便宜——只读、不发按键、不占剪贴板。
///
/// 装了剪贴板管理器（CopyQ 等）也不受影响：它们管的是 CLIPBOARD，
/// 和 PRIMARY 是两个独立通道。
#[cfg(target_os = "linux")]
fn capture_primary_selection() -> Option<String> {
    use arboard::{Clipboard, GetExtLinux, LinuxClipboardKind};

    let tries = 3;
    let gap = Duration::from_millis(20);
    let mut last_err = None;
    for _ in 0..tries {
        match Clipboard::new() {
            Ok(mut cb) => match cb.get().clipboard(LinuxClipboardKind::Primary).text() {
                Ok(t) => return Some(t),
                Err(e) => last_err = Some(e.to_string()),
            },
            Err(e) => last_err = Some(e.to_string()),
        }
        std::thread::sleep(gap);
    }
    if let Some(e) = last_err {
        trace!("PRIMARY 读取失败（重试 {tries} 次后）：{e}");
    }
    None
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

// 取词已改为读 PRIMARY selection，不再有剪贴板还原判定要测。
// 保留的测试覆盖「残留哨兵识别」——它决定启动时会不会误清用户的剪贴板。
#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;

    /// 与 `repair_clipboard` 里判据一致：只有以哨兵前缀开头的才清。
    fn is_leftover_sentinel(text: &str) -> bool {
        text.starts_with("hymt2-sentinel-")
    }

    #[test]
    fn 识别残留哨兵() {
        assert!(is_leftover_sentinel("hymt2-sentinel-123-456"));
        assert!(is_leftover_sentinel("hymt2-sentinel-"));
    }

    #[test]
    fn 用户正常内容不会被误判为哨兵() {
        // 这些绝不能触发清理，否则用户剪贴板会被清空。
        for s in [
            "用户复制的一段普通文字",
            "",
            "hymt2-sentinel",   // 少一个连字符
            "hy-mt2-sentinel-1", // 前缀不对
            " hymt2-sentinel-1", // 前面有空格
            "https://example.com/hymt2-sentinel-1", // 前缀不在开头
        ] {
            assert!(!is_leftover_sentinel(s), "误判为哨兵: {s:?}");
        }
    }
}
