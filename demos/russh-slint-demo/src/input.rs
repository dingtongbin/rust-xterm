//! 键盘输入处理：具名键映射 + key-pressed 回调主体
//!
//! 与 slint-demo 的差异：
//! - slint-demo 调用 `event_loop.send_input(bytes)` 写入本地 PTY
//! - 本 demo 调用 `SshBridge::send_input(bytes)` 发送到远端 SSH channel
//! - 本地 TerminalManager **不**做本地回显：远端 shell 负责回显（如 bash 默认 echo）

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
    // Shift+PageUp/PageDown：scrollback 导航
    if mods.shift && !mods.ctrl && !mods.alt {
        match text {
            "PageUp" | "PageDown" => {
                let mut ctx = ctx.borrow_mut();
                let max = ctx.manager.max_scrollback();
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
    if let Some(key) = map_named_key(text) {
        let data = KeyMapping::encode_key(key, mods, false);
        let mut ctx = ctx.borrow_mut();
        if let Some(bridge) = ctx.bridge.as_ref() {
            let _ = bridge.send_input(data);
        }
        // 任何非导航键按下时，重置 scrollback
        if !is_nav_key(text) {
            ctx.scroll_offset = 0;
        }
        return;
    }
    // 普通字符
    if !text.is_empty() {
        let bytes = text.as_bytes();
        let mut ctx = ctx.borrow_mut();
        // Alt 前缀 ESC
        if mods.alt && bytes.len() == 1 && bytes[0] < 0x80 {
            let mut data = vec![0x1b];
            data.extend_from_slice(bytes);
            if let Some(bridge) = ctx.bridge.as_ref() {
                let _ = bridge.send_input(data);
            }
        } else if mods.ctrl && bytes.len() == 1 {
            let c = bytes[0];
            if c.is_ascii_lowercase() || c.is_ascii_uppercase() {
                // Ctrl+字母 → 用 KeyMapping 保证一致编码
                let key = KeyInput::Char(c.to_ascii_lowercase() as char);
                let data = KeyMapping::encode_key(key, mods, false);
                if let Some(bridge) = ctx.bridge.as_ref() {
                    let _ = bridge.send_input(data);
                }
            } else if let Some(bridge) = ctx.bridge.as_ref() {
                let _ = bridge.send_input(bytes.to_vec());
            }
        } else if let Some(bridge) = ctx.bridge.as_ref() {
            let _ = bridge.send_input(bytes.to_vec());
            // 普通字符输入重置 scrollback
            ctx.scroll_offset = 0;
        }
    }
}

/// 具名键映射：Slint 的 key-pressed event.text 可能是具名字符串
pub(crate) fn map_named_key(text: &str) -> Option<KeyInput> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_named_key_arrows() {
        assert_eq!(map_named_key("Up"), Some(KeyInput::ArrowUp));
        assert_eq!(map_named_key("ArrowDown"), Some(KeyInput::ArrowDown));
        assert_eq!(map_named_key("Left"), Some(KeyInput::ArrowLeft));
        assert_eq!(map_named_key("Right"), Some(KeyInput::ArrowRight));
    }

    #[test]
    fn test_map_named_key_function_keys() {
        assert_eq!(map_named_key("F1"), Some(KeyInput::F1));
        assert_eq!(map_named_key("F12"), Some(KeyInput::F12));
    }

    #[test]
    fn test_map_named_key_returns_none_for_chars() {
        assert_eq!(map_named_key("a"), None);
        assert_eq!(map_named_key("1"), None);
        assert_eq!(map_named_key(""), None);
    }

    #[test]
    fn test_map_named_key_enter_and_backspace() {
        assert_eq!(map_named_key("Return"), Some(KeyInput::Enter));
        assert_eq!(map_named_key("Enter"), Some(KeyInput::Enter));
        assert_eq!(map_named_key("Backspace"), Some(KeyInput::Backspace));
        assert_eq!(map_named_key("Tab"), Some(KeyInput::Tab));
        assert_eq!(map_named_key("Escape"), Some(KeyInput::Esc));
        assert_eq!(map_named_key("Esc"), Some(KeyInput::Esc));
    }

    #[test]
    fn test_is_nav_key_basic() {
        assert!(is_nav_key("Up"));
        assert!(is_nav_key("ArrowLeft"));
        assert!(is_nav_key("PageUp"));
        assert!(!is_nav_key("Return"));
        assert!(!is_nav_key("F1"));
    }

    #[test]
    fn test_encode_arrow_up() {
        let mods = KeyMods::default();
        let data = KeyMapping::encode_key(KeyInput::ArrowUp, mods, false);
        assert_eq!(data, b"\x1b[A");
    }

    #[test]
    fn test_encode_enter() {
        let mods = KeyMods::default();
        let data = KeyMapping::encode_key(KeyInput::Enter, mods, false);
        assert_eq!(data, b"\r");
    }

    #[test]
    fn test_encode_ctrl_c() {
        let mods = KeyMods {
            ctrl: true,
            ..KeyMods::default()
        };
        let data = KeyMapping::encode_key(KeyInput::Char('c'), mods, false);
        assert_eq!(data, b"\x03");
    }
}
