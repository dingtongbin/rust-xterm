//! 简化 Cell 模型
//!
//! 从 WezTerm 的 `Cell` + `CellAttributes` 提取渲染所需的
//! 最小信息集，避免上层直接依赖 WezTerm 内部类型。
//!
//! ## 设计原则
//!
//! - 只保留渲染必需的字段：文本、宽度、前景色、背景色、属性标志
//! - 颜色预解析为 RGBA，避免渲染时重复查表
//! - 属性用 bitflags 压缩，减少内存占用

use crate::Color;

/// Cell 属性标志位
///
/// 使用 bitflags 压缩存储，每个标志占 1 bit。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CellFlags(pub u16);

impl CellFlags {
    /// 粗体
    pub const BOLD: u16 = 1 << 0;
    /// 斜体
    pub const ITALIC: u16 = 1 << 1;
    /// 反色
    pub const REVERSE: u16 = 1 << 2;
    /// 下划线
    pub const UNDERLINE: u16 = 1 << 3;
    /// 双下划线
    pub const DOUBLE_UNDERLINE: u16 = 1 << 4;
    /// 波浪下划线（Undercurl）
    pub const UNDERCURL: u16 = 1 << 5;
    /// 删除线
    pub const STRIKETHROUGH: u16 = 1 << 6;
    /// 闪烁
    pub const BLINK: u16 = 1 << 7;
    /// 不可见
    pub const INVISIBLE: u16 = 1 << 8;
    /// 暗淡
    pub const DIM: u16 = 1 << 9;
    /// 行尾换行标记
    pub const WRAPPED: u16 = 1 << 10;

    /// 检查是否包含指定标志
    #[inline]
    pub fn contains(&self, flag: u16) -> bool {
        self.0 & flag != 0
    }

    /// 从 WezTerm CellAttributes 转换
    pub fn from_attrs(attrs: &wezterm_term::CellAttributes) -> Self {
        use wezterm_term::Blink;
        use wezterm_term::Intensity;
        use wezterm_term::Underline;

        let mut flags = 0u16;

        match attrs.intensity() {
            Intensity::Bold => flags |= Self::BOLD,
            Intensity::Half => flags |= Self::DIM,
            _ => {}
        }

        if attrs.italic() {
            flags |= Self::ITALIC;
        }
        if attrs.reverse() {
            flags |= Self::REVERSE;
        }
        if attrs.blink() != Blink::None {
            flags |= Self::BLINK;
        }
        if attrs.invisible() {
            flags |= Self::INVISIBLE;
        }
        if attrs.strikethrough() {
            flags |= Self::STRIKETHROUGH;
        }

        match attrs.underline() {
            Underline::Single => flags |= Self::UNDERLINE,
            Underline::Double => flags |= Self::DOUBLE_UNDERLINE,
            Underline::Curly => flags |= Self::UNDERCURL,
            _ => {}
        }

        Self(flags)
    }
}

/// rust-xterm 简化 Cell
///
/// 包含渲染一个字符位置所需的全部信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustXtermCell {
    /// 该位置的文本（可能是空字符串表示空白，或多字符表示宽字符/emoji）
    pub text: String,
    /// 显示宽度（0, 1, 或 2）
    pub width: usize,
    /// 前景色（RGBA）
    pub fg: Color,
    /// 背景色（RGBA）
    pub bg: Color,
    /// 属性标志
    pub flags: CellFlags,
}

impl RustXtermCell {
    /// 创建空白 Cell
    pub fn blank() -> Self {
        Self {
            text: String::new(),
            width: 0,
            fg: Color::WHITE,
            bg: Color::BLACK,
            flags: CellFlags(0),
        }
    }

    /// 是否为空白
    pub fn is_blank(&self) -> bool {
        self.text.is_empty() || self.text == " "
    }

    /// 是否为宽字符
    pub fn is_wide(&self) -> bool {
        self.width == 2
    }
}

impl Default for RustXtermCell {
    fn default() -> Self {
        Self::blank()
    }
}
