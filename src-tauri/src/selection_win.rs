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

/// 取词上限。PDF 阅读器、网页这类控件可能一次返回几万字符，
/// 全量丢给模型既慢又会挤爆上下文，截断更实际。
const MAX_CHARS: usize = 5000;

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
pub fn selected_text() -> Option<String> {
    // 捕获跑在每次点击新起的线程上，COM 必须按线程初始化。
    // `UIAutomation::new()` 内部会自己调 CoInitializeEx，不用我们操心。
    let ui = UIAutomation::new().ok()?;
    let element = ui.get_focused_element().ok()?;

    // 焦点落在本应用自己身上就放弃。弹窗是 alwaysOnTop 的，弹出过程会
    // 改变焦点；不挡住的话容易自激成「弹窗→取到弹窗文字→再弹窗」。
    let element_pid = element.get_process_id().ok()?;
    if element_pid == std::process::id() {
        return None;
    }

    // 见 focus_is_foreground 的注释：UIA 焦点可能还停在上一个应用上。
    if !focus_is_foreground(element_pid) {
        return None;
    }

    let text = read_selection(&element)?;

    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    Some(text.chars().take(MAX_CHARS).collect())
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