//! 渲染定时器回调主体：EventLoop::tick + 渲染 + FPS + 内存
//!
//! 每次 tick 完成：
//! 1. 检测窗口 size 变化 → 实时 resize 同步
//! 2. `EventLoop::tick` → 渲染脏区 + 光标（跟踪脏区/光标变化）
//! 3. 若处于 scrollback 视图：全屏重绘快照
//! 4. 空闲快速路径：无变更时跳过 fps/mem/状态栏重计算
//! 5. FPS / 内存刷新 / 仅在有像素变更时上传到 Slint Image
//! 6. 选区高亮渲染：在像素上传前调用 render_selection
//! 7. 状态栏文本（dirty 检查，仅在变化时 set）
//!
//! 渲染节流：仅在脏区 / 光标变化 / scrollback 重绘时上传像素到 Slint Image。
//! 状态栏文本 dirty 检查：仅在字符串变化时 set，减少 Slint 属性通知开销。
use crate::app_ctx::AppCtx;
use crate::MEM_REFRESH_MS;
use rust_xterm_host::Event;
use slint::{ComponentHandle, Image as SlintImage, SharedPixelBuffer, SharedString};
use std::cell::RefCell;
use std::time::Instant;

pub(crate) fn tick(ctx: &RefCell<AppCtx>, app_weak: &slint::Weak<crate::App>) {
    let mut ctx = ctx.borrow_mut();

    // ---- 0. 检测窗口 size 变化，实时 resize 同步 ----
    // 原 200ms resize_timer 已删除，改为每 tick (16ms) 检测，达到实时同步。
    let mut force_upload = false;
    if let Some(app) = app_weak.upgrade() {
        let window_size = app.window().size();
        let scale = app.window().scale_factor();
        let cur = (window_size.width, window_size.height);
        if cur != ctx.last_window_size && window_size.width > 0 && window_size.height > 0 {
            ctx.last_window_size = cur;
            crate::resize::handle_resize_now(&mut ctx, &app, scale);
            force_upload = true;
        }
    }

    // ---- 1. EventLoop::tick + 渲染 ----
    let mut has_dirty = false;
    let mut cursor_changed = false;

    if let Some(event) = ctx.event_loop.tick() {
        match event {
            Event::FrameUpdate(frame) => {
                if ctx.scroll_offset == 0 {
                    // 正常脏区渲染
                    if !frame.dirty_spans.is_empty() {
                        let render_result = ctx.renderer.render_frame(&frame.dirty_spans);
                        has_dirty = !render_result.dirty_rects.is_empty();
                    }
                    // 跟踪光标可见性变化（闪烁 / 移动），变化时触发像素上传
                    let cursor_visible = frame.cursor.visible;
                    if ctx.last_cursor_visible != Some(cursor_visible) {
                        cursor_changed = true;
                        ctx.last_cursor_visible = Some(cursor_visible);
                    }
                    // 画光标
                    if cursor_visible {
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

    // ---- 2. scrollback 视图全屏重绘 ----
    let mut scrollback_redrawn = false;
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
            scrollback_redrawn = true;
        }
    }

    // ---- 3. 空闲快速路径 ----
    // 空闲 = 无脏区、无光标变化、无 scrollback 重绘、无强制上传。
    // 空闲时跳过 fps_tracker / mem refresh / 状态栏 dirty 检查等重计算，
    // 仅更新 FPS=0 和 scroll 属性，大幅降低空闲 CPU 占用。
    let idle = !has_dirty && !cursor_changed && !scrollback_redrawn && !force_upload;
    if idle {
        if let Some(app) = app_weak.upgrade() {
            // FPS 显示 0（fps_tracker 不被调用，避免空闲时计数）
            let idle_fps = "FPS:   0";
            if ctx.last_fps_text.as_deref() != Some(idle_fps) {
                app.set_fps_text(SharedString::from(idle_fps));
                ctx.last_fps_text = Some(idle_fps.to_string());
            }
            // scroll_text：组合 scroll_offset + pty_alive（dirty 检查）
            let scroll_str = format!(
                "Scroll: {}{}",
                ctx.scroll_offset,
                if ctx.pty_alive { "" } else { " [PTY closed]" }
            );
            if ctx.last_scroll_text.as_deref() != Some(scroll_str.as_str()) {
                app.set_scroll_text(SharedString::from(scroll_str.clone()));
                ctx.last_scroll_text = Some(scroll_str);
            }
        }
        return; // 提前返回，跳过后续重计算
    }

    // ---- 4. 内存刷新（非空闲时执行） ----
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

    // ---- 5. 像素上传（fps_tracker 仅在 should_upload 时调用；render_selection 渲染选区高亮） ----
    let mut fps = 0.0f64;
    let should_upload = has_dirty || cursor_changed || scrollback_redrawn || force_upload;
    if should_upload {
        // fps_tracker 仅在实际渲染时计数
        fps = ctx.fps_tracker.tick();
        // 渲染选区高亮（在像素上传前，反相被选 cell）
        if let Some(sel) = ctx.event_loop.manager_ref().selection() {
            let snap = ctx.event_loop.manager_ref().screen_snapshot();
            ctx.renderer.render_selection(&sel, &snap.rows);
        }
        let canvas = ctx.renderer.canvas();
        let buffer = canvas.buffer();
        let w = canvas.width();
        let h = canvas.height();
        if w > 0 && h > 0 {
            let pixel_buffer =
                SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(buffer, w, h);
            let image = SlintImage::from_rgba8(pixel_buffer);
            if let Some(app) = app_weak.upgrade() {
                app.set_terminal_image(image);
            }
        }
    }

    // ---- 6. 状态栏文本（dirty 检查，仅在变化时 set） ----
    if let Some(app) = app_weak.upgrade() {
        // fps：取整数位比较以减少 set 频率
        let fps_str = format!("FPS: {:>3}", fps as i32);
        if ctx.last_fps_text.as_deref() != Some(fps_str.as_str()) {
            app.set_fps_text(SharedString::from(fps_str.clone()));
            ctx.last_fps_text = Some(fps_str);
        }
        // mem：已 500ms 节流，但 set 仍每帧执行，加 dirty 检查
        let mem_str = format!("Mem: {:.1} MB", ctx.last_mem_mb);
        if ctx.last_mem_text.as_deref() != Some(mem_str.as_str()) {
            app.set_mem_text(SharedString::from(mem_str.clone()));
            ctx.last_mem_text = Some(mem_str);
        }
        // scroll_text：组合 scroll_offset + pty_alive
        let scroll_str = format!(
            "Scroll: {}{}",
            ctx.scroll_offset,
            if ctx.pty_alive { "" } else { " [PTY closed]" }
        );
        if ctx.last_scroll_text.as_deref() != Some(scroll_str.as_str()) {
            app.set_scroll_text(SharedString::from(scroll_str.clone()));
            ctx.last_scroll_text = Some(scroll_str);
        }
    }
}
