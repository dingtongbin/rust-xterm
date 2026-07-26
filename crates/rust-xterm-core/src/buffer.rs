//! Buffer 与 Marker 抽象（xterm.js 风格）
//!
//! 提供类似 xterm.js 的 `Buffer` / `Marker` API，
//! 允许宿主在 scrollback 中标记位置、查询行内容。
//!
//! ## Marker
//!
//! Marker 是 scrollback 中的一个位置标记，即使内容滚动，
//! Marker 也会跟踪到正确的位置（类似书签）。
//!
//! ## Buffer
//!
//! Buffer 是终端屏幕的抽象视图，提供行级访问。
//! xterm.js 有 normal 和 alternate 两个 buffer。

use crate::cell::RustXtermCell;
use crate::selection::SelectionRange;
use unicode_segmentation::UnicodeSegmentation;

/// Marker：scrollback 中的位置标记
///
/// 类似 xterm.js 的 `IMarker`。
/// 创建后即使内容滚动，`line` 属性会自动更新到正确位置。
#[derive(Debug, Clone)]
pub struct Marker {
    /// 唯一 ID
    pub id: u32,
    /// 标记的逻辑行号（基于 scrollback 顶部）
    pub line: i32,
}

impl Marker {
    /// 创建新的 Marker
    pub fn new(id: u32, line: i32) -> Self {
        Self { id, line }
    }
}

/// Buffer 类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferType {
    /// 正常缓冲区（含 scrollback）
    Normal,
    /// 备用缓冲区（全屏应用如 vim/less）
    Alternate,
}

/// Buffer 视图
///
/// 类似 xterm.js 的 `IBuffer`。
/// 提供对终端屏幕内容的行级访问。
#[derive(Debug, Clone)]
pub struct Buffer {
    /// Buffer 类型
    pub kind: BufferType,
    /// 光标所在行（相对于 viewport 顶部）
    pub cursor_y: usize,
    /// 光标所在列
    pub cursor_x: usize,
    /// viewport 顶部对应的逻辑行号
    pub base_y: usize,
    /// viewport 高度（行数）
    pub height: usize,
    /// viewport 宽度（列数）
    pub width: usize,
    /// 行数据
    pub lines: Vec<Vec<RustXtermCell>>,
}

impl Buffer {
    /// 获取指定行的 Cell 数据
    pub fn line(&self, y: usize) -> Option<&Vec<RustXtermCell>> {
        self.lines.get(y)
    }

    /// 获取指定位置的 Cell
    pub fn cell(&self, x: usize, y: usize) -> Option<&RustXtermCell> {
        self.lines.get(y)?.get(x)
    }

    /// 行数
    pub fn length(&self) -> usize {
        self.lines.len()
    }

    /// 获取某行的纯文本
    pub fn line_text(&self, y: usize) -> Option<String> {
        self.lines
            .get(y)
            .map(|line| line.iter().map(|c| c.text.as_str()).collect::<String>())
    }

    /// 获取整个 buffer 的纯文本
    pub fn dump(&self) -> String {
        let mut out = String::new();
        for (i, line) in self.lines.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            for cell in line {
                out.push_str(&cell.text);
            }
        }
        out
    }

    /// 调整 buffer 尺寸
    pub fn resize(&mut self, rows: usize, cols: usize) {
        self.height = rows;
        self.width = cols;
        self.lines.resize(rows, vec![RustXtermCell::blank(); cols]);
        for row in &mut self.lines {
            row.resize(cols, RustXtermCell::blank());
        }
    }
}

/// Buffer 命名空间
///
/// 类似 xterm.js 的 `IBufferNamespace`。
///
/// 历史设计曾持有 `normal`/`alternate`/`active` 三个影子字段，
/// 但 [`TerminalManager::buffer`] 总是从 WezTerm 实时重建快照，
/// 这些字段从未被读取，反而误导维护者。现已清理，仅保留
/// Marker 跟踪相关状态。
#[derive(Debug, Clone)]
pub struct BufferNamespace {
    /// Marker 列表（存储创建时的原始行号）
    pub markers: Vec<Marker>,
    /// 下一个 Marker ID
    next_marker_id: u32,
    /// 自上次 `add_marker` 以来 scrollback 增长的行数。
    /// Marker 的"有效行号" = `marker.line - scrollback_offset`，
    /// 小于 0 表示已被推出可视区。
    scrollback_offset: usize,
}

impl BufferNamespace {
    /// 创建新的 Buffer 命名空间
    ///
    /// 不再需要传入尺寸：Buffer 视图由 [`TerminalManager::buffer`]
    /// 从 WezTerm 实时重建，这里只跟踪 Marker。
    pub fn new() -> Self {
        Self {
            markers: Vec::new(),
            next_marker_id: 0,
            scrollback_offset: 0,
        }
    }

    /// 添加 Marker
    ///
    /// 新 Marker 基于当前 scrollback 位置创建，因此重置
    /// `scrollback_offset` 为 0。
    pub fn add_marker(&mut self, line: i32) -> Marker {
        let marker = Marker::new(self.next_marker_id, line);
        self.next_marker_id += 1;
        self.markers.push(marker.clone());
        // 新标记基于当前位置，重置累计偏移
        self.scrollback_offset = 0;
        marker
    }

    /// 移除 Marker
    pub fn remove_marker(&mut self, id: u32) -> bool {
        let before = self.markers.len();
        self.markers.retain(|m| m.id != id);
        self.markers.len() < before
    }

    /// 获取所有有效 Marker（带有效行号，过滤掉已推出可视区的）
    ///
    /// 返回的 Marker 的 `line` 字段是经过 scrollback 偏移修正后的
    /// "有效行号"，而非创建时的原始行号。
    pub fn markers(&self) -> Vec<Marker> {
        self.markers
            .iter()
            .filter_map(|m| {
                let effective = m.line - self.scrollback_offset as i32;
                if effective < 0 {
                    None
                } else {
                    Some(Marker::new(m.id, effective))
                }
            })
            .collect()
    }

    /// 获取当前 scrollback 偏移量（自上次 `add_marker` 以来增长的行数）
    pub fn scrollback_offset(&self) -> usize {
        self.scrollback_offset
    }

    /// 累加 scrollback 偏移量
    ///
    /// 由 [`TerminalManager::write`] 在检测到 scrollback 增长时调用。
    pub fn add_scrollback_offset(&mut self, delta: usize) {
        self.scrollback_offset = self.scrollback_offset.saturating_add(delta);
    }
}

impl Default for BufferNamespace {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// 选区文本提取（Task 5）
// ============================================================================

/// 提取选区文本
///
/// 从屏幕行数据 `rows` 中按 [`SelectionRange`] 提取文本。
///
/// - **线性选区**（`rectangular = false`）：按起点终点排序后跨行连接，
///   首行从起点列到行尾，中间行整行，末行从行首到终点列，行间以 `\n` 分隔。
/// - **矩形选区**（`rectangular = true`）：每行独立按列范围 `[min, max]` 截取，
///   行间以 `\n` 分隔。
///
/// 坐标为 `(row, col)` 0-based，越界坐标自动 clamp。
pub fn selection_text(range: SelectionRange, rows: &[Vec<RustXtermCell>]) -> String {
    let (sr, sc) = range.start;
    let (er, ec) = range.end;

    if range.rectangular {
        let (r1, r2) = (sr.min(er), sr.max(er));
        let (c1, c2) = (sc.min(ec), sc.max(ec));
        let mut out = String::new();
        for r in r1..=r2 {
            if r > r1 {
                out.push('\n');
            }
            let row = match rows.get(r) {
                Some(row) => row,
                None => continue,
            };
            let row_len = row.len();
            if row_len == 0 {
                continue;
            }
            let cs = c1.min(row_len);
            let ce = c2.min(row_len.saturating_sub(1));
            // Grapheme 聚簇对齐：避免拆分 ZWJ 序列 / 国旗对
            let (cs, ce) = align_span_to_grapheme(row, cs, ce);
            for c in cs..=ce {
                if let Some(cell) = row.get(c) {
                    out.push_str(&cell.text);
                }
            }
        }
        out
    } else {
        // 线性选区：先按 (row, col) 字典序排序起点终点
        let ((sr, sc), (er, ec)) = if (sr, sc) <= (er, ec) {
            ((sr, sc), (er, ec))
        } else {
            ((er, ec), (sr, sc))
        };
        let mut out = String::new();
        for r in sr..=er {
            if r > sr {
                out.push('\n');
            }
            let row = match rows.get(r) {
                Some(row) => row,
                None => continue,
            };
            let row_len = row.len();
            if row_len == 0 {
                continue;
            }
            let col_start = if r == sr { sc.min(row_len) } else { 0 };
            let col_end = if r == er {
                ec.min(row_len.saturating_sub(1))
            } else {
                row_len.saturating_sub(1)
            };
            // Grapheme 聚簇对齐：向左扩展到 cluster 起点，向右扩展到 cluster 终点
            let (col_start, col_end) = align_span_to_grapheme(row, col_start, col_end);
            for c in col_start..=col_end {
                if let Some(cell) = row.get(c) {
                    out.push_str(&cell.text);
                }
            }
        }
        out
    }
}

// ============================================================================
// 智能选区扩展（Task 6 / Task 3 智能选词 / Task 7 grapheme 聚簇）
// ============================================================================

/// 字符类别（用于智能选词边界判定）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CharClass {
    /// 空白
    Whitespace,
    /// 字母数字
    Alphanumeric,
    /// 标点
    Punctuation,
}

/// 判定一个 Cell 的字符类别
///
/// 取 Cell 文本的第一个字符归类；空文本（空白 Cell）归为 `Whitespace`。
/// 三类划分遵循 `is_whitespace` / `is_alphanumeric` / 其余归为标点。
fn cell_class(cell: &RustXtermCell) -> CharClass {
    match cell.text.chars().next() {
        None => CharClass::Whitespace,
        Some(c) if c.is_whitespace() => CharClass::Whitespace,
        Some(c) if c.is_alphanumeric() => CharClass::Alphanumeric,
        Some(_) => CharClass::Punctuation,
    }
}

/// 判定 Cell 是否为空白（空文本或首字符为空白）
fn is_whitespace_cell(cell: &RustXtermCell) -> bool {
    match cell.text.chars().next() {
        None => true,
        Some(c) => c.is_whitespace(),
    }
}

// ----------------------------------------------------------------------------
// SubTask 3.2: token 边界检测
// ----------------------------------------------------------------------------

/// 检测 token 边界：从 `pos` 向左右扩展直到遇到空白 Cell
///
/// token = 连续非空白字符序列。返回同一行内的闭区间 `(left, right)`，
/// 若点击位置本身为空白或越界则返回 `None`。
///
/// 坐标 `pos = (row, col)` 0-based。
pub fn detect_token_bounds(
    pos: (usize, usize),
    rows: &[Vec<RustXtermCell>],
) -> Option<(usize, usize)> {
    let (row, col) = pos;
    let row_cells = rows.get(row)?;
    if col >= row_cells.len() || is_whitespace_cell(&row_cells[col]) {
        return None;
    }
    let row_len = row_cells.len();

    // 向左扩展
    let mut left = col;
    while left > 0 && !is_whitespace_cell(&row_cells[left - 1]) {
        left -= 1;
    }
    // 向右扩展
    let mut right = col;
    while right + 1 < row_len && !is_whitespace_cell(&row_cells[right + 1]) {
        right += 1;
    }
    Some((left, right))
}

// ----------------------------------------------------------------------------
// SubTask 3.1 / 3.4: 智能选词模式检测（纯手写启发式，不引入 regex/url）
// ----------------------------------------------------------------------------

/// 检测 token 文本是否匹配智能选词模式（URL / Unix 路径 / IPv4 / IPv6）
fn matches_smart_pattern(text: &str) -> bool {
    is_url(text) || is_unix_path(text) || is_ipv4(text) || is_ipv6(text)
}

/// URL scheme 检测：`http://` / `https://` / `ftp://` / `file://` / `ssh://`（大小写不敏感）
fn is_url(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    ["http://", "https://", "ftp://", "file://", "ssh://"]
        .iter()
        .any(|s| lower.starts_with(s))
}

/// Unix 路径检测：以 `/` 开头，仅含 `[A-Za-z0-9/_.-]`
fn is_unix_path(text: &str) -> bool {
    if !text.starts_with('/') {
        return false;
    }
    text.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '.' | '-'))
}

/// IPv4 检测：4 段数字用 `.` 分隔，每段 0-255
fn is_ipv4(text: &str) -> bool {
    let parts: Vec<&str> = text.split('.').collect();
    if parts.len() != 4 {
        return false;
    }
    for part in parts {
        if part.is_empty() || part.len() > 3 {
            return false;
        }
        if !part.chars().all(|c| c.is_ascii_digit()) {
            return false;
        }
        let n: u32 = match part.parse() {
            Ok(n) => n,
            Err(_) => return false,
        };
        if n > 255 {
            return false;
        }
    }
    true
}

/// IPv6 检测：含 `:` 且仅含 `[0-9a-fA-F:.]`
fn is_ipv6(text: &str) -> bool {
    if !text.contains(':') {
        return false;
    }
    text.chars()
        .all(|c| c.is_ascii_hexdigit() || matches!(c, ':' | '.'))
}

// ----------------------------------------------------------------------------
// SubTask 7.2: grapheme 聚簇对齐
// ----------------------------------------------------------------------------

/// 为一行构建 cell → grapheme cluster 范围的映射
///
/// 返回 `Vec<(start_cell, end_cell)>`，长度等于 `row.len()`。
/// 每个 cell 映射到它所属 grapheme cluster 的 cell 范围（闭区间）。
/// 空 cell（width=0 占位）映射到自身 `(i, i)`。
///
/// 用于确保选区列边界不拆分 ZWJ 序列 / 国旗对 / 肤色修饰符等。
fn grapheme_cluster_spans(row: &[RustXtermCell]) -> Vec<(usize, usize)> {
    let n = row.len();
    if n == 0 {
        return Vec::new();
    }

    // 拼接整行文本，记录每个 cell 的起始 char offset
    let mut full_text = String::new();
    let mut cell_offsets = Vec::with_capacity(n + 1);
    cell_offsets.push(0usize);
    for cell in row.iter() {
        full_text.push_str(&cell.text);
        cell_offsets.push(full_text.chars().count());
    }

    // 遍历 grapheme cluster，记录每个 cluster 的 char 范围 (start, end)
    let mut clusters: Vec<(usize, usize)> = Vec::new();
    let mut pos = 0;
    for g in full_text.graphemes(true) {
        let len = g.chars().count();
        clusters.push((pos, pos + len));
        pos += len;
    }

    // 为每个 cell 找到它所属的 cluster，并映射回 cell 范围
    let mut result = vec![(0usize, 0usize); n];
    let mut ci = 0usize; // 当前 cluster 索引（单调推进）
    for i in 0..n {
        let cell_start = cell_offsets[i];
        let cell_end = cell_offsets[i + 1];
        if cell_start == cell_end {
            // 空 cell（不贡献字符）
            result[i] = (i, i);
            continue;
        }
        // 推进 ci 到包含 cell_start 的 cluster
        while ci < clusters.len() && clusters[ci].1 <= cell_start {
            ci += 1;
        }
        if ci >= clusters.len() {
            result[i] = (i, i);
            continue;
        }
        let (cs, ce) = clusters[ci];
        // cluster 的 cell 范围：起点 = 最大 j 使 cell_offsets[j] <= cs
        let start_cell = (0..n).rev().find(|&j| cell_offsets[j] <= cs).unwrap_or(0);
        // 终点 = 最大 j 使 cell_offsets[j] < ce
        let end_cell = (0..n)
            .rev()
            .find(|&j| cell_offsets[j] < ce)
            .unwrap_or(start_cell);
        result[i] = (start_cell, end_cell);
    }

    result
}

/// 将列边界 `[left, right]` 对齐到 grapheme cluster 边界
///
/// - `left` 向左扩展到所属 cluster 的起点
/// - `right` 向右扩展到所属 cluster 的终点
fn align_span_to_grapheme(row: &[RustXtermCell], left: usize, right: usize) -> (usize, usize) {
    if row.is_empty() {
        return (left, right);
    }
    let spans = grapheme_cluster_spans(row);
    let aligned_left = spans.get(left).map_or(left, |(s, _)| *s).min(left);
    let aligned_right = spans.get(right).map_or(right, |(_, e)| *e).max(right);
    (aligned_left, aligned_right)
}

// ----------------------------------------------------------------------------
// 智能选词：URL / 路径 / IP 优先，回退到字符类别边界
// ----------------------------------------------------------------------------

/// 智能选词：从 `pos` 向左右扩展
///
/// 坐标 `pos = (row, col)` 0-based。返回同一行内的线性选区。
///
/// 1. 先检测点击位置所在 token 是否匹配 URL / Unix 路径 / IPv4 / IPv6，
///    匹配则选区覆盖整个 token（并按 grapheme cluster 边界对齐）。
/// 2. 否则回退到按字符类别（空白 / 字母数字 / 标点）边界扩展。
pub fn select_word(pos: (usize, usize), rows: &[Vec<RustXtermCell>]) -> SelectionRange {
    let (row, col) = pos;
    let row_cells = match rows.get(row) {
        Some(r) => r,
        None => return SelectionRange::linear(pos, pos),
    };

    // 1. 智能选词：检测 URL / 路径 / IP
    if let Some((left, right)) = detect_token_bounds(pos, rows) {
        let token_text: String = row_cells[left..=right]
            .iter()
            .map(|c| c.text.as_str())
            .collect();
        if matches_smart_pattern(&token_text) {
            let (al, ar) = align_span_to_grapheme(row_cells, left, right);
            return SelectionRange {
                start: (row, al),
                end: (row, ar),
                rectangular: false,
            };
        }
    }

    // 2. 回退：按字符类别边界扩展
    let class = row_cells.get(col).map_or(CharClass::Whitespace, cell_class);
    let row_len = row_cells.len();

    // 向左扩展
    let mut left = col;
    while left > 0 {
        let same = row_cells
            .get(left - 1)
            .is_some_and(|c| cell_class(c) == class);
        if same {
            left -= 1;
        } else {
            break;
        }
    }
    // 向右扩展
    let mut right = col;
    while right + 1 < row_len {
        let same = row_cells
            .get(right + 1)
            .is_some_and(|c| cell_class(c) == class);
        if same {
            right += 1;
        } else {
            break;
        }
    }

    // Grapheme 聚簇对齐
    let (al, ar) = align_span_to_grapheme(row_cells, left, right);
    SelectionRange {
        start: (row, al),
        end: (row, ar),
        rectangular: false,
    }
}

/// 选整行：从第 0 列到最后一列
///
/// 坐标 `pos = (row, col)` 0-based，仅 `row` 生效。返回覆盖整行的线性选区。
pub fn select_line(pos: (usize, usize), rows: &[Vec<RustXtermCell>]) -> SelectionRange {
    let (row, _col) = pos;
    let cols = rows.get(row).map_or(0, Vec::len);
    let last = cols.saturating_sub(1);
    SelectionRange {
        start: (row, 0),
        end: (row, last),
        rectangular: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffer_namespace_creation() {
        let ns = BufferNamespace::new();
        assert!(ns.markers.is_empty());
        assert_eq!(ns.scrollback_offset(), 0);
    }

    #[test]
    fn test_marker() {
        let mut ns = BufferNamespace::new();
        let m1 = ns.add_marker(10);
        let m2 = ns.add_marker(20);

        assert_eq!(m1.id, 0);
        assert_eq!(m1.line, 10);
        assert_eq!(m2.id, 1);
        assert_eq!(m2.line, 20);
        assert_eq!(ns.markers().len(), 2);

        assert!(ns.remove_marker(0));
        assert_eq!(ns.markers().len(), 1);
    }

    #[test]
    fn test_marker_effective_line_with_scrollback() {
        // 验证 Task 4：scrollback 增长时 marker 有效行号随之调整
        let mut ns = BufferNamespace::new();
        let _ = ns.add_marker(23);
        // 模拟 scrollback 增长 1 行
        ns.add_scrollback_offset(1);
        let markers = ns.markers();
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].line, 22);
    }

    #[test]
    fn test_marker_filtered_when_out_of_view() {
        // 验证：有效行号 < 0 的 marker 被过滤掉
        let mut ns = BufferNamespace::new();
        let _ = ns.add_marker(2);
        // scrollback 增长 3 行，使有效行号 = 2 - 3 = -1（应被过滤）
        ns.add_scrollback_offset(3);
        assert!(ns.markers().is_empty());
    }

    #[test]
    fn test_add_marker_resets_offset() {
        // 验证：新增 marker 时重置 scrollback_offset
        let mut ns = BufferNamespace::new();
        let _ = ns.add_marker(5);
        ns.add_scrollback_offset(10);
        assert_eq!(ns.scrollback_offset(), 10);
        // 新 marker 重置偏移
        let _ = ns.add_marker(8);
        assert_eq!(ns.scrollback_offset(), 0);
    }

    #[test]
    fn test_buffer_line_text() {
        // 直接构造 Buffer 验证 line_text（不依赖 BufferNamespace 的影子字段）
        let mut buf = Buffer {
            kind: BufferType::Normal,
            cursor_y: 0,
            cursor_x: 0,
            base_y: 0,
            height: 24,
            width: 80,
            lines: vec![vec![RustXtermCell::blank(); 80]; 24],
        };
        for (i, ch) in "Hello".chars().enumerate() {
            buf.lines[0][i] = RustXtermCell {
                text: ch.to_string(),
                width: 1,
                fg: crate::Color::WHITE,
                bg: crate::Color::BLACK,
                flags: crate::CellFlags(0),
                hyperlink: None,
            };
        }
        assert_eq!(buf.line_text(0).unwrap(), "Hello");
    }

    // ========================================================================
    // Task 3: 智能选词规则（URL / 路径 / IP）
    // ========================================================================

    /// 测试辅助：从字符串构造一行 Cell，每个字符一个 Cell
    fn make_row(text: &str) -> Vec<RustXtermCell> {
        text.chars()
            .map(|ch| RustXtermCell {
                text: ch.to_string(),
                width: 1,
                fg: crate::Color::WHITE,
                bg: crate::Color::BLACK,
                flags: crate::CellFlags(0),
                hyperlink: None,
            })
            .collect()
    }

    /// 测试辅助：从字符列表构造一行 Cell
    fn make_row_chars(chs: &[char]) -> Vec<RustXtermCell> {
        chs.iter()
            .map(|&ch| RustXtermCell {
                text: ch.to_string(),
                width: 1,
                fg: crate::Color::WHITE,
                bg: crate::Color::BLACK,
                flags: crate::CellFlags(0),
                hyperlink: None,
            })
            .collect()
    }

    #[test]
    fn test_select_word_url() {
        // "see https://ex.com/p now"
        //  0123456789012345678901234
        // 双击 "ex"（col 12）断言选区为 "https://ex.com/p"
        let row = make_row("see https://ex.com/p now");
        let rows = vec![row];
        let range = select_word((0, 12), &rows);
        assert_eq!(range.start, (0, 4), "URL 选区起点应在 'h'");
        assert_eq!(range.end, (0, 19), "URL 选区终点应在 'p'");
        assert_eq!(selection_text(range, &rows), "https://ex.com/p");
        // 在 URL 中间双击也应选整个 URL
        let range2 = select_word((0, 14), &rows); // '.' of ".com"
        assert_eq!(selection_text(range2, &rows), "https://ex.com/p");
    }

    #[test]
    fn test_select_word_unix_path() {
        let row = make_row("/usr/local/bin");
        let rows = vec![row];
        // 双击中间任意位置都应选整条路径
        let range = select_word((0, 5), &rows); // 'l' of "local"
        assert_eq!(range.start, (0, 0));
        assert_eq!(range.end, (0, 13));
        assert_eq!(selection_text(range, &rows), "/usr/local/bin");
    }

    #[test]
    fn test_select_word_ipv4() {
        let row = make_row("192.168.1.1");
        let rows = vec![row];
        let range = select_word((0, 0), &rows);
        assert_eq!(range.start, (0, 0));
        assert_eq!(range.end, (0, 10));
        assert_eq!(selection_text(range, &rows), "192.168.1.1");
        // 中间双击也应全选
        let range2 = select_word((0, 5), &rows); // '6' of "168"
        assert_eq!(selection_text(range2, &rows), "192.168.1.1");
    }

    #[test]
    fn test_select_word_ipv6() {
        let row = make_row("fe80::1");
        let rows = vec![row];
        let range = select_word((0, 2), &rows);
        assert_eq!(selection_text(range, &rows), "fe80::1");
    }

    #[test]
    fn test_select_word_plain() {
        // 普通单词：不匹配 URL/路径/IP，回退到字符类别边界
        let row = make_row("hello world");
        let rows = vec![row];
        let range = select_word((0, 0), &rows); // 'h' of "hello"
        assert_eq!(range.start, (0, 0));
        assert_eq!(range.end, (0, 4));
        assert_eq!(selection_text(range, &rows), "hello");
        // 标点回退：双击 "foo-bar" 的 '-' 应只选 '-'
        let row2 = make_row("foo-bar");
        let rows2 = vec![row2];
        let range2 = select_word((0, 3), &rows2); // '-'
        assert_eq!(selection_text(range2, &rows2), "-");
    }

    #[test]
    fn test_detect_token_bounds() {
        let row = make_row("see https://ex.com/p now");
        let rows = vec![row];
        // 点击 'e' of "ex"（col 12）→ token = "https://ex.com/p"（col 4..19）
        assert_eq!(detect_token_bounds((0, 12), &rows), Some((4, 19)));
        // 点击空白（col 3）→ None
        assert_eq!(detect_token_bounds((0, 3), &rows), None);
        // 点击 'n' of "now"（col 21）→ token = "now"（col 21..23）
        assert_eq!(detect_token_bounds((0, 21), &rows), Some((21, 23)));
        // 越界行 → None
        assert_eq!(detect_token_bounds((5, 0), &rows), None);
        // 越界列 → None
        assert_eq!(detect_token_bounds((0, 100), &rows), None);
    }

    #[test]
    fn test_smart_pattern_detection() {
        // URL
        assert!(is_url("https://example.com"));
        assert!(is_url("HTTP://EXAMPLE.COM")); // 大小写不敏感
        assert!(is_url("ftp://host"));
        assert!(is_url("file:///a/b"));
        assert!(is_url("ssh://user@host"));
        assert!(!is_url("example.com"));
        // Unix 路径
        assert!(is_unix_path("/usr/local/bin"));
        assert!(is_unix_path("/a/b_c.txt"));
        assert!(!is_unix_path("relative/path"));
        assert!(!is_unix_path("/has space"));
        // IPv4
        assert!(is_ipv4("192.168.1.1"));
        assert!(is_ipv4("0.0.0.0"));
        assert!(is_ipv4("255.255.255.255"));
        assert!(!is_ipv4("256.1.1.1"));
        assert!(!is_ipv4("1.2.3"));
        assert!(!is_ipv4("1.2.3.4.5"));
        assert!(!is_ipv4("a.b.c.d"));
        // IPv6
        assert!(is_ipv6("fe80::1"));
        assert!(is_ipv6("2001:db8::1"));
        assert!(is_ipv6("::1"));
        assert!(!is_ipv6("192.168.1.1"));
        assert!(!is_ipv6("no-colon"));
    }

    // ========================================================================
    // Task 7: Unicode grapheme 聚簇
    // ========================================================================

    #[test]
    fn test_grapheme_zwj_selection() {
        // 👨‍👩‍👧 = U+1F468 U+200D U+1F469 U+200D U+1F467（family ZWJ，单一 grapheme cluster）
        // 每个 code point 一个 cell，验证双击选中整个序列
        let row = make_row_chars(&[
            '\u{1F468}', // 👨
            '\u{200D}',  // ZWJ
            '\u{1F469}', // 👩
            '\u{200D}',  // ZWJ
            '\u{1F467}', // 👧
        ]);
        let rows = vec![row];
        // 双击 cell 2（👩）：应选覆盖整个 ZWJ 序列
        let range = select_word((0, 2), &rows);
        assert_eq!(range.start, (0, 0));
        assert_eq!(range.end, (0, 4));
        let expected = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";
        assert_eq!(selection_text(range, &rows), expected);
        // 双击 cell 0（👨）也应选整个序列
        let range0 = select_word((0, 0), &rows);
        assert_eq!(selection_text(range0, &rows), expected);
    }

    #[test]
    fn test_grapheme_flag_selection() {
        // 🇨🇳 = U+1F1E8 U+1F1F3（regional indicator pair，单一 grapheme cluster）
        let row = make_row_chars(&['\u{1F1E8}', '\u{1F1F3}']);
        let rows = vec![row];
        // 双击 cell 0 应覆盖两个 regional indicator
        let range = select_word((0, 0), &rows);
        assert_eq!(range.start, (0, 0));
        assert_eq!(range.end, (0, 1));
        assert_eq!(selection_text(range, &rows), "\u{1F1E8}\u{1F1F3}");
        // 双击 cell 1 也应选整个国旗
        let range1 = select_word((0, 1), &rows);
        assert_eq!(selection_text(range1, &rows), "\u{1F1E8}\u{1F1F3}");
    }

    #[test]
    fn test_grapheme_cluster_spans_mapping() {
        // 验证 grapheme_cluster_spans 的 cell 映射逻辑
        // "a👨‍👩b"：a(0) 👨(1) ZWJ(2) 👩(3) b(4)
        // grapheme clusters: "a" → cell 0；"👨‍👩" → cells 1..3；"b" → cell 4
        let row = make_row_chars(&['a', '\u{1F468}', '\u{200D}', '\u{1F469}', 'b']);
        let spans = grapheme_cluster_spans(&row);
        assert_eq!(spans.len(), 5);
        assert_eq!(spans[0], (0, 0)); // 'a' 独立 cluster
        assert_eq!(spans[1], (1, 3)); // 👨‍👩 跨 cell 1..3
        assert_eq!(spans[2], (1, 3)); // ZWJ 属同一 cluster
        assert_eq!(spans[3], (1, 3)); // 👩 属同一 cluster
        assert_eq!(spans[4], (4, 4)); // 'b' 独立 cluster
    }

    #[test]
    fn test_grapheme_selection_text_alignment() {
        // 验证 selection_text 在列边界落在 grapheme 中间时对齐到 cluster 边界
        // "x👨‍👩y"：x(0) 👨(1) ZWJ(2) 👩(3) y(4)
        let row = make_row_chars(&['x', '\u{1F468}', '\u{200D}', '\u{1F469}', 'y']);
        let rows = vec![row];
        // 选区 (0, 0)-(0, 2)：col 2 是 ZWJ，处于 "👨‍👩" cluster 中间
        // 应对齐到 (0, 0)-(0, 3)，文本 = "x👨‍👩"
        let range = SelectionRange::linear((0, 0), (0, 2));
        let text = selection_text(range, &rows);
        assert_eq!(text, "x\u{1F468}\u{200D}\u{1F469}");
    }
}
