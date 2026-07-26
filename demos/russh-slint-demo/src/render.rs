//! 渲染定时器回调主体
//!
//! 每次 tick 完成：
//! 1. 非阻塞 drain `SshBridge.event_rx` 全部事件：
//!    - Status → 更新遮罩状态文本（`status-text`）
//!    - Connected → 切换 `connecting=false`，撤销遮罩，标记本地 channel_alive
//!    - Data → `manager.write(bytes)` 喂入本地终端
//!    - Error → 切回遮罩并显示红色错误信息
//!    - Closed → 标记 channel 已关闭，更新底部状态栏
//! 2. 若已连接：`manager.poll_frame` → 渲染脏区 + 光标
//! 3. 若处于 scrollback 视图：全屏重绘快照
//! 4. `manager.drain_output()` → `bridge.send_input`（鼠标报告、CSI 6n 等回环）
//! 5. FPS / 内存刷新 / 上传像素到 Slint Image
use crate::app_ctx::AppCtx;
use crate::ssh::SshEvent;
use crate::MEM_REFRESH_MS;
use slint::{Image as SlintImage, SharedPixelBuffer, SharedString};
use std::cell::RefCell;
use std::time::Instant;

pub(crate) fn tick(ctx: &RefCell<AppCtx>, app_weak: &slint::Weak<crate::App>) {
    let mut ctx = ctx.borrow_mut();

    // ---- 1. drain SSH 事件 ----
    // 必须先取出所有事件再 poll_frame，避免单帧多次 set_connecting 引起抖动
    let mut data_buf: Vec<Vec<u8>> = Vec::new();
    let mut status_update: Option<String> = None;
    let mut error_update: Option<String> = None;
    let mut should_connect: bool = false;
    let mut should_close: bool = false;

    {
        // 临时不可变借用 bridge，仅 drain 事件，不在块内修改 ctx
        let bridge_opt = ctx.bridge.as_ref();
        if let Some(bridge) = bridge_opt {
            while let Ok(event) = bridge.event_rx.try_recv() {
                match event {
                    SshEvent::Status(s) => status_update = Some(s),
                    SshEvent::Connected => {
                        should_connect = true;
                    }
                    SshEvent::Data(bytes) => data_buf.push(bytes),
                    SshEvent::Error(msg) => error_update = Some(msg),
                    SshEvent::Closed => {
                        should_close = true;
                    }
                }
            }
        }
    }
    // 在不可变借用结束后，再修改 ctx 状态
    if should_connect {
        ctx.connected = true;
        ctx.channel_alive = true;
    }
    if should_close {
        ctx.channel_alive = false;
    }

    // 应用 UI 更新（遮罩状态、错误信息）
    if let Some(app) = app_weak.upgrade() {
        if let Some(s) = status_update {
            app.set_status_text(SharedString::from(s));
        }
        if should_connect {
            app.set_connecting(false);
            app.set_ssh_text(SharedString::from("SSH: connected"));
        }
        if let Some(err) = error_update {
            app.set_connecting(true);
            app.set_error_text(SharedString::from(err));
        }
        if should_close {
            if !error_update_was_set(&app) {
                app.set_connecting(true);
                app.set_status_text(SharedString::from("SSH 连接已关闭"));
            }
            app.set_ssh_text(SharedString::from("SSH: closed"));
        }
    }

    // 喂入 SSH 数据到本地终端
    for bytes in data_buf {
        ctx.manager.write(&bytes);
    }

    // ---- 2. poll_frame + 渲染（仅已连接时） ----
    if ctx.connected {
        let now = Instant::now();
        if let Some(frame) = ctx.manager.poll_frame(now) {
            if ctx.scroll_offset == 0 {
                if !frame.dirty_spans.is_empty() {
                    ctx.renderer.render_frame(&frame.dirty_spans);
                }
                if frame.cursor.visible {
                    ctx.renderer.render_cursor(&frame.cursor);
                }
            }
        }
    }

    // ---- 3. scrollback 视图全屏重绘 ----
    if ctx.scroll_offset > 0 {
        let snap = ctx.manager.snapshot_scrolled(ctx.scroll_offset);
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

    // ---- 4. drain_output 回环（鼠标报告等） ----
    if ctx.connected {
        let out = ctx.manager.drain_output();
        if !out.is_empty() {
            if let Some(bridge) = ctx.bridge.as_ref() {
                let _ = bridge.send_input(out);
            }
        }
    }

    // ---- 5. FPS + 内存 ----
    let fps = ctx.fps_tracker.tick();
    let now = Instant::now();
    if now.duration_since(ctx.last_mem_refresh).as_millis() > MEM_REFRESH_MS as u128 {
        let pid = ctx.pid;
        ctx.sys.refresh_process(pid);
        let used = ctx.sys.process(pid).map(|p| p.memory()).unwrap_or(0);
        let mb = (used as f64) / (1024.0 * 1024.0);
        ctx.last_mem_mb = mb;
        ctx.last_mem_refresh = now;
    }

    // ---- 6. 上传像素到 Slint Image + 状态栏文本 ----
    let canvas = ctx.renderer.canvas();
    let buffer = canvas.buffer();
    let w = canvas.width();
    let h = canvas.height();
    // 即使 w/h 为 0 也要避免 SharedPixelBuffer::clone_from_slice panic
    if w > 0 && h > 0 {
        let pixel_buffer = SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(buffer, w, h);
        let image = SlintImage::from_rgba8(pixel_buffer);
        if let Some(app) = app_weak.upgrade() {
            app.set_terminal_image(image);
        }
    }

    if let Some(app) = app_weak.upgrade() {
        app.set_fps_text(SharedString::from(format!("FPS: {fps:>5.1}")));
        app.set_mem_text(SharedString::from(format!(
            "Mem: {:.1} MB",
            ctx.last_mem_mb
        )));
        app.set_scroll_text(SharedString::from(format!(
            "Scroll: {}{}",
            ctx.scroll_offset,
            if ctx.channel_alive {
                ""
            } else {
                " [SSH closed]"
            }
        )));
    }
}

/// 检查 app 当前是否已经显示错误（避免被 Closed 误覆盖）
fn error_update_was_set(app: &crate::App) -> bool {
    !app.get_error_text().is_empty()
}
