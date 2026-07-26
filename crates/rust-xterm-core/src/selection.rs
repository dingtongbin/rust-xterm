//! 选区系统模型（xterm.js 风格）
//!
//! 提供与 xterm.js 选区 API 对齐的几何模型 [`SelectionRange`]，
//! 用于描述终端屏幕上的文本选区。
//!
//! ## 坐标约定
//!
//! - 坐标为 `(row, col)`，0-based
//! - `row` 为当前可视窗口的行索引（不含 scrollback 偏移）
//! - `col` 为列索引
//!
//! ## 选区类型
//!
//! - **线性选区**（`rectangular = false`）：从 `start` 到 `end` 的连续文本流，
//!   跨行时按阅读顺序连接。`start` / `end` 的相对顺序任意，提取文本时会自动排序。
//! - **矩形选区**（`rectangular = true`）：以 `start` / `end` 为对角的矩形区域，
//!   每行独立按列范围截取。

/// 选区范围
///
/// 描述终端屏幕上的一个文本选区，坐标为 `(row, col)` 0-based。
/// 详见 [模块文档](crate::selection)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectionRange {
    /// 选区起点 `(row, col)`
    pub start: (usize, usize),
    /// 选区终点 `(row, col)`
    pub end: (usize, usize),
    /// 是否为矩形选区
    pub rectangular: bool,
}

impl SelectionRange {
    /// 创建线性选区
    pub fn linear(start: (usize, usize), end: (usize, usize)) -> Self {
        Self {
            start,
            end,
            rectangular: false,
        }
    }

    /// 创建矩形选区
    pub fn rectangular(start: (usize, usize), end: (usize, usize)) -> Self {
        Self {
            start,
            end,
            rectangular: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_selection_range_constructors() {
        let lin = SelectionRange::linear((0, 0), (1, 5));
        assert!(!lin.rectangular);
        assert_eq!(lin.start, (0, 0));
        assert_eq!(lin.end, (1, 5));

        let rect = SelectionRange::rectangular((0, 0), (2, 3));
        assert!(rect.rectangular);
    }
}
