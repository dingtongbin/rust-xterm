//! 终端配置：实现 WezTerm 的 `TerminalConfiguration` trait
//!
//! 这是防腐层的核心组件之一。WezTerm 的配置系统极其复杂，
//! 包含数百个选项。rust-xterm 只需要一个最小化的默认配置，
//! 足以驱动状态机正常运转即可。
//!
//! 设计原则：
//! - 所有配置项在初始化时锁定，运行时不可变
//! - 使用合理的默认值，不暴露 WezTerm 的复杂配置树
//! - 宿主层可通过 `RustXtermConfigBuilder` 定制关键参数

use std::sync::Arc;
use wezterm_bidi::ParagraphDirectionHint;
use wezterm_term::color::ColorPalette;
use wezterm_term::config::{BidiMode, NewlineCanon, TerminalConfiguration};
use wezterm_term::UnicodeVersion;

/// rust-xterm 终端配置
///
/// 实现了 WezTerm 的 `TerminalConfiguration` trait，
/// 提供最小化的默认配置。
#[derive(Debug, Clone)]
pub struct RustXtermConfig {
    /// 滚动缓冲区行数
    scrollback: usize,
    /// 颜色调色板
    palette: ColorPalette,
    /// 配置代际计数器（用于缓存失效）
    generation: usize,
}

impl Default for RustXtermConfig {
    fn default() -> Self {
        Self {
            scrollback: 1000,
            palette: ColorPalette::default(),
            generation: 0,
        }
    }
}

impl RustXtermConfig {
    /// 创建配置构建器
    pub fn builder() -> RustXtermConfigBuilder {
        RustXtermConfigBuilder::default()
    }

    /// 转换为 `Arc<dyn TerminalConfiguration>`，用于注入 WezTerm
    pub fn into_arc(self) -> Arc<dyn TerminalConfiguration + Send + Sync> {
        Arc::new(self)
    }
}

impl TerminalConfiguration for RustXtermConfig {
    fn generation(&self) -> usize {
        self.generation
    }

    fn scrollback_size(&self) -> usize {
        self.scrollback
    }

    fn color_palette(&self) -> ColorPalette {
        self.palette.clone()
    }

    fn canonicalize_pasted_newlines(&self) -> NewlineCanon {
        NewlineCanon::CarriageReturnAndLineFeed
    }

    fn unicode_version(&self) -> UnicodeVersion {
        UnicodeVersion {
            version: 9,
            ambiguous_are_wide: false,
            cell_widths: None,
        }
    }

    fn bidi_mode(&self) -> BidiMode {
        BidiMode {
            enabled: false,
            hint: ParagraphDirectionHint::LeftToRight,
        }
    }
}

/// 配置构建器
#[derive(Debug, Default)]
pub struct RustXtermConfigBuilder {
    scrollback: Option<usize>,
    palette: Option<ColorPalette>,
}

impl RustXtermConfigBuilder {
    /// 设置滚动缓冲区大小
    pub fn scrollback(mut self, rows: usize) -> Self {
        self.scrollback = Some(rows);
        self
    }

    /// 设置颜色调色板
    pub fn palette(mut self, palette: ColorPalette) -> Self {
        self.palette = Some(palette);
        self
    }

    /// 构建配置
    pub fn build(self) -> RustXtermConfig {
        RustXtermConfig {
            scrollback: self.scrollback.unwrap_or(1000),
            palette: self.palette.unwrap_or_default(),
            generation: 0,
        }
    }
}
