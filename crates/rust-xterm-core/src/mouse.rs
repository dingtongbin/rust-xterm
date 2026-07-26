//! 鼠标事件抽象（防腐层）
//!
//! 暴露与 xterm.js 风格对齐的、与 WezTerm 内部类型解耦的鼠标 API。
//! 宿主层只需构造 [`MouseEvent`] 并交给 [`crate::TerminalManager::mouse_event`]，
//! 由核心层负责转换为 WezTerm 的 `wezterm_term::input::MouseEvent` 并提交。
//!
//! ## 工作流
//!
//! 1. 宿主 GUI 捕获原始鼠标事件（按下/释放/移动/滚轮）
//! 2. 转换为 rust-xterm 的 [`MouseEvent`]，调用 `TerminalManager::mouse_event`
//! 3. 若应用启用了鼠标跟踪模式，WezTerm 会自动编码报告并写入 [`crate::CapturingWriter`]
//! 4. 宿主在每次 tick 后调用 [`crate::TerminalManager::drain_output`] 取出报告字节
//!    并转发给 PTY，从而完成鼠标 → 子进程的闭环

use std::time::Instant;

/// 修饰键状态
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyMods {
    /// Shift 键
    pub shift: bool,
    /// Alt 键
    pub alt: bool,
    /// Ctrl 键
    pub ctrl: bool,
}

/// 鼠标选区状态机
///
/// 在非鼠标跟踪模式下，由 [`crate::TerminalManager`] 持有以实现
/// 单击拖拽选区、双击选词、三击选行的交互。
///
/// 点击计数遵循 500ms 时间窗与同位置约束，循环 `1 → 2 → 3 → 1`。
/// 坐标统一为 `(row, col)` 0-based，与 [`crate::SelectionRange`] 一致。
#[derive(Debug, Clone)]
pub struct MouseState {
    /// 是否处于选区拖拽中
    pub selecting: bool,
    /// 当前拖拽起点 `(row, col)`
    pub select_start: (usize, usize),
    /// 当前点击计数（1/2/3）
    pub click_count: u32,
    /// 上次点击时间
    pub last_click_time: Instant,
    /// 上次点击位置 `(row, col)`
    pub last_click_pos: (usize, usize),
}

impl Default for MouseState {
    fn default() -> Self {
        Self {
            selecting: false,
            select_start: (0, 0),
            click_count: 0,
            last_click_time: Instant::now(),
            last_click_pos: (0, 0),
        }
    }
}

/// 鼠标按键
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    /// 左键
    Left,
    /// 中键
    Middle,
    /// 右键
    Right,
    /// 无按键（用于移动事件）
    None,
}

/// 鼠标动作
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseAction {
    /// 按下
    Press,
    /// 释放
    Release,
    /// 移动
    Move,
    /// 滚轮向上滚动 `n` 格
    WheelUp(u32),
    /// 滚轮向下滚动 `n` 格
    WheelDown(u32),
}

/// 将 rust-xterm 鼠标抽象转换为 WezTerm 的 `MouseEvent`
pub(crate) fn to_wezterm_event(
    x: usize,
    y: usize,
    action: MouseAction,
    button: MouseButton,
    mods: KeyMods,
) -> wezterm_term::input::MouseEvent {
    use wezterm_term::input::{KeyModifiers, MouseButton as WzButton, MouseEvent, MouseEventKind};

    let kind = match action {
        MouseAction::Press | MouseAction::Release => match action {
            MouseAction::Press => MouseEventKind::Press,
            _ => MouseEventKind::Release,
        },
        MouseAction::Move => MouseEventKind::Move,
        // 滚轮按 Press/Release 处理；WezTerm 内部用 MouseButton::WheelUp/Down 区分
        MouseAction::WheelUp(_) => MouseEventKind::Press,
        MouseAction::WheelDown(_) => MouseEventKind::Press,
    };

    let wz_button = match (action, button) {
        (MouseAction::WheelUp(n), _) => WzButton::WheelUp(n as usize),
        (MouseAction::WheelDown(n), _) => WzButton::WheelDown(n as usize),
        (_, MouseButton::Left) => WzButton::Left,
        (_, MouseButton::Middle) => WzButton::Middle,
        (_, MouseButton::Right) => WzButton::Right,
        (_, MouseButton::None) => WzButton::None,
    };

    let mut modifiers = KeyModifiers::NONE;
    if mods.shift {
        modifiers |= KeyModifiers::SHIFT;
    }
    if mods.alt {
        modifiers |= KeyModifiers::ALT;
    }
    if mods.ctrl {
        modifiers |= KeyModifiers::CTRL;
    }

    MouseEvent {
        kind,
        x,
        y: y as i64,
        x_pixel_offset: 0,
        y_pixel_offset: 0,
        button: wz_button,
        modifiers,
    }
}
