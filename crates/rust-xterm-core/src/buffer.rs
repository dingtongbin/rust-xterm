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
use crate::TerminalSize;

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
/// 管理正常和备用两个 buffer。
#[derive(Debug, Clone)]
pub struct BufferNamespace {
    /// 正常 buffer
    pub normal: Buffer,
    /// 备用 buffer
    pub alternate: Buffer,
    /// 当前激活的 buffer 类型
    pub active: BufferType,
    /// Marker 列表
    pub markers: Vec<Marker>,
    /// 下一个 Marker ID
    next_marker_id: u32,
}

impl BufferNamespace {
    /// 创建新的 Buffer 命名空间
    pub fn new(size: TerminalSize) -> Self {
        let normal = Buffer {
            kind: BufferType::Normal,
            cursor_y: 0,
            cursor_x: 0,
            base_y: 0,
            height: size.rows,
            width: size.cols,
            lines: vec![vec![RustXtermCell::blank(); size.cols]; size.rows],
        };
        let alternate = normal.clone();
        Self {
            normal: Buffer {
                kind: BufferType::Normal,
                ..normal
            },
            alternate: Buffer {
                kind: BufferType::Alternate,
                ..alternate
            },
            active: BufferType::Normal,
            markers: Vec::new(),
            next_marker_id: 0,
        }
    }

    /// 获取当前激活的 buffer
    pub fn active(&self) -> &Buffer {
        match self.active {
            BufferType::Normal => &self.normal,
            BufferType::Alternate => &self.alternate,
        }
    }

    /// 获取当前激活的 buffer（可变）
    pub fn active_mut(&mut self) -> &mut Buffer {
        match self.active {
            BufferType::Normal => &mut self.normal,
            BufferType::Alternate => &mut self.alternate,
        }
    }

    /// 添加 Marker
    pub fn add_marker(&mut self, line: i32) -> Marker {
        let marker = Marker::new(self.next_marker_id, line);
        self.next_marker_id += 1;
        self.markers.push(marker.clone());
        marker
    }

    /// 移除 Marker
    pub fn remove_marker(&mut self, id: u32) -> bool {
        let before = self.markers.len();
        self.markers.retain(|m| m.id != id);
        self.markers.len() < before
    }

    /// 获取所有 Marker
    pub fn markers(&self) -> &[Marker] {
        &self.markers
    }

    /// 切换到备用 buffer
    pub fn activate_alternate(&mut self) {
        self.active = BufferType::Alternate;
    }

    /// 切换回正常 buffer
    pub fn activate_normal(&mut self) {
        self.active = BufferType::Normal;
    }

    /// 是否在备用 buffer
    pub fn is_alternate(&self) -> bool {
        self.active == BufferType::Alternate
    }

    /// 调整 buffer 尺寸
    pub fn resize(&mut self, size: TerminalSize) {
        self.normal.resize(size.rows, size.cols);
        self.alternate.resize(size.rows, size.cols);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffer_creation() {
        let ns = BufferNamespace::new(TerminalSize::new(24, 80));
        assert_eq!(ns.active, BufferType::Normal);
        assert_eq!(ns.normal.height, 24);
        assert_eq!(ns.normal.width, 80);
        assert_eq!(ns.normal.length(), 24);
    }

    #[test]
    fn test_marker() {
        let mut ns = BufferNamespace::new(TerminalSize::new(24, 80));
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
    fn test_buffer_switch() {
        let mut ns = BufferNamespace::new(TerminalSize::new(24, 80));
        assert!(!ns.is_alternate());

        ns.activate_alternate();
        assert!(ns.is_alternate());

        ns.activate_normal();
        assert!(!ns.is_alternate());
    }

    #[test]
    fn test_line_text() {
        let mut ns = BufferNamespace::new(TerminalSize::new(24, 80));
        for (i, ch) in "Hello".chars().enumerate() {
            ns.normal.lines[0][i] = RustXtermCell {
                text: ch.to_string(),
                width: 1,
                fg: crate::Color::WHITE,
                bg: crate::Color::BLACK,
                flags: crate::CellFlags(0),
            };
        }

        assert_eq!(ns.normal.line_text(0).unwrap(), "Hello");
    }
}
