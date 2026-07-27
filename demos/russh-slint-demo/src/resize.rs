//! 实时 resize 同步：在 render::tick 开头检测窗口尺寸变化时立即执行。
//!
//! 与 slint-demo 差异：resize 不仅同步本地 manager/renderer，
//! 还通过 `SshBridge::send_resize(cols, rows)` 通知远端 PTY 窗口大小。
//!
//! 原先由独立的 200ms resize_timer 轮询驱动，现改为在 render::tick 每 16ms
//! 检测窗口尺寸变化，达到实时同步（无 200ms 延迟）。
use crate::app_ctx::AppCtx;
use crate::{CELL_H, CELL_W, STATUS_BAR_H};
use rust_xterm_core::DirtySpan;
use slint::ComponentHandle;

/// 立即执行 resize 全流程。
///
/// 调用前提：调用方已确认窗口尺寸与 `ctx.last_window_size` 不同（由 render::tick 检测）。
/// 本函数完成：
/// 1. 根据 window 物理像素 + scale 计算 cols/rows
/// 2. `renderer.set_scale_factor` + `manager.resize` + `renderer.resize` + `renderer.clear`
/// 3. 全屏重绘（render_frame + render_cursor）
/// 4. 通知远端 SSH channel 窗口大小变更
pub(crate) fn handle_resize_now(ctx: &mut AppCtx, app: &crate::App, scale: f32) {
    // Slint 1.6 的 `Window::size()` 返回 `PhysicalSize`（物理像素），
    // 直接作物理像素使用，切勿再乘 scale（否则 HiDPI 下 canvas 双倍放大）。
    let window_size = app.window().size();
    let phys_w_total = window_size.width;
    let phys_h_total = window_size.height;
    // STATUS_BAR_H 是 Slint 中的 logical px，需乘 scale 转物理像素。
    let status_bar_h_phys = (STATUS_BAR_H as f32 * scale) as u32;
    if phys_w_total == 0 || phys_h_total <= status_bar_h_phys {
        return;
    }
    let avail_phys_w = phys_w_total;
    let avail_phys_h = phys_h_total - status_bar_h_phys;
    // renderer 内部 cell 已被 scale 放大（set_scale_factor 后
    // scaled_metrics.cell_w/h = CELL_W/H * scale），cols/rows 用物理像素除以 scaled cell。
    let scaled_cell_w = (CELL_W as f32 * scale) as u32;
    let scaled_cell_h = (CELL_H as f32 * scale) as u32;
    if scaled_cell_w == 0 || scaled_cell_h == 0 {
        return;
    }
    let new_cols = (avail_phys_w / scaled_cell_w) as usize;
    let new_rows = (avail_phys_h / scaled_cell_h) as usize;
    if new_cols == 0 || new_rows == 0 {
        return;
    }
    // 同步 renderer 的 scale_factor，使 scaled_metrics 与下方 cell 计算一致。
    ctx.renderer.set_scale_factor(scale);
    // 本地终端 resize
    ctx.manager
        .resize(rust_xterm_core::TerminalSize::new(new_rows, new_cols));
    // canvas 精确对齐 cols × scaled_cell，消除 letterbox。
    let canvas_w = new_cols as u32 * scaled_cell_w;
    let canvas_h = new_rows as u32 * scaled_cell_h;
    ctx.renderer.resize(canvas_w, canvas_h);
    ctx.renderer.clear();
    // 强制全屏重绘
    ctx.scroll_offset = 0;
    let snap = ctx.manager.screen_snapshot();
    let spans: Vec<DirtySpan> = snap
        .rows
        .iter()
        .enumerate()
        .map(|(row, cells)| DirtySpan {
            row,
            col_start: 0,
            col_end: cells.len(),
            cells: cells.clone(),
        })
        .collect();
    if !spans.is_empty() {
        ctx.renderer.render_frame(&spans);
    }
    let frame = ctx.manager.cursor();
    ctx.renderer.render_cursor(&frame);
    // 通知远端 SSH channel 窗口大小变更
    if let Some(bridge) = ctx.bridge.as_ref() {
        let _ = bridge.send_resize(new_cols as u16, new_rows as u16);
    }
}
