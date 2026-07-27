//! 鼠标事件处理：pointer-event 回调主体
//!
//! HiDPI 修正：像素→列换算预留 `scale` 参数（与 russh-slint-demo 一致）。
//! Slint pointer-event 的 mouse-x/mouse-y 是逻辑像素，CELL_W/CELL_H 也是逻辑像素，
//! 直接相除即得列/行号；scale 参数留作未来 Slint 改为物理像素坐标时的扩展点。
use crate::app_ctx::{AppCtx, ClipboardHandle};
use crate::{CELL_H, CELL_W};
use rust_xterm_core::mouse::{MouseAction, MouseButton};
use std::cell::RefCell;

pub(crate) fn handle_pointer_event(
    ctx: &RefCell<AppCtx>,
    clipboard: &ClipboardHandle,
    x: f32,
    y: f32,
    kind: i32,
    button: i32,
    scale: f32,
) {
    let mut ctx = ctx.borrow_mut();
    // HiDPI 坐标换算：Slint pointer-event 的 mouse-x/mouse-y 是逻辑像素，
    // CELL_W/CELL_H 也是逻辑像素，直接相除即得列/行号。
    // scale 参数预留（若未来 Slint 改为物理像素坐标，需改用 CELL_W * scale）。
    let _ = scale;
    let col = (x as u32 / CELL_W) as usize;
    let row = (y as u32 / CELL_H) as usize;
    let cols = ctx.event_loop.manager_ref().size().cols;
    let rows = ctx.event_loop.manager_ref().size().rows;
    let col = col.min(cols.saturating_sub(1));
    let row = row.min(rows.saturating_sub(1));

    let mouse_btn = match button {
        1 => MouseButton::Left,
        2 => MouseButton::Right,
        3 => MouseButton::Middle,
        _ => MouseButton::None,
    };
    let action = match kind {
        0 => MouseAction::Press,
        1 => MouseAction::Release,
        2 => MouseAction::Move,
        _ => return,
    };

    let need_copy = mouse_btn == MouseButton::Left && action == MouseAction::Release;
    let need_paste = mouse_btn == MouseButton::Middle && action == MouseAction::Press;

    // 转发：manager 内部自动判定鼠标跟踪模式 vs 选区模式
    // 使用 current_mods（由 key-pressed 回调维护）而非 default，
    // 以支持 Alt+拖拽矩形选区
    let mods = ctx.current_mods;
    ctx.event_loop
        .manager()
        .mouse_event(col, row, action, mouse_btn, mods);

    // 释放左键后：若选区非空，复制到剪贴板
    if need_copy {
        let text = ctx.event_loop.manager_ref().selection_text();
        if let Some(t) = text {
            if !t.is_empty() {
                if let Ok(mut cb) = clipboard.lock() {
                    let _ = cb.set_text(t);
                }
            }
        }
    }

    // 中键粘贴（支持 bracketed paste）
    if need_paste {
        if let Ok(mut cb) = clipboard.lock() {
            if let Ok(text) = cb.get_text() {
                if !text.is_empty() {
                    let bracketed = ctx.event_loop.manager_ref().is_bracketed_paste_enabled();
                    if bracketed {
                        let _ = ctx.event_loop.send_input(b"\x1b[200~");
                        let _ = ctx.event_loop.send_input(text.as_bytes());
                        let _ = ctx.event_loop.send_input(b"\x1b[201~");
                    } else {
                        let _ = ctx.event_loop.send_input(text.as_bytes());
                    }
                }
            }
        }
    }
}
