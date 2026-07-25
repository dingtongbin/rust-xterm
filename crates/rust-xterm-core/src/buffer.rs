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
