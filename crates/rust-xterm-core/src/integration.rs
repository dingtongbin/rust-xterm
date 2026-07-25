//! GUI 集成抽象
//!
//! 提供 trait 化的渲染面抽象，使 rust-xterm 可以轻松集成进
//! 任意 GUI 框架（Slint / winit / egui / Tauri / 自研引擎）。
//!
//! ## 集成步骤
//!
//! 1. 实现 [`RenderSurface`] trait，桥接 rust-xterm 的帧输出到你的 GUI 后端
//! 2. 实现 [`InputSource`] trait，桥接用户输入到 rust-xterm
//! 3. 在 GUI 事件循环中调用 [`TerminalManager::poll_frame`] 并推送到 `RenderSurface`
//!
//! ## 设计原则
//!
//! - **零依赖**：trait 定义不引入任何 GUI 依赖
//! - **零拷贝**：传递 `&[u8]` 切片，避免不必要的拷贝
//! - **异步友好**：所有方法都是同步的，可适配同步/异步事件循环

use crate::{CursorMeta, RustXtermCell};

/// 渲染面抽象
///
/// 宿主实现此 trait，将 rust-xterm 的帧输出桥接到 GUI 后端。
pub trait RenderSurface {
    /// 更新指定行的 Cell 数据
    ///
    /// - `y`: 行索引
    /// - `cells`: 该行的 Cell 数据
    fn update_row(&mut self, y: usize, cells: &[RustXtermCell]);

    /// 更新多个脏区
    fn update_dirty_rows(&mut self, dirty: &[(usize, &[RustXtermCell])]) {
        for (y, cells) in dirty {
            self.update_row(*y, cells);
        }
    }

    /// 更新光标位置和样式
    fn update_cursor(&mut self, cursor: CursorMeta);

    /// 更新终端尺寸（像素）
    ///
    /// - `width`: 画布宽度（像素）
    /// - `height`: 画布高度（像素）
    fn resize(&mut self, width: u32, height: u32);

    /// 提交本次帧更新（双缓冲场景下交换缓冲区）
    fn present(&mut self);

    /// 获取当前渲染度量（单元格宽高、基线等）
    fn metrics(&self) -> RenderMetrics;
}

/// 渲染度量
#[derive(Debug, Clone, Copy, Default)]
pub struct RenderMetrics {
    /// 单元格宽度（像素）
    pub cell_width: u32,
    /// 单元格高度（像素）
    pub cell_height: u32,
    /// 基线 Y 坐标
    pub baseline: u32,
    /// 画布宽度（像素）
    pub canvas_width: u32,
    /// 画布高度（像素）
    pub canvas_height: u32,
}

/// 输入源抽象
///
/// 宿主实现此 trait，将用户输入桥接到 rust-xterm。
pub trait InputSource {
    /// 读取用户输入字节流
    ///
    /// 返回读取的字节数。若无输入返回 0。
    fn poll_input(&mut self, buf: &mut [u8]) -> usize;

    /// 是否有输入待读取
    fn has_input(&self) -> bool;
}

/// 终端尺寸提供者
///
/// 宿主实现此 trait，将 GUI 窗口尺寸变化通知 rust-xterm。
pub trait SizeSource {
    /// 获取当前窗口尺寸（像素）
    fn pixel_size(&self) -> (u32, u32);

    /// 根据 DPI 和字体度量计算终端行列数
    fn cell_size(&self) -> (u32, u32);
}

/// Null 渲染面（用于测试和 headless 场景）
pub struct NullRenderSurface {
    metrics: RenderMetrics,
    row_count: usize,
    cursor: Option<CursorMeta>,
}

impl NullRenderSurface {
    /// 创建 Null 渲染面
    pub fn new() -> Self {
        Self {
            metrics: RenderMetrics {
                cell_width: 8,
                cell_height: 16,
                baseline: 13,
                canvas_width: 640,
                canvas_height: 384,
            },
            row_count: 0,
            cursor: None,
        }
    }

    /// 获取已更新的行数
    pub fn updated_rows(&self) -> usize {
        self.row_count
    }

    /// 获取最后的光标状态
    pub fn last_cursor(&self) -> Option<CursorMeta> {
        self.cursor
    }
}

impl Default for NullRenderSurface {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderSurface for NullRenderSurface {
    fn update_row(&mut self, _y: usize, _cells: &[RustXtermCell]) {
        self.row_count += 1;
    }

    fn update_cursor(&mut self, cursor: CursorMeta) {
        self.cursor = Some(cursor);
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.metrics.canvas_width = width;
        self.metrics.canvas_height = height;
    }

    fn present(&mut self) {}

    fn metrics(&self) -> RenderMetrics {
        self.metrics
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_null_render_surface() {
        let mut surface = NullRenderSurface::new();
        let cells = vec![RustXtermCell::blank(); 80];
        surface.update_row(0, &cells);
        surface.update_row(1, &cells);

        assert_eq!(surface.updated_rows(), 2);

        let cursor = CursorMeta {
            x: 5,
            y: 3,
            visible: true,
            shape: crate::CursorShape::Block,
        };
        surface.update_cursor(cursor);
        assert!(surface.last_cursor().is_some());
    }

    #[test]
    fn test_update_dirty_rows() {
        let mut surface = NullRenderSurface::new();
        let cells = vec![RustXtermCell::blank(); 80];
        let dirty: Vec<(usize, &[RustXtermCell])> = vec![(0, &cells), (1, &cells), (2, &cells)];
        surface.update_dirty_rows(&dirty);
        assert_eq!(surface.updated_rows(), 3);
    }

    #[test]
    fn test_render_metrics() {
        let surface = NullRenderSurface::new();
        let m = surface.metrics();
        assert_eq!(m.cell_width, 8);
        assert_eq!(m.cell_height, 16);
    }
}
