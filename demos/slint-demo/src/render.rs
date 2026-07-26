//! 渲染定时器回调主体：EventLoop::tick + 渲染 + FPS + 内存
use crate::app_ctx::AppCtx;
use crate::MEM_REFRESH_MS;
use rust_xterm_host::Event;
use slint::{Image as SlintImage, SharedPixelBuffer, SharedString};
use std::cell::RefCell;
use std::time::Instant;

pub(crate) fn tick(ctx: &RefCell<AppCtx>, app_weak: &slint::Weak<crate::App>) {
    let mut ctx = ctx.borrow_mut();
    // EventLoop::tick
    if let Some(event) = ctx.event_loop.tick() {
        match event {
            Event::FrameUpdate(frame) => {
                if ctx.scroll_offset == 0 {
                    // 正常脏区渲染
                    if !frame.dirty_spans.is_empty() {
                        ctx.renderer.render_frame(&frame.dirty_spans);
                    }
                    // 画光标
                    if frame.cursor.visible {
                        ctx.renderer.render_cursor(&frame.cursor);
                    }
                }
                // scroll_offset > 0 时跳过 live 渲染，保持 scrollback 视图
            }
            Event::Closed => {
                ctx.pty_alive = false;
                if let Some(app) = app_weak.upgrade() {
                    app.set_mem_text("PTY closed".into());
                }
            }
        }
    }

    // 若处于 scrollback 视图，全屏重绘
    if ctx.scroll_offset > 0 {
        let snap = ctx
            .event_loop
            .manager_ref()
            .snapshot_scrolled(ctx.scroll_offset);
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
    }

    // FPS
    let fps = ctx.fps_tracker.tick();

    // 内存刷新
    let now = Instant::now();
    if now.duration_since(ctx.last_mem_refresh).as_millis() > MEM_REFRESH_MS as u128 {
        let pid = ctx.pid;
        ctx.sys.refresh_process(pid);
        // sysinfo 0.30: Process::memory() 返回进程 RSS（字节）
        let used = ctx.sys.process(pid).map(|p| p.memory()).unwrap_or(0);
        let mb = (used as f64) / (1024.0 * 1024.0);
        ctx.last_mem_mb = mb;
        ctx.last_mem_refresh = now;
    }

    // 上传像素到 Slint Image
    let canvas = ctx.renderer.canvas();
    let buffer = canvas.buffer();
    let w = canvas.width();
    let h = canvas.height();
    let pixel_buffer = SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(buffer, w, h);
    let image = SlintImage::from_rgba8(pixel_buffer);

    if let Some(app) = app_weak.upgrade() {
        app.set_terminal_image(image);
        app.set_fps_text(SharedString::from(format!("FPS: {fps:>5.1}")));
        app.set_mem_text(SharedString::from(format!(
            "Mem: {:.1} MB",
            ctx.last_mem_mb
        )));
        app.set_scroll_text(SharedString::from(format!(
            "Scroll: {}{}",
            ctx.scroll_offset,
            if ctx.pty_alive { "" } else { " [PTY closed]" }
        )));
    }
}
