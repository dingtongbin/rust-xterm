//! 键盘输入处理：具名键映射 + key-pressed 回调主体
use crate::app_ctx::AppCtx;
use rust_xterm_core::input::{KeyInput, KeyMapping};
use rust_xterm_core::mouse::KeyMods;
use std::cell::RefCell;

pub(crate) fn handle_key_pressed(
    ctx: &RefCell<AppCtx>,
    text: &str,
    ctrl: bool,
    alt: bool,
    shift: bool,
) {
    let mods = KeyMods { ctrl, alt, shift };
    {
        // 保存当前修饰键状态，供鼠标回调读取（矩形选区等）
        let mut ctx = ctx.borrow_mut();
        ctx.current_mods = mods;
    }
    // Shift+PageUp/PageDown：scrollback 导航（Slint 1.6 TouchArea 无滚轮事件，
    // 用键盘快捷键替代）
    if mods.shift && !mods.ctrl && !mods.alt {
        match text {
            "PageUp" | "PageDown" => {
                let mut ctx = ctx.borrow_mut();
                let max = ctx.event_loop.manager_ref().max_scrollback();
                if text == "PageUp" {
                    ctx.scroll_offset = (ctx.scroll_offset + 5).min(max.max(1));
                } else {
                    ctx.scroll_offset = ctx.scroll_offset.saturating_sub(5);
                }
                return;
            }
            _ => {}
        }
    }
    // 优先尝试映射具名键
    if let Some(key) = map_named_key(text, &mods) {
        let mut ctx = ctx.borrow_mut();
        let _ = ctx.event_loop.send_key(key, mods);
        // 任何非导航键按下时，重置 scrollback
        if !is_nav_key(text) {
            ctx.scroll_offset = 0;
        }
        return;
    }
    // 普通字符
    if !text.is_empty() {
        let bytes = text.as_bytes();
        // Alt 前缀 ESC
        if mods.alt && bytes.len() == 1 && bytes[0] < 0x80 {
            let mut data = vec![0x1b];
            data.extend_from_slice(bytes);
            let mut ctx = ctx.borrow_mut();
            let _ = ctx.event_loop.send_input(&data);
        } else if mods.ctrl && bytes.len() == 1 {
            let c = bytes[0];
            if c.is_ascii_lowercase() || c.is_ascii_uppercase() {
                // Ctrl+字母 → 用 KeyMapping 保证一致编码
                let key = KeyInput::Char(c.to_ascii_lowercase() as char);
                let data = KeyMapping::encode_key(key, mods, false);
                let mut ctx = ctx.borrow_mut();
                let _ = ctx.event_loop.send_input(&data);
            } else {
                let mut ctx = ctx.borrow_mut();
                let _ = ctx.event_loop.send_input(bytes);
            }
        } else {
            let mut ctx = ctx.borrow_mut();
            let _ = ctx.event_loop.send_input(bytes);
            // 普通字符输入重置 scrollback
            ctx.scroll_offset = 0;
        }
    }
}

/// 具名键映射：Slint 的 key-pressed event.text 可能是具名字符串
pub(crate) fn map_named_key(text: &str, _mods: &KeyMods) -> Option<KeyInput> {
    match text {
        "Up" | "ArrowUp" => Some(KeyInput::ArrowUp),
        "Down" | "ArrowDown" => Some(KeyInput::ArrowDown),
        "Left" | "ArrowLeft" => Some(KeyInput::ArrowLeft),
        "Right" | "ArrowRight" => Some(KeyInput::ArrowRight),
        "Home" => Some(KeyInput::Home),
        "End" => Some(KeyInput::End),
        "Insert" => Some(KeyInput::Insert),
        "Delete" => Some(KeyInput::Delete),
        "PageUp" => Some(KeyInput::PageUp),
        "PageDown" => Some(KeyInput::PageDown),
        "Return" | "Enter" => Some(KeyInput::Enter),
        "Backspace" => Some(KeyInput::Backspace),
        "Tab" => Some(KeyInput::Tab),
        "Escape" | "Esc" => Some(KeyInput::Esc),
        "F1" => Some(KeyInput::F1),
        "F2" => Some(KeyInput::F2),
        "F3" => Some(KeyInput::F3),
        "F4" => Some(KeyInput::F4),
        "F5" => Some(KeyInput::F5),
        "F6" => Some(KeyInput::F6),
        "F7" => Some(KeyInput::F7),
        "F8" => Some(KeyInput::F8),
        "F9" => Some(KeyInput::F9),
        "F10" => Some(KeyInput::F10),
        "F11" => Some(KeyInput::F11),
        "F12" => Some(KeyInput::F12),
        _ => None,
    }
}

pub(crate) fn is_nav_key(text: &str) -> bool {
    matches!(
        text,
        "Up" | "Down"
            | "Left"
            | "Right"
            | "ArrowUp"
            | "ArrowDown"
            | "ArrowLeft"
            | "ArrowRight"
            | "PageUp"
            | "PageDown"
    )
}
