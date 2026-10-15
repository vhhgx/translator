//! Windows 上的取词实现：UI Automation。
//!
//! **完全不碰剪贴板。** 早先的实现是「清空剪贴板当哨兵 → 模拟 Ctrl+C →
//! 读回 → 还原」，那套做法会短暂清空用户剪贴板，还原时又可能盖掉用户
//! 在那 160ms 里自己复制的内容——表现出来就是「复制什么都不进去了」。
//! 借剪贴板中转本质上有损，不该用在用户天天在用的功能上。
//!
//! UIA 直接问目标应用「你当前选中了什么」，走的是应用自己的状态：
//! 既不惊动系统，也就不存在抢剪贴板、覆盖用户数据的问题。
//!
//! 代价是**覆盖面**：没实现 UIA 的控件（部分终端、少数游戏、自绘界面）
//! 取不到选区，此时直接不弹窗，而不是退回剪贴板——宁可少划一次，
//! 也不能再动用户剪贴板。

use uiautomation::patterns::{UILegacyIAccessiblePattern, UITextPattern};
use uiautomation::{UIElement, UIAutomation};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId,
};

/// 与 selection.rs 里同名宏等价的追踪，单独定义是因为宏不会跨模块可见。
/// 取词失败要能说清卡在哪一步，否则「不弹窗」永远只能靠猜。
macro_rules! trace {
    ($($arg:tt)*) => {
        if std::env::var("HYMT2_SEL_TRACE").map(|v| v == "1").unwrap_or(false) {
            eprintln!("[sel:uia] {}", format!($($arg)*));
        }
    };
}

/// 取词上限。PDF 阅读器、网页这类控件可能一次返回几万字符，
/// 全量丢给模型既慢又会挤爆上下文，截断更实际。
const MAX_CHARS: usize = 5000;

/// 焦点控件不支持取词时，往上最多试几层祖先。
const WALK_DEPTH: usize = 6;

/// 往祖先链上找不到、改为向下广搜时，最多翻多少个节点。
/// 重页面整棵树能上万，逐个问是 COM 往返乘以上万，得封顶。
const DESCENDANT_LIMIT: usize = 400;

/// MSAA `STATE_SYSTEM_SELECTED`。用来区分「元素被选中了」（它的内容才是选区）
/// 和「元素只是有个标签」（按钮、菜单项——那是 Name，不是选区）。
const STATE_SYSTEM_SELECTED: u32 = 0x0000_0002;

/// 当前前台窗口所属进程；取不到返回 None。
///
/// 单独抽出来是为了能直接单测，不必真去开一个窗口。
fn foreground_pid() -> Option<u32> {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_null() {
        return None;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, &raw mut pid) };
    (pid != 0).then_some(pid)
}

/// UIA 焦点所在进程是否就是当前前台窗口所属进程。
///
/// 这条校验是划词会不会「莫名其妙自己弹」的关键。
///
/// `IUIAutomation::GetFocusedElement` 返回的是**UIA 焦点**，它和 Windows
/// 的真实前台焦点**不是一回事**：点进一个不参与 UIA 的地方（任务栏、桌面、
/// 游戏、某些工具窗口）时，Windows 的前台已经换人了，但 UIA 焦点可能仍旧
/// 留在上一个文本控件上——**而那个控件的选区通常还在**。
///
/// 不校验的话，用户点一下任务栏，我们就把上一个窗口的旧选区又翻译一遍，
/// 这正是「自己莫名唤出、且无法稳定复现」的来源。
///
/// 取不到前台进程就当作「不是」：宁可这次不划词。
fn focus_is_foreground(element_pid: u32) -> bool {
    foreground_pid() == Some(element_pid)
}

/// 读当前焦点控件的选中文字；没有选区、或控件不支持 UIA 时返回 None。
///
/// **会重试。** Chromium 系（Chrome / Edge）第一次响应 UIA 查询时，会同步把
/// 无障碍打开并**重建整棵可访问性树**。页面越重越慢：普通页面几十毫秒就绪，
/// GitHub 这类重 React 站点要久得多。单次读取必然读到尚未刷新的空树，
/// 表现就是「别的网站划词都正常，GitHub 上压根不弹窗」。隔一会儿多试几次即可。
/// 重试很便宜——纯读，不发按键，也不碰剪贴板。
///
/// 代价：点在没有选区的地方时要多花约 700ms 才判定为「没选词」。
/// 这段跑在每次点击新起的线程上，不占监听线程，界面无感。
pub fn selected_text() -> Option<String> {
    const GAPS_MS: [u64; 5] = [40, 80, 150, 200, 250];

    for i in 0..=GAPS_MS.len() {
        if let Some(t) = try_read() {
            return Some(t);
        }
        if let Some(g) = GAPS_MS.get(i) {
            std::thread::sleep(std::time::Duration::from_millis(*g));
        }
    }
    None
}

fn try_read() -> Option<String> {
    // 捕获跑在每次点击新起的线程上，COM 必须按线程初始化。
    // `UIAutomation::new()` 内部会自己调 CoInitializeEx，不用我们操心。
    let ui = match UIAutomation::new() {
        Ok(u) => u,
        Err(e) => {
            trace!("UIAutomation::new() 失败：{e}");
            return None;
        }
    };
    let element = match ui.get_focused_element() {
        Ok(e) => e,
        Err(e) => {
            trace!("get_focused_element() 失败：{e}");
            return None;
        }
    };

    // 焦点落在本应用自己身上就放弃。弹窗是 alwaysOnTop 的，弹出过程会
    // 改变焦点；不挡住的话容易自激成「弹窗→取到弹窗文字→再弹窗」。
    let element_pid = match element.get_process_id() {
        Ok(p) => p,
        Err(e) => {
            trace!("get_process_id() 失败：{e}");
            return None;
        }
    };
    if element_pid == std::process::id() {
        trace!("焦点在本进程内（pid={element_pid}）→ 放弃");
        return None;
    }

    // 见 focus_is_foreground 的注释：UIA 焦点可能还停在上一个应用上。
    match (focus_is_foreground(element_pid), foreground_pid()) {
        (true, _) => {}
        (false, fg) => {
            trace!(
                "UIA 焦点 pid={element_pid} 与前台窗口 pid={fg:?} 不一致 → 放弃"
            );
            return None;
        }
    }

    let name = element.get_name().unwrap_or_default();
    let ctype = element.get_control_type().map(|c| format!("{c:?}")).unwrap_or_default();
    let text = match read_selection_walk(&ui, &element) {
        Some(t) => t,
        None => {
            trace!(
                "焦点控件「{name}」({ctype}) 及其 {WALK_DEPTH} 层祖先都不支持取词，或选区为空"
            );
            return None;
        }
    };

    let text = text.trim();
    if text.is_empty() {
        trace!("控件「{name}」({ctype}) 读到的是空白");
        return None;
    }
    Some(text.chars().take(MAX_CHARS).collect())
}

/// 往祖先链上找能读出选区的元素。
///
/// 焦点控件本身常常只是个容器——Chromium 里选完文字，`GetFocusedElement`
/// 给回的多半是承载正文的那层 div 或 Document，它自己不一定实现了
/// TextPattern，真正的选区却挂在它下面的文本节点上。只认焦点元素的话，
/// 重 DOM 的页面（GitHub 这类嵌套深的）就一条都读不出来。
///
/// 逐层往上试，命中即止；深度有限，避免在整棵树的深处空转。
fn read_selection_walk(ui: &UIAutomation, element: &UIElement) -> Option<String> {
    // ControlViewWalker 只走「控件视图」，比原始视图干净，不会掉进纯装饰节点。
    let walker = match ui.get_control_view_walker() {
        Ok(w) => w,
        Err(e) => {
            trace!("拿不到 tree walker：{e}");
            return None;
        }
    };

    // 先看焦点元素本身，再逐层往上——记事本、VSCode 这类标准控件的焦点
    // 就在取词元素上，多数情况第 0 层就中了。
    let mut cur = element.clone();
    for depth in 0..WALK_DEPTH {
        if let Some(t) = read_selection(&cur) {
            trace!("在第 {depth} 层取到选区：{} 字", t.trim().chars().count());
            return Some(t);
        }
        match walker.get_parent(&cur) {
            Ok(p) => cur = p,
            Err(_) => break,
        }
    }

    // 往上是死的，就往下找。
    //
    // 浏览器里选完文字，GetFocusedElement 给回的往往就是 Document 本身
    // （名字是页面标题），它上面没有祖先，真正的选区挂在文档**下面**某个
    // 文本节点上。GitHub 这类重页面尤其如此：只认焦点元素的话一条都读不出来，
    // 而这恰恰是「别的网站好好的，GitHub 上不弹窗」的原因。
    //
    // 广度优先 + 数量封顶：整棵可访问性树在重页面上能上万，逐个问一遍是
    // 几十毫秒的 COM 往返乘以上万，不现实。封顶取个够用的量，命中即止。
    let mut queue = std::collections::VecDeque::new();
    if let Ok(child) = walker.get_first_child(element) {
        queue.push_back(child);
    }
    let mut visited = 0usize;
    while let Some(node) = queue.pop_front() {
        if visited >= DESCENDANT_LIMIT {
            trace!("已翻 {DESCENDANT_LIMIT} 个节点，放弃向下找");
            return None;
        }
        visited += 1;
        if let Some(t) = read_selection(&node) {
            trace!(
                "向下第 {visited} 个节点取到选区：{} 字",
                t.trim().chars().count()
            );
            return Some(t);
        }
        if let Ok(c) = walker.get_first_child(&node) {
            queue.push_back(c);
        }
        let mut sib = node.clone();
        while let Ok(next) = walker.get_next_sibling(&sib) {
            if visited >= DESCENDANT_LIMIT {
                break;
            }
            queue.push_back(next.clone());
            sib = next;
        }
    }
    None
}

/// 依次尝试两种取法，覆盖面比只认一种好不少：
///
/// - **TextPattern**：标准文本控件（记事本、VSCode、Office、浏览器正文）。
///   划词翻译的主要场景都走这条。
/// - **LegacyIAccessible**：老控件和自绘界面的兜底。名字里带 Legacy，
///   但覆盖面意外地广，很多老程序只实现了它。选中项的 Name 即其文本。
///
/// 返回的可能是多段（多选），按顺序拼起来。
fn read_selection(element: &UIElement) -> Option<String> {
    if let Ok(pattern) = element.get_pattern::<UITextPattern>() {
        if let Ok(ranges) = pattern.get_selection() {
            let mut text = String::new();
            for r in ranges {
                // -1 = 不限长度；真正的上限在 selected_text 里统一截。
                if let Ok(part) = r.get_text(-1) {
                    text.push_str(&part);
                }
            }
            if !text.trim().is_empty() {
                return Some(text);
            }
        }
    }

    if let Ok(pattern) = element.get_pattern::<UILegacyIAccessiblePattern>() {
        // **必须确认元素真的处于「选中」态，否则一律不信。**
        //
        // LegacyIAccessible 的 get_selection() 返回的是「被判为选中的子项」的
        // Name。对按钮、菜单项、链接来说，那个 Name 就是**它自己的标签**——
        // 于是点一下微信侧边栏的「微信」按钮，读回来的「选中文字」就成了
        // 「微信」，划词框凭空冒出来。
        //
        // 上一版没判这个状态，误触发只是零星发生（要正好轮到这个元素被读到）；
        // 加了向下广搜之后几乎必中，点哪儿都弹窗。
        let selected = pattern.get_state().unwrap_or(0) & STATE_SYSTEM_SELECTED != 0;
        if !selected {
            let name = element.get_name().unwrap_or_default();
            trace!("跳过「{name}」：Legacy 兜底但未处于选中态（标签≠选区）");
            return None;
        }
        if let Ok(items) = pattern.get_selection() {
            let mut text = String::new();
            for item in items {
                if let Ok(name) = item.get_name() {
                    text.push_str(&name);
                }
            }
            if !text.trim().is_empty() {
                return Some(text);
            }
        }
    }

    None
}