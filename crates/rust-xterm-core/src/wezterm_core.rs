//! WezTermCore：防腐层
//!
//! 封装 `wezterm_term::Terminal`，隔离其复杂的配置系统。
//! 对外暴露最小化的、稳定的 API，使 rust-xterm 上层不直接依赖
//! WezTerm 的内部类型。
//!
//! ## 职责
//!
//! - 初始化：注入 `NullWriter`（阻断 PTY 尝试），配置 `TerminalSize`
//! - 功能映射：
//!   - `advance_bytes`：接收 UTF-8 流
//!   - `screen_snapshot`：获取当前 Grid 的快照
//!   - `resize`：传递窗口大小变化
//!   - `cursor_pos`：获取光标位置
//!   - `current_seqno`：获取当前序列号（用于脏区判断）

use crate::cell::{CellFlags, RustXtermCell};
use crate::config::RustXtermConfig;
use crate::mouse::{to_wezterm_event, KeyMods, MouseAction, MouseButton};
use crate::null_writer::{CapturingWriter, OutputBuffer};
use crate::{Color, CursorMeta, CursorShape, TerminalSize};
use std::sync::Arc;
use wezterm_term::config::TerminalConfiguration;
use wezterm_term::terminal::Terminal;
use wezterm_term::TerminalSize as WzTerminalSize;

/// WezTerm 核心防腐层
///
/// 持有 `wezterm_term::Terminal` 实例，对外提供稳定的、
/// 最小化的 API。所有 WezTerm 内部类型都被转换为 rust-xterm
/// 自己的类型，避免泄漏实现细节。
pub struct WezTermCore {
    /// WezTerm 终端实例
    terminal: Terminal,
    /// 配置引用（保持存活）
    _config: Arc<dyn TerminalConfiguration + Send + Sync>,
    /// 当前尺寸
    size: TerminalSize,
    /// 终端输出捕获缓冲区（鼠标报告、查询响应等）
    output_buffer: OutputBuffer,
}

impl WezTermCore {
    /// 创建新的 WezTerm 核心
    ///
    /// - `size`：初始终端尺寸
    /// - `config`：终端配置
    pub fn new(size: TerminalSize, config: RustXtermConfig) -> Self {
        let config_arc = config.into_arc();
        let wz_size = WzTerminalSize {
            rows: size.rows,
            cols: size.cols,
            pixel_width: 0,
            pixel_height: 0,
            dpi: 0,
        };

        let (writer, output_buffer) = CapturingWriter::new();

        let terminal = Terminal::new(
            wz_size,
            config_arc.clone(),
            "rust-xterm",
            "2.0",
            Box::new(writer),
        );

        Self {
            terminal,
            _config: config_arc,
            size,
            output_buffer,
        }
    }

    /// 取出终端产生的所有输出字节（鼠标报告、CSI/OSC 响应等）
    ///
    /// 宿主层应在每次 tick 后调用，将响应转发给 PTY。
    /// 若不调用，输出会被下次 drain 覆盖（不会无限增长）。
    pub fn drain_output(&self) -> Vec<u8> {
        CapturingWriter::drain(&self.output_buffer)
    }

    /// 是否有任何鼠标跟踪模式被启用（DECSET 1000/1002/1003）
    ///
    /// 宿主层据此决定：把鼠标事件转发给终端（`true`），还是用于本地滚动选词（`false`）。
    pub fn is_mouse_grabbed(&self) -> bool {
        self.terminal.is_mouse_grabbed()
    }

    /// 提交一个鼠标事件
    ///
    /// 若应用启用了鼠标跟踪，WezTerm 会自动按当前编码（X10/UTF-8/SGR/SgrPixels）
    /// 编码报告并写入 [`CapturingWriter`]，宿主在下次 `drain_output` 即可取出转发。
    /// 坐标 `x`/`y` 为**当前可视窗口**的 0-based 列/行（不含滚动回溯偏移）。
    pub fn mouse_event(
        &mut self,
        x: usize,
        y: usize,
        action: MouseAction,
        button: MouseButton,
        mods: KeyMods,
    ) {
        let ev = to_wezterm_event(x, y, action, button, mods);
        let _ = self.terminal.mouse_event(ev);
    }

    /// 滚动缓冲区总行数（含可视区与历史回溯）
    pub fn scrollback_rows(&self) -> usize {
        self.terminal.screen().scrollback_rows()
    }

    /// 接收 UTF-8 字节流，送入 WezTerm 状态机解析
    ///
    /// WezTerm 内部解析 ANSI 转义序列，更新 Grid。
    /// 此方法立即返回，不触发渲染。
    pub fn advance_bytes(&mut self, bytes: &str) {
        self.terminal.advance_bytes(bytes);
    }

    /// 调整终端尺寸
    pub fn resize(&mut self, size: TerminalSize) {
        let wz_size = WzTerminalSize {
            rows: size.rows,
            cols: size.cols,
            pixel_width: 0,
            pixel_height: 0,
            dpi: 0,
        };
        self.terminal.resize(wz_size);
        self.size = size;
    }

    /// 获取当前尺寸
    pub fn size(&self) -> TerminalSize {
        self.size
    }

    /// 获取当前序列号
    ///
    /// 用于脏区判断：记录上次渲染时的 seqno，
    /// 下次渲染时比较 seqno 差异即可知道哪些行变更了。
    pub fn current_seqno(&self) -> u64 {
        self.terminal.current_seqno() as u64
    }

    /// 获取窗口标题（OSC 0/2）
    pub fn title(&self) -> String {
        self.terminal.get_title().to_string()
    }

    /// 获取图标名称（OSC 1）
    ///
    /// WezTerm 不单独暴露 icon_name，这里返回 title 作为回退。
    pub fn icon_name(&self) -> String {
        self.terminal.get_title().to_string()
    }

    /// 是否处于备用屏幕（alternate screen）
    pub fn is_alt_screen_active(&self) -> bool {
        self.terminal.is_alt_screen_active()
    }

    /// 获取当前调色板
    pub fn palette(&self) -> wezterm_term::color::ColorPalette {
        self.terminal.palette()
    }

    /// 获取光标元信息
    pub fn cursor_meta(&self) -> CursorMeta {
        let pos = self.terminal.cursor_pos();
        CursorMeta {
            x: pos.x,
            y: if pos.y >= 0 { pos.y as usize } else { 0 },
            visible: matches!(pos.visibility, wezterm_surface::CursorVisibility::Visible),
            shape: convert_cursor_shape(pos.shape),
        }
    }

    /// 获取屏幕快照（当前可视窗口）
    ///
    /// 返回所有可见行的 Cell 数据。
    /// 注意：此方法会克隆数据，应在渲染时调用，不要在输入流中调用。
    pub fn screen_snapshot(&self) -> ScreenSnapshot {
        self.snapshot_scrolled(0)
    }

    /// 获取带滚动偏移的屏幕快照
    ///
    /// `scroll_offset = 0` 等价于 [`Self::screen_snapshot`]（实时可视窗口）；
    /// `scroll_offset > 0` 表示向上回溯 `scroll_offset` 行进入历史滚动缓冲。
    /// 超出可用回溯行数时自动 clamp 到最大值。
    pub fn snapshot_scrolled(&self, scroll_offset: usize) -> ScreenSnapshot {
        let screen = self.terminal.screen();
        let palette = self.terminal.palette();
        let seqno = self.current_seqno();

        let total_rows = screen.scrollback_rows();
        let vis_rows = screen.physical_rows;
        // 可向上回溯的最大行数（回溯窗口上界）
        let max_scroll = total_rows.saturating_sub(vis_rows);
        let offset = scroll_offset.min(max_scroll);

        // 可视窗口的物理行范围：[total - vis - offset, total - offset)
        let phys_start = total_rows.saturating_sub(vis_rows).saturating_sub(offset);
        let phys_end = total_rows.saturating_sub(offset);

        let lines = screen.lines_in_phys_range(phys_start..phys_end);

        let mut rows = Vec::with_capacity(vis_rows);
        for line in &lines {
            let mut row = Vec::with_capacity(screen.physical_cols);
            for cell_ref in line.visible_cells() {
                let cell = convert_cell(&cell_ref, &palette);
                row.push(cell);
            }
            // 补齐列数
            while row.len() < screen.physical_cols {
                row.push(RustXtermCell::blank());
            }
            rows.push(row);
        }

        // 补齐行数
        while rows.len() < vis_rows {
            let row = vec![RustXtermCell::blank(); screen.physical_cols];
            rows.push(row);
        }

        ScreenSnapshot {
            rows,
            size: self.size,
            seqno,
        }
    }

    /// 可向上回溯的最大行数（= 滚动缓冲总行数 - 可视行数）
    pub fn max_scrollback(&self) -> usize {
        let screen = self.terminal.screen();
        screen
            .scrollback_rows()
            .saturating_sub(screen.physical_rows)
    }

    /// 获取自指定 seqno 以来变更的行索引列表
    ///
    /// 用于增量渲染：只提取变更行的数据。
    pub fn changed_rows_since(&self, since_seqno: u64) -> Vec<usize> {
        let screen = self.terminal.screen();
        let total_rows = screen.scrollback_rows();
        let vis_rows = screen.physical_rows;
        let phys_start = total_rows.saturating_sub(vis_rows);

        let lines = screen.lines_in_phys_range(phys_start..total_rows);

        let mut changed = Vec::new();
        for (vis_y, line) in lines.iter().enumerate() {
            let line_seqno: u64 = line.current_seqno() as u64;
            if line_seqno > since_seqno {
                changed.push(vis_y);
            }
        }

        changed
    }

    /// 获取指定行的 Cell 数据（增量渲染用）
    pub fn row_cells(&self, y: usize) -> Vec<RustXtermCell> {
        let screen = self.terminal.screen();
        let palette = self.terminal.palette();

        let total_rows = screen.scrollback_rows();
        let vis_rows = screen.physical_rows;
        let phys_start = total_rows.saturating_sub(vis_rows);

        if y >= vis_rows {
            return Vec::new();
        }

        let lines = screen.lines_in_phys_range(phys_start..total_rows);
        if let Some(line) = lines.get(y) {
            let mut row = Vec::with_capacity(screen.physical_cols);
            for cell_ref in line.visible_cells() {
                let cell = convert_cell(&cell_ref, &palette);
                row.push(cell);
            }
            while row.len() < screen.physical_cols {
                row.push(RustXtermCell::blank());
            }
            row
        } else {
            vec![RustXtermCell::blank(); screen.physical_cols]
        }
    }

    /// 获取对内部 Terminal 的不可变引用（高级 API）
    ///
    /// 仅供需要直接访问 WezTerm API 的高级用户使用。
    pub fn inner(&self) -> &Terminal {
        &self.terminal
    }

    /// 获取对内部 Terminal 的可变引用（高级 API）
    pub fn inner_mut(&mut self) -> &mut Terminal {
        &mut self.terminal
    }
}

/// 屏幕快照
///
/// 包含所有可见行的 Cell 数据和元信息。
#[derive(Debug, Clone)]
pub struct ScreenSnapshot {
    /// 行数据，每行为一 Vec<RustXtermCell>
    pub rows: Vec<Vec<RustXtermCell>>,
    /// 快照时的尺寸
    pub size: TerminalSize,
    /// 快照时的序列号
    pub seqno: u64,
}

impl ScreenSnapshot {
    /// 获取指定位置的 Cell
    pub fn cell(&self, x: usize, y: usize) -> Option<&RustXtermCell> {
        self.rows.get(y)?.get(x)
    }

    /// 行数
    pub fn rows(&self) -> usize {
        self.rows.len()
    }

    /// 列数（取第一行的长度，若空则返回 0）
    pub fn cols(&self) -> usize {
        self.rows.first().map(|r| r.len()).unwrap_or(0)
    }
}

/// 转换 WezTerm CellRef 为 rust-xterm RustXtermCell
fn convert_cell(
    cell_ref: &wezterm_term::CellRef,
    palette: &wezterm_term::color::ColorPalette,
) -> RustXtermCell {
    let text = cell_ref.str().to_string();
    let width = cell_ref.width();
    let attrs = cell_ref.attrs();
    let flags = CellFlags::from_attrs(attrs);

    let fg = resolve_color(attrs.foreground(), palette, true);
    let bg = resolve_color(attrs.background(), palette, false);

    RustXtermCell {
        text,
        width,
        fg,
        bg,
        flags,
    }
}

/// 解析 WezTerm ColorAttribute 为 rust-xterm Color
fn resolve_color(
    color_attr: wezterm_term::color::ColorAttribute,
    palette: &wezterm_term::color::ColorPalette,
    is_foreground: bool,
) -> Color {
    use wezterm_term::color::ColorAttribute;

    match color_attr {
        ColorAttribute::Default => {
            if is_foreground {
                Color::WHITE
            } else {
                Color::BLACK
            }
        }
        ColorAttribute::TrueColorWithDefaultFallback(srgba) => {
            let (r, g, b, a) = srgba.as_rgba_u8();
            Color::rgba(r, g, b, a)
        }
        ColorAttribute::TrueColorWithPaletteFallback(srgba, _) => {
            let (r, g, b, a) = srgba.as_rgba_u8();
            Color::rgba(r, g, b, a)
        }
        ColorAttribute::PaletteIndex(idx) => {
            let resolved = if is_foreground {
                palette.resolve_fg(ColorAttribute::PaletteIndex(idx))
            } else {
                palette.resolve_bg(ColorAttribute::PaletteIndex(idx))
            };
            let (r, g, b, a) = resolved.as_rgba_u8();
            Color::rgba(r, g, b, a)
        }
    }
}

/// 转换 WezTerm 光标形状为 rust-xterm 光标形状
fn convert_cursor_shape(shape: wezterm_surface::CursorShape) -> CursorShape {
    use wezterm_surface::CursorShape as WzShape;
    match shape {
        WzShape::Default => CursorShape::Default,
        WzShape::BlinkingBlock | WzShape::SteadyBlock => CursorShape::Block,
        WzShape::BlinkingUnderline | WzShape::SteadyUnderline => CursorShape::Underline,
        WzShape::BlinkingBar | WzShape::SteadyBar => CursorShape::Bar,
    }
}
