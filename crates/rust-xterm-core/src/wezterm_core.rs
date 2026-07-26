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
    /// 焦点报告模式（DECSET 1004）是否启用
    ///
    /// WezTerm 内部以私有字段 `focus_tracking` 维护此状态，
    /// 未暴露查询 API，故在此镜像一份供 [`Self::is_focus_reporting_enabled`] 使用。
    /// 在 [`Self::advance_bytes`] 中扫描 `\x1b[?1004h` / `\x1b[?1004l` 维护。
    focus_reporting_enabled: bool,
    /// 当前滚动区域（DECSTBM），1-based `(top, bottom)`，`None` 表示全屏
    ///
    /// WezTerm 内部以私有字段 `top_and_bottom_margins` 维护，
    /// 未暴露查询 API，故在此镜像一份供 [`Self::scroll_region`] 使用。
    /// 在 [`Self::advance_bytes`] 中扫描 `\x1b[<top>;<bottom>r` 维护。
    scroll_region: Option<(usize, usize)>,
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
            focus_reporting_enabled: false,
            scroll_region: None,
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
    ///
    /// 同时镜像扫描焦点报告（DECSET 1004）与滚动区域（DECSTBM）序列，
    /// 维护 [`Self::focus_reporting_enabled`] 与 [`Self::scroll_region`] 状态，
    /// 因为 WezTerm 未公开这两个字段的查询 API。
    pub fn advance_bytes(&mut self, bytes: &str) {
        // 先扫描镜像状态，再喂入 WezTerm（WezTerm 也会处理这些序列，互不干扰）
        self.scan_csi_state(bytes.as_bytes());
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
        // 尺寸变化后缓存的滚动区域可能越界，重置为全屏
        self.scroll_region = None;
    }

    /// 是否启用了焦点报告模式（DECSET 1004）
    ///
    /// 当应用发送 `\x1b[?1004h` 时启用，`\x1b[?1004l` 时禁用。
    pub fn is_focus_reporting_enabled(&self) -> bool {
        self.focus_reporting_enabled
    }

    /// 通知终端焦点状态变化
    ///
    /// 若启用了焦点报告模式（DECSET 1004），WezTerm 会向输出缓冲写入
    /// `\x1b[I`（`focused = true`）或 `\x1b[O`（`focused = false`），
    /// 宿主在下次 [`Self::drain_output`] 即可取出转发给 PTY。
    ///
    /// 委托给 WezTerm 的 `focus_changed`，同时更新其内部焦点状态
    /// （用于 `has_unseen_output` 等）。
    pub fn set_focused(&mut self, focused: bool) {
        self.terminal.focus_changed(focused);
    }

    /// 获取当前滚动区域（DECSTBM），1-based `(top, bottom)`
    ///
    /// 返回 `None` 表示全屏（未设置 DECSTBM 或已重置）。
    /// 坐标为 1-based，与 DECSTBM 序列参数一致。
    pub fn scroll_region(&self) -> Option<(usize, usize)> {
        self.scroll_region
    }

    /// 扫描 CSI 序列以镜像维护焦点报告与滚动区域状态
    ///
    /// 仅识别两类序列：
    /// - `\x1b[?1004h` / `\x1b[?1004l`：DECSET/DECRST 1004
    /// - `\x1b[<top>;<bottom>r`：DECSTBM（空参数表示重置为全屏）
    ///
    /// 此扫描不消耗输入（WezTerm 仍会正常处理这些序列），
    /// 仅用于维护本地镜像状态以支持查询 API。
    fn scan_csi_state(&mut self, bytes: &[u8]) {
        let mut i = 0;
        let len = bytes.len();
        while i < len {
            // 识别 CSI 起点：ESC [ (0x1b 0x5b)
            if bytes[i] == 0x1b && i + 1 < len && bytes[i + 1] == 0x5b {
                let start = i + 2;
                let mut j = start;
                // 参数字节：0x30..=0x3f（含数字、';'、':'、'?'、'<'、'='、'>'）
                while j < len && (0x30..=0x3f).contains(&bytes[j]) {
                    j += 1;
                }
                let param_bytes = &bytes[start..j];
                // 中间字节：0x20..=0x2f
                while j < len && (0x20..=0x2f).contains(&bytes[j]) {
                    j += 1;
                }
                // 最终字节：0x40..=0x7e
                if j < len && (0x40..=0x7e).contains(&bytes[j]) {
                    let final_byte = bytes[j];
                    // 区分私有序列（以 '?' 开头）
                    let (is_private, params) = if !param_bytes.is_empty() && param_bytes[0] == b'?'
                    {
                        (true, &param_bytes[1..])
                    } else {
                        (false, param_bytes)
                    };
                    if is_private {
                        // DECSET/DECRST：final byte 'h' 或 'l'
                        if final_byte == b'h' || final_byte == b'l' {
                            if let Some(code) = parse_first_uint(params) {
                                if code == 1004 {
                                    self.focus_reporting_enabled = final_byte == b'h';
                                }
                            }
                        }
                    } else if final_byte == b'r' {
                        // DECSTBM
                        self.scroll_region = parse_decstbm(params, self.size.rows);
                    }
                    i = j + 1;
                    continue;
                }
                // 不完整的 CSI，跳过 ESC
                i += 1;
                continue;
            }
            i += 1;
        }
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

    /// 是否启用了括号粘贴模式（bracketed paste）
    ///
    /// 当应用发送 `\x1b[?2004h` 时启用，`\x1b[?2004l` 时禁用。
    /// WezTerm 内部维护此状态，这里直接委托。
    pub fn is_bracketed_paste_enabled(&self) -> bool {
        self.terminal.bracketed_paste_enabled()
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
    pub fn screen_snapshot(&self, default_fg: Color, default_bg: Color) -> ScreenSnapshot {
        self.snapshot_scrolled(0, default_fg, default_bg)
    }

    /// 获取带滚动偏移的屏幕快照
    ///
    /// `scroll_offset = 0` 等价于 [`Self::screen_snapshot`]（实时可视窗口）；
    /// `scroll_offset > 0` 表示向上回溯 `scroll_offset` 行进入历史滚动缓冲。
    /// 超出可用回溯行数时自动 clamp 到最大值。
    ///
    /// - `default_fg`：默认前景色，用于 `ColorAttribute::Default` 的前景色回退
    /// - `default_bg`：默认背景色，用于 `ColorAttribute::Default` 的背景色回退
    pub fn snapshot_scrolled(
        &self,
        scroll_offset: usize,
        default_fg: Color,
        default_bg: Color,
    ) -> ScreenSnapshot {
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
                let cell = convert_cell(&cell_ref, &palette, default_fg, default_bg);
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
    ///
    /// - `default_fg`：默认前景色，用于 `ColorAttribute::Default` 的前景色回退
    /// - `default_bg`：默认背景色，用于 `ColorAttribute::Default` 的背景色回退
    pub fn row_cells(&self, y: usize, default_fg: Color, default_bg: Color) -> Vec<RustXtermCell> {
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
                let cell = convert_cell(&cell_ref, &palette, default_fg, default_bg);
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
///
/// - `default_fg`/`default_bg`：用户配置的默认前景/背景色，
///   用于 `ColorAttribute::Default` 分支，避免硬编码黑白
fn convert_cell(
    cell_ref: &wezterm_term::CellRef,
    palette: &wezterm_term::color::ColorPalette,
    default_fg: Color,
    default_bg: Color,
) -> RustXtermCell {
    let text = cell_ref.str().to_string();
    let width = cell_ref.width();
    let attrs = cell_ref.attrs();
    let flags = CellFlags::from_attrs(attrs);

    let fg = resolve_color(attrs.foreground(), palette, true, default_fg, default_bg);
    let bg = resolve_color(attrs.background(), palette, false, default_fg, default_bg);

    // 尝试提取超链接
    let hyperlink = attrs.hyperlink().map(|h| h.uri().to_string());

    RustXtermCell {
        text,
        width,
        fg,
        bg,
        flags,
        hyperlink,
    }
}

/// 解析 WezTerm ColorAttribute 为 rust-xterm Color
///
/// - `default_fg`/`default_bg`：当 `color_attr == ColorAttribute::Default` 时，
///   根据是前景还是背景返回对应默认色（而非硬编码的黑白）
fn resolve_color(
    color_attr: wezterm_term::color::ColorAttribute,
    palette: &wezterm_term::color::ColorPalette,
    is_foreground: bool,
    default_fg: Color,
    default_bg: Color,
) -> Color {
    use wezterm_term::color::ColorAttribute;

    match color_attr {
        ColorAttribute::Default => {
            if is_foreground {
                default_fg
            } else {
                default_bg
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

/// 解析 CSI 参数字节中第一个分号前的无符号整数
///
/// 用于 DECSET/DECRST 的私有模式码解析，如 `1004` 来自 `1004` 或 `1004;...`。
/// 返回 `None` 表示无数字（默认值）。
fn parse_first_uint(params: &[u8]) -> Option<u32> {
    let end = params
        .iter()
        .position(|&b| b == b';')
        .unwrap_or(params.len());
    let s = &params[..end];
    if s.is_empty() {
        return None;
    }
    std::str::from_utf8(s)
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
}

/// 解析 DECSTBM（Set Top and Bottom Margins）参数为 1-based `(top, bottom)`
///
/// 参数形如 `<top>;<bottom>`，缺省时 `top = 1`，`bottom = rows`。
/// - 空参数 → `None`（重置为全屏）
/// - `top >= bottom` → `None`（无效，WezTerm 会忽略）
/// - 等价于全屏（`top == 1 && bottom == rows`）→ `None`
fn parse_decstbm(params: &[u8], rows: usize) -> Option<(usize, usize)> {
    // 空参数：重置为全屏
    if params.is_empty() {
        return None;
    }
    let mut parts = params.split(|&b| b == b';');
    let top = parts
        .next()
        .filter(|s| !s.is_empty())
        .and_then(|s| {
            std::str::from_utf8(s)
                .ok()
                .and_then(|t| t.parse::<usize>().ok())
        })
        .unwrap_or(1);
    let bottom = parts
        .next()
        .filter(|s| !s.is_empty())
        .and_then(|s| {
            std::str::from_utf8(s)
                .ok()
                .and_then(|t| t.parse::<usize>().ok())
        })
        .unwrap_or(rows);
    if top >= bottom || bottom > rows {
        return None;
    }
    // 等价于全屏则视为未设置
    if top == 1 && bottom == rows {
        return None;
    }
    Some((top, bottom))
}
