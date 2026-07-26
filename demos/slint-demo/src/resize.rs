//! resize 检测定时器回调主体
use crate::app_ctx::AppCtx;
use crate::{CELL_H, CELL_W, STATUS_BAR_H};
use slint::ComponentHandle;
use std::cell::RefCell;

pub(crate) fn handle_resize(ctx: &RefCell<AppCtx>, app: &crate::App, last_size: &mut (u32, u32)) {
    let window_size = app.window().size();
    let scale = app.window().scale_factor();
    let w_logical = window_size.width;
    let h_logical = window_size.height;
    if (w_logical, h_logical) != *last_size && w_logical > 0 && h_logical > STATUS_BAR_H {
        *last_size = (w_logical, h_logical);
        let avail_h_logical = h_logical - STATUS_BAR_H;
        let new_cols = (w_logical / CELL_W) as usize;
        let new_rows = (avail_h_logical / CELL_H) as usize;
        if new_cols > 0 && new_rows > 0 {
            let mut ctx = ctx.borrow_mut();
            ctx.event_loop.resize(new_rows, new_cols);
            // renderer 用物理像素：避免 HiDPI 下 canvas 比 Image 显示矩形小，被拉伸导致模糊
            let phys_w = (w_logical as f32 * scale) as u32;
            let phys_h = (avail_h_logical as f32 * scale) as u32;
            ctx.renderer.resize(phys_w, phys_h);
            ctx.renderer.clear();
            // 强制全屏重绘
            ctx.scroll_offset = 0;
            let snap = ctx.event_loop.manager_ref().screen_snapshot();
            let spans: Vec<rust_xterm_core::DirtySpan> = snap
                .rows
                .iter()
                .enumerate()
                .map(|(row, cells)| rust_xterm_core::DirtySpan {
                    row,
                    col_start: 0,
                    col_end: cells.len(),
                    cells: cells.clone(),
                })
                .collect();
            if !spans.is_empty() {
                ctx.renderer.render_frame(&spans);
            }
            let frame = ctx.event_loop.manager_ref().cursor();
            ctx.renderer.render_cursor(&frame);
        }
    }
}
