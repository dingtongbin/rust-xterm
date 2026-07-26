//! 键盘映射核心层
//!
//! 提供与 xterm.js 风格对齐的键盘输入抽象 [`KeyInput`] 与编码器
//! [`KeyMapping`]，将逻辑按键 + 修饰键编码为终端字节序列，
//! 供宿主层通过 PTY 发送给子进程。
//!
//! ## 编码规则
//!
//! - **方向键 / Home / End**：应用光标模式（`app_cursor = true`）使用 SS3
//!   （`\x1bOA` 等），普通模式使用 CSI（`\x1b[A` 等）。
//! - **功能键 F1-F4**：SS3 序列（`\x1bOP` 等）；**F5-F12**：CSI 序列
//!   （`\x1b[15~` 等）。
//! - **Insert / Delete / PageUp / PageDown**：CSI 序列（`\x1b[2~` 等）。
//! - **Enter / Backspace / Tab / Esc**：分别编码为 `\r` / `\x7f` / `\t` / `\x1b`。
//! - **Char**：
//!   - `Ctrl` + ASCII 字母 → `(ch as u8) & 0x1f`（控制字符）
//!   - `Alt` → 前置 `\x1b` 再拼接字符的 UTF-8 字节
//!   - 普通 → 字符的 UTF-8 字节
//!
//! 修饰键复用 [`crate::mouse::KeyMods`]。

use crate::mouse::KeyMods;

/// 逻辑按键
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyInput {
    /// 可打印字符
    Char(char),
    /// 方向键上
    ArrowUp,
    /// 方向键下
    ArrowDown,
    /// 方向键左
    ArrowLeft,
    /// 方向键右
    ArrowRight,
    /// Home
    Home,
    /// End
    End,
    /// Insert
    Insert,
    /// Delete
    Delete,
    /// PageUp
    PageUp,
    /// PageDown
    PageDown,
    /// 功能键 F1
    F1,
    /// 功能键 F2
    F2,
    /// 功能键 F3
    F3,
    /// 功能键 F4
    F4,
    /// 功能键 F5
    F5,
    /// 功能键 F6
    F6,
    /// 功能键 F7
    F7,
    /// 功能键 F8
    F8,
    /// 功能键 F9
    F9,
    /// 功能键 F10
    F10,
    /// 功能键 F11
    F11,
    /// 功能键 F12
    F12,
    /// 回车
    Enter,
    /// 退格
    Backspace,
    /// 制表符
    Tab,
    /// Esc
    Esc,
}

/// 键盘编码器
///
/// 将 [`KeyInput`] + [`KeyMods`] 编码为终端字节序列。
/// 所有方法均为无状态纯函数。
pub struct KeyMapping;

impl KeyMapping {
    /// 将按键编码为字节序列
    ///
    /// - `key`：逻辑按键
    /// - `mods`：修饰键状态
    /// - `app_cursor`：是否处于应用光标模式（DECSET 1 / DECSET 1006 之外，
    ///   由宿主根据 `\x1b[?1h` 维护）
    pub fn encode_key(key: KeyInput, mods: KeyMods, app_cursor: bool) -> Vec<u8> {
        match key {
            KeyInput::Char(ch) => Self::encode_char(ch, mods),
            KeyInput::ArrowUp => Self::arrow(app_cursor, b'A'),
            KeyInput::ArrowDown => Self::arrow(app_cursor, b'B'),
            KeyInput::ArrowRight => Self::arrow(app_cursor, b'C'),
            KeyInput::ArrowLeft => Self::arrow(app_cursor, b'D'),
            KeyInput::Home => Self::arrow(app_cursor, b'H'),
            KeyInput::End => Self::arrow(app_cursor, b'F'),
            KeyInput::Insert => b"\x1b[2~".to_vec(),
            KeyInput::Delete => b"\x1b[3~".to_vec(),
            KeyInput::PageUp => b"\x1b[5~".to_vec(),
            KeyInput::PageDown => b"\x1b[6~".to_vec(),
            KeyInput::F1 => b"\x1bOP".to_vec(),
            KeyInput::F2 => b"\x1bOQ".to_vec(),
            KeyInput::F3 => b"\x1bOR".to_vec(),
            KeyInput::F4 => b"\x1bOS".to_vec(),
            KeyInput::F5 => b"\x1b[15~".to_vec(),
            KeyInput::F6 => b"\x1b[17~".to_vec(),
            KeyInput::F7 => b"\x1b[18~".to_vec(),
            KeyInput::F8 => b"\x1b[19~".to_vec(),
            KeyInput::F9 => b"\x1b[20~".to_vec(),
            KeyInput::F10 => b"\x1b[21~".to_vec(),
            KeyInput::F11 => b"\x1b[23~".to_vec(),
            KeyInput::F12 => b"\x1b[24~".to_vec(),
            KeyInput::Enter => b"\r".to_vec(),
            KeyInput::Backspace => b"\x7f".to_vec(),
            KeyInput::Tab => b"\t".to_vec(),
            KeyInput::Esc => b"\x1b".to_vec(),
        }
    }

    /// 方向键 / Home / End 的编码
    ///
    /// `app_cursor = true` → SS3（`\x1bO<final>`）；`false` → CSI（`\x1b[<final>`）。
    fn arrow(app_cursor: bool, final_byte: u8) -> Vec<u8> {
        if app_cursor {
            vec![0x1b, b'O', final_byte]
        } else {
            vec![0x1b, b'[', final_byte]
        }
    }

    /// 字符键的编码
    fn encode_char(ch: char, mods: KeyMods) -> Vec<u8> {
        // Ctrl + ASCII 字母 → 控制字符 (ch as u8) & 0x1f
        if mods.ctrl && ch.is_ascii_alphabetic() {
            let ctrl_byte = (ch as u32 as u8) & 0x1f;
            let mut out = Vec::new();
            if mods.alt {
                out.push(0x1b);
            }
            out.push(ctrl_byte);
            return out;
        }
        let mut out = Vec::new();
        if mods.alt {
            out.push(0x1b);
        }
        // 普通或 Alt：追加字符的 UTF-8 字节
        let mut buf = [0u8; 4];
        let s = ch.encode_utf8(&mut buf);
        out.extend_from_slice(s.as_bytes());
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arrows_app_cursor() {
        let mods = KeyMods::default();
        assert_eq!(
            KeyMapping::encode_key(KeyInput::ArrowUp, mods, true),
            b"\x1bOA"
        );
        assert_eq!(
            KeyMapping::encode_key(KeyInput::ArrowUp, mods, false),
            b"\x1b[A"
        );
        assert_eq!(
            KeyMapping::encode_key(KeyInput::ArrowDown, mods, true),
            b"\x1bOB"
        );
        assert_eq!(
            KeyMapping::encode_key(KeyInput::ArrowRight, mods, true),
            b"\x1bOC"
        );
        assert_eq!(
            KeyMapping::encode_key(KeyInput::ArrowLeft, mods, true),
            b"\x1bOD"
        );
    }

    #[test]
    fn test_home_end_app_cursor() {
        let mods = KeyMods::default();
        assert_eq!(
            KeyMapping::encode_key(KeyInput::Home, mods, true),
            b"\x1bOH"
        );
        assert_eq!(
            KeyMapping::encode_key(KeyInput::Home, mods, false),
            b"\x1b[H"
        );
        assert_eq!(KeyMapping::encode_key(KeyInput::End, mods, true), b"\x1bOF");
        assert_eq!(
            KeyMapping::encode_key(KeyInput::End, mods, false),
            b"\x1b[F"
        );
    }

    #[test]
    fn test_function_keys() {
        let mods = KeyMods::default();
        assert_eq!(KeyMapping::encode_key(KeyInput::F1, mods, false), b"\x1bOP");
        assert_eq!(KeyMapping::encode_key(KeyInput::F4, mods, false), b"\x1bOS");
        assert_eq!(
            KeyMapping::encode_key(KeyInput::F5, mods, false),
            b"\x1b[15~"
        );
        assert_eq!(
            KeyMapping::encode_key(KeyInput::F12, mods, false),
            b"\x1b[24~"
        );
    }

    #[test]
    fn test_special_keys() {
        let mods = KeyMods::default();
        assert_eq!(
            KeyMapping::encode_key(KeyInput::Insert, mods, false),
            b"\x1b[2~"
        );
        assert_eq!(
            KeyMapping::encode_key(KeyInput::Delete, mods, false),
            b"\x1b[3~"
        );
        assert_eq!(
            KeyMapping::encode_key(KeyInput::PageUp, mods, false),
            b"\x1b[5~"
        );
        assert_eq!(
            KeyMapping::encode_key(KeyInput::PageDown, mods, false),
            b"\x1b[6~"
        );
        assert_eq!(KeyMapping::encode_key(KeyInput::Enter, mods, false), b"\r");
        assert_eq!(
            KeyMapping::encode_key(KeyInput::Backspace, mods, false),
            b"\x7f"
        );
        assert_eq!(KeyMapping::encode_key(KeyInput::Tab, mods, false), b"\t");
        assert_eq!(KeyMapping::encode_key(KeyInput::Esc, mods, false), b"\x1b");
    }

    #[test]
    fn test_char_plain() {
        let mods = KeyMods::default();
        assert_eq!(
            KeyMapping::encode_key(KeyInput::Char('a'), mods, false),
            b"a"
        );
        assert_eq!(
            KeyMapping::encode_key(KeyInput::Char('Z'), mods, false),
            b"Z"
        );
        assert_eq!(
            KeyMapping::encode_key(KeyInput::Char('你'), mods, false),
            "你".as_bytes()
        );
    }

    #[test]
    fn test_char_ctrl() {
        let mods = KeyMods {
            ctrl: true,
            ..Default::default()
        };
        assert_eq!(
            KeyMapping::encode_key(KeyInput::Char('a'), mods, false),
            b"\x01"
        );
        assert_eq!(
            KeyMapping::encode_key(KeyInput::Char('z'), mods, false),
            b"\x1a"
        );
        assert_eq!(
            KeyMapping::encode_key(KeyInput::Char('A'), mods, false),
            b"\x01"
        );
        assert_eq!(
            KeyMapping::encode_key(KeyInput::Char('c'), mods, false),
            b"\x03"
        );
    }

    #[test]
    fn test_char_alt() {
        let mods = KeyMods {
            alt: true,
            ..Default::default()
        };
        assert_eq!(
            KeyMapping::encode_key(KeyInput::Char('a'), mods, false),
            b"\x1ba"
        );
        assert_eq!(KeyMapping::encode_key(KeyInput::Char('你'), mods, false), {
            let mut v = vec![0x1b];
            v.extend_from_slice("你".as_bytes());
            v
        });
    }

    #[test]
    fn test_char_ctrl_alt() {
        let mods = KeyMods {
            ctrl: true,
            alt: true,
            ..Default::default()
        };
        assert_eq!(
            KeyMapping::encode_key(KeyInput::Char('a'), mods, false),
            b"\x1b\x01"
        );
    }
}
