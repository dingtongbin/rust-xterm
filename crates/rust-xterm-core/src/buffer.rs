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
            let row_len = rows.get(r).map_or(0, Vec::len);
            if row_len == 0 {
                continue;
            }
            let cs = c1.min(row_len);
            let ce = c2.min(row_len.saturating_sub(1));
            if let Some(row) = rows.get(r) {
                for c in cs..=ce {
                    if let Some(cell) = row.get(c) {
                        out.push_str(&cell.text);
                    }
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
            let row_len = rows.get(r).map_or(0, Vec::len);
            if row_len == 0 {
                continue;
            }
            let col_start = if r == sr { sc.min(row_len) } else { 0 };
            let col_end = if r == er {
                ec.min(row_len.saturating_sub(1))
            } else {
                row_len.saturating_sub(1)
            };
            if let Some(row) = rows.get(r) {
                for c in col_start..=col_end {
                    if let Some(cell) = row.get(c) {
                        out.push_str(&cell.text);
                    }
                }
            }
        }
        out
    }
}

// ============================================================================
// 智能选区扩展（Task 6）
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

/// 智能选词：从 `pos` 向左右扩展，直到字符类别边界
///
/// 坐标 `pos = (row, col)` 0-based。返回同一行内的线性选区。
/// 点击位置 Cell 的类别决定扩展方向上的同类边界。
pub fn select_word(pos: (usize, usize), rows: &[Vec<RustXtermCell>]) -> SelectionRange {
    let (row, col) = pos;
    let row_cells = rows.get(row);
    let class = row_cells
        .and_then(|r| r.get(col))
        .map_or(CharClass::Whitespace, cell_class);
    let row_len = row_cells.map_or(0, Vec::len);

    // 向左扩展
    let mut left = col;
    while left > 0 {
        let same = row_cells
            .and_then(|r| r.get(left - 1))
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
            .and_then(|r| r.get(right + 1))
            .is_some_and(|c| cell_class(c) == class);
        if same {
            right += 1;
        } else {
            break;
        }
    }
    SelectionRange {
        start: (row, left),
        end: (row, right),
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
}
