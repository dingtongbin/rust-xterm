//! 鼠标事件处理：pointer-event 回调主体
//!
//! 与 slint-demo 的差异：
//! - 中键粘贴不再走 `event_loop.send_input`，改走 `bridge.send_input`
//! - 鼠标跟踪模式下产生的响应（manager.drain_output）由 render::tick 统一回环
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
) {
    let mut ctx = ctx.borrow_mut();
    let col = (x as u32 / CELL_W) as usize;
    let row = (y as u32 / CELL_H) as usize;
    let cols = ctx.manager.size().cols;
    let rows = ctx.manager.size().rows;
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
    let mods = ctx.current_mods;
    ctx.manager.mouse_event(col, row, action, mouse_btn, mods);

    // 释放左键后：若选区非空，复制到剪贴板
    if need_copy {
        let text = ctx.manager.selection_text();
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
                    let bracketed = ctx.manager.is_bracketed_paste_enabled();
                    if bracketed {
                        if let Some(bridge) = ctx.bridge.as_ref() {
                            let _ = bridge.send_input(b"\x1b[200~".to_vec());
                            let _ = bridge.send_input(text.as_bytes().to_vec());
                            let _ = bridge.send_input(b"\x1b[201~".to_vec());
                        }
                    } else if let Some(bridge) = ctx.bridge.as_ref() {
                        let _ = bridge.send_input(text.as_bytes().to_vec());
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_button_mapping_logic() {
        // 不实际操作 ctx，仅验证 i32→MouseButton 的映射逻辑（无 ssh 启动）
        let map = |b: i32| match b {
            1 => MouseButton::Left,
            2 => MouseButton::Right,
            3 => MouseButton::Middle,
            _ => MouseButton::None,
        };
        assert_eq!(map(1), MouseButton::Left);
        assert_eq!(map(2), MouseButton::Right);
        assert_eq!(map(3), MouseButton::Middle);
        assert_eq!(map(0), MouseButton::None);
        assert_eq!(map(99), MouseButton::None);
    }

    #[test]
    fn test_action_mapping_logic() {
        let map = |k: i32| match k {
            0 => Some(MouseAction::Press),
            1 => Some(MouseAction::Release),
            2 => Some(MouseAction::Move),
            _ => None,
        };
        assert_eq!(map(0), Some(MouseAction::Press));
        assert_eq!(map(1), Some(MouseAction::Release));
        assert_eq!(map(2), Some(MouseAction::Move));
        assert_eq!(map(99), None);
    }
}
