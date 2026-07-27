//! 渲染定时器回调主体
//!
//! 每次 tick 完成：
//! 1. 检测窗口 size 变化 → 实时 resize 同步（Task 6）
//! 2. 非阻塞 drain `SshBridge.event_rx` 全部事件（64KB 上限，Task 7）：
//!    - Status → 更新遮罩状态文本（`status-text`）
//!    - Connected → 切换 `connecting=false`，撤销遮罩，标记本地 channel_alive
//!    - Data → `manager.write(bytes)` 喂入本地终端
//!    - Error → 切回遮罩并显示红色错误信息
//!    - Closed → 标记 channel 已关闭，`renderer.clear()` 清屏并强制上传一次
//! 3. 若已连接：`manager.poll_frame` → 渲染脏区 + 光标（跟踪脏区/光标变化）
//! 4. 若处于 scrollback 视图：全屏重绘快照
//! 5. `manager.drain_output()` → `bridge.send_input`（鼠标报告、CSI 6n 等回环）
//! 6. 空闲快速路径（Task 5）：无变更时跳过 fps/mem/状态栏重计算
//! 7. FPS / 内存刷新 / 仅在有像素变更时上传到 Slint Image
//! 8. 选区高亮渲染（Task 4.1）：在像素上传前调用 render_selection
//! 9. 状态栏文本 + scroll 属性（dirty 检查，仅在变化时 set）
use crate::app_ctx::AppCtx;
use crate::ssh::SshEvent;
use crate::MEM_REFRESH_MS;
use slint::{ComponentHandle, Image as SlintImage, SharedPixelBuffer, SharedString};
use std::cell::RefCell;
use std::time::Instant;

/// 单次 tick 最多 drain 的 SSH 数据字节数（Task 7：防止大数据流卡帧）。
/// 超出部分留到下一 tick，保证按键回包延迟 < 100ms。
const MAX_DRAIN_BYTES: usize = 64 * 1024;

pub(crate) fn tick(ctx: &RefCell<AppCtx>, app_weak: &slint::Weak<crate::App>) {
    let mut ctx = ctx.borrow_mut();

    // ---- 0. 检测窗口 size 变化，实时 resize 同步 (Task 6) ----
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

    // ---- 1. drain SSH 事件（Task 7: 64KB 上限） ----
    // 必须先取出所有事件再 poll_frame，避免单帧多次 set_connecting 引起抖动
    let mut data_buf: Vec<Vec<u8>> = Vec::new();
    let mut status_update: Option<String> = None;
    let mut error_update: Option<String> = None;
    let mut should_connect: bool = false;
    let mut should_close: bool = false;
    let mut has_data: bool = false;
    let mut drained_bytes: usize = 0;

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
                    SshEvent::Data(bytes) => {
                        has_data = true;
                        drained_bytes += bytes.len();
                        data_buf.push(bytes);
                        if drained_bytes >= MAX_DRAIN_BYTES {
                            break; // 剩余事件留到下一 tick
                        }
                    }
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
    // Closed：标记断连 + 清屏 + 强制上传一次清屏后像素（避免画面停滞）
    if should_close {
        ctx.channel_alive = false;
        ctx.connected = false;
        ctx.renderer.clear();
        force_upload = true;
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
    let mut has_dirty = false;
    let mut cursor_changed = false;
    let mut scrollback_redrawn = false;

    if ctx.connected {
        let now = Instant::now();
        if let Some(frame) = ctx.manager.poll_frame(now) {
            if ctx.scroll_offset == 0 {
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
                if cursor_visible {
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
            scrollback_redrawn = true;
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

    // ---- 5. 空闲快速路径 (Task 5.4) ----
    // 空闲 = 无数据、无脏区、无光标变化、无 scrollback 重绘、无强制上传。
    // 空闲时跳过 fps_tracker / mem refresh / 状态栏 dirty 检查等重计算，
    // 仅更新 FPS=0 和 scroll 属性，大幅降低空闲 CPU 占用。
    let idle = !has_data && !has_dirty && !cursor_changed && !scrollback_redrawn && !force_upload;
    if idle {
        if let Some(app) = app_weak.upgrade() {
            // FPS 显示 0（fps_tracker 不被调用，避免空闲时计数）
            let idle_fps = "FPS:   0";
            if ctx.last_fps_text.as_deref() != Some(idle_fps) {
                app.set_fps_text(SharedString::from(idle_fps));
                ctx.last_fps_text = Some(idle_fps.to_string());
            }
            // scroll 属性（dirty 检查）
            let scroll_max = ctx.manager.max_scrollback();
            if ctx.last_scroll_max != Some(scroll_max) {
                app.set_scroll_max(scroll_max as i32);
                ctx.last_scroll_max = Some(scroll_max);
            }
            let scroll_offset = ctx.scroll_offset;
            if ctx.last_scroll_offset != Some(scroll_offset) {
                app.set_scroll_offset(scroll_offset as i32);
                ctx.last_scroll_offset = Some(scroll_offset);
            }
        }
        return; // 提前返回，跳过后续重计算
    }

    // ---- 6. 内存刷新（非空闲时执行） ----
    let now = Instant::now();
    if now.duration_since(ctx.last_mem_refresh).as_millis() > MEM_REFRESH_MS as u128 {
        let pid = ctx.pid;
        ctx.sys.refresh_process(pid);
        let used = ctx.sys.process(pid).map(|p| p.memory()).unwrap_or(0);
        let mb = (used as f64) / (1024.0 * 1024.0);
        ctx.last_mem_mb = mb;
        ctx.last_mem_refresh = now;
    }

    // ---- 7. 像素上传 (Task 5.3: fps_tracker 仅在 should_upload 时调用; Task 4.1: render_selection) ----
    let mut fps = 0.0f64;
    let should_upload = has_dirty || cursor_changed || scrollback_redrawn || force_upload;
    if should_upload {
        // Task 5.3: fps_tracker 仅在实际渲染时计数
        fps = ctx.fps_tracker.tick();
        // Task 4.1: 渲染选区高亮（在像素上传前，反相被选 cell）
        if let Some(sel) = ctx.manager.selection() {
            let snap = ctx.manager.screen_snapshot();
            ctx.renderer.render_selection(&sel, &snap.rows);
        }
        let canvas = ctx.renderer.canvas();
        let buffer = canvas.buffer();
        let w = canvas.width();
        let h = canvas.height();
        // 即使 w/h 为 0 也要避免 SharedPixelBuffer::clone_from_slice panic
        if w > 0 && h > 0 {
            let pixel_buffer =
                SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(buffer, w, h);
            let image = SlintImage::from_rgba8(pixel_buffer);
            if let Some(app) = app_weak.upgrade() {
                app.set_terminal_image(image);
            }
        }
    }

    // ---- 8. 状态栏文本（dirty 检查，仅在变化时 set） ----
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
        // scroll_text：组合 scroll_offset + channel_alive
        let scroll_str = format!(
            "Scroll: {}{}",
            ctx.scroll_offset,
            if ctx.channel_alive {
                ""
            } else {
                " [SSH closed]"
            }
        );
        if ctx.last_scroll_text.as_deref() != Some(scroll_str.as_str()) {
            app.set_scroll_text(SharedString::from(scroll_str.clone()));
            ctx.last_scroll_text = Some(scroll_str);
        }
    }

    // ---- 9. scroll 属性更新（dirty 检查） ----
    if let Some(app) = app_weak.upgrade() {
        let scroll_max = ctx.manager.max_scrollback();
        if ctx.last_scroll_max != Some(scroll_max) {
            app.set_scroll_max(scroll_max as i32);
            ctx.last_scroll_max = Some(scroll_max);
        }
        let scroll_offset = ctx.scroll_offset;
        if ctx.last_scroll_offset != Some(scroll_offset) {
            app.set_scroll_offset(scroll_offset as i32);
            ctx.last_scroll_offset = Some(scroll_offset);
        }
    }
}

/// 检查 app 当前是否已经显示错误（避免被 Closed 误覆盖）
fn error_update_was_set(app: &crate::App) -> bool {
    !app.get_error_text().is_empty()
}
