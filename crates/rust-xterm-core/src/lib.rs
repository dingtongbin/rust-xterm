//! # rust-xterm-core: 究极工业方案 2.0 核心引擎
//!
//! 基于 WezTerm-term 的源码级剥离，提供：
//! - [`WezTermCore`]：防腐层，封装 `wezterm_term::Terminal`
//! - [`CodecGate`]：编码闸门，处理 GBK/UTF-8 边界与断包
//! - [`DamageTracker`]：脏区追踪器，记录逻辑变更
//! - [`TerminalManager`]：终极结构体，聚合所有子系统
//!
//! ## 设计哲学
//!
//! - **静态确定性**：所有内存模型在初始化时锁定，运行时零分配
//! - **绝对解耦**：核心库无 OS/GUI 依赖，可嵌入任意宿主
//! - **零开销抽象**：防腐层仅做引用转发，不引入额外拷贝
//!
//! ## 安全策略
//!
//! ```rust,ignore
//! #![forbid(unsafe_code)]
//! #![warn(missing_docs)]
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]

// ============================================================================
// 模块声明
// ============================================================================

/// 终端配置：实现 `TerminalConfiguration` trait
pub mod config;

/// NullWriter：阻断 PTY 尝试的空写入器
pub mod null_writer;

/// WezTermCore：防腐层，封装 wezterm_term::Terminal
pub mod wezterm_core;

/// CodecGate：编码闸门，处理 GBK/UTF-8 双向转换
pub mod codec_gate;

/// DamageTracker：脏区追踪器
pub mod damage;

/// 简化 Cell 模型：从 wezterm Cell 提取渲染所需信息
pub mod cell;

/// TerminalManager：终极结构体
pub mod manager;

/// 运行时状态
pub mod state;

/// Windows Terminal 主题（Campbell 配色）
pub mod theme;

/// 事件系统（xterm.js 风格）
pub mod events;

/// Buffer 与 Marker 抽象（xterm.js 风格）
pub mod buffer;

/// 自定义序列解析器（xterm.js 风格）
pub mod parser;

/// Addon 插件系统（xterm.js 风格）
pub mod addon;

/// 鼠标事件抽象（xterm.js 风格）
pub mod mouse;

/// 键盘映射核心层（xterm.js 风格）
pub mod input;

/// 选区系统模型（xterm.js 风格）
pub mod selection;

/// GUI 集成抽象
pub mod integration;

// ============================================================================
// 公共重导出
// ============================================================================

pub use addon::{Addon, AddonContext};
pub use buffer::{Buffer, BufferNamespace, BufferType, Marker};
pub use cell::{CellFlags, RustXtermCell};
pub use codec_gate::{Codec, CodecGate, CodecStats};
pub use config::{RustXtermConfig, RustXtermConfigBuilder};
pub use damage::{DamageTracker, DirtyRect};
pub use events::{EventBus, EventSubscription, TerminalEvent};
pub use input::{KeyInput, KeyMapping};
pub use integration::{InputSource, NullRenderSurface, RenderMetrics, RenderSurface, SizeSource};
pub use manager::{DirtyRow, FrameUpdate, TerminalManager};
pub use mouse::{KeyMods, MouseAction, MouseButton, MouseState};
pub use null_writer::{CapturingWriter, NullWriter, OutputBuffer};
pub use parser::Parser;
pub use selection::SelectionRange;
pub use state::RuntimeState;
pub use theme::WindowsTerminalTheme;
pub use wezterm_core::{ScreenSnapshot, WezTermCore};

/// 终端尺寸（逻辑行列数）
///
/// 对应 wezterm 的 `TerminalSize`，但剥离了像素尺寸与 DPI，
/// 因为这些属于渲染层职责。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalSize {
    /// 可见行数
    pub rows: usize,
    /// 可见列数
    pub cols: usize,
}

impl Default for TerminalSize {
    fn default() -> Self {
        Self { rows: 24, cols: 80 }
    }
}

impl TerminalSize {
    /// 创建新的终端尺寸
    pub const fn new(rows: usize, cols: usize) -> Self {
        Self { rows, cols }
    }
}

/// 光标元信息
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorMeta {
    /// 光标 X 坐标（列，0-based）
    pub x: usize,
    /// 光标 Y 坐标（行，0-based）
    pub y: usize,
    /// 光标是否可见
    pub visible: bool,
    /// 光标形状
    pub shape: CursorShape,
}

/// 光标形状
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorShape {
    /// 默认（通常是块状）
    Default,
    /// 块状
    Block,
    /// 竖线
    Bar,
    /// 下划线
    Underline,
}

impl Default for CursorMeta {
    fn default() -> Self {
        Self {
            x: 0,
            y: 0,
            visible: true,
            shape: CursorShape::Default,
        }
    }
}

/// RGBA 颜色（8-bit 通道）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Color {
    /// 红色通道
    pub r: u8,
    /// 绿色通道
    pub g: u8,
    /// 蓝色通道
    pub b: u8,
    /// Alpha 通道
    pub a: u8,
}

impl Color {
    /// 创建新的 RGBA 颜色
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// 创建不透明 RGB 颜色
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    /// 黑色
    pub const BLACK: Self = Self::rgb(0, 0, 0);

    /// 白色
    pub const WHITE: Self = Self::rgb(255, 255, 255);
}
