//! 键盘映射核心层
//!
//! 提供与 xterm.js 风格对齐的键盘输入抽象 [`KeyInput`] 与编码器
//! [`KeyMapping`]，将逻辑按键 + 修饰键编码为终端字节序列，
//! 供宿主层通过 PTY 发送给子进程。
//!
//! ## 编码规则
//!
//! - **方向键 / Home / End**：携带 ctrl/alt/shift 任一修饰键时输出
//!   modifyOtherKeys 编码 `\x1b[1;<modifier><final>`（modifier 见下）；
//!   无修饰键时应用光标模式（`app_cursor = true`）使用 SS3
//!   （`\x1bOA` 等），普通模式使用 CSI（`\x1b[A` 等）。
//! - **功能键 F1-F4**：携带修饰键时改用 CSI `\x1b[1;<modifier>P/Q/R/S`，
//!   否则使用 SS3 序列（`\x1bOP` 等）；**F5-F12**：携带修饰键时输出
//!   `\x1b[<param>;<modifier>~`，否则输出 `\x1b[<param>~`（如 F5 为 `\x1b[15~`）。
//! - **Insert / Delete / PageUp / PageDown**：CSI 序列（`\x1b[2~` 等）。
//! - **Enter / Backspace / Tab / Esc**：分别编码为 `\r` / `\x7f` / `\t` / `\x1b`；
//!   `Shift+Tab` 输出 backtab 序列 `\x1b[Z`。
//! - **Char**：
//!   - `Ctrl` + ASCII 字母 → `(ch as u8) & 0x1f`（控制字符）
//!   - `Alt` → 前置 `\x1b` 再拼接字符的 UTF-8 字节
//!   - 普通 → 字符的 UTF-8 字节
//!
//! 修饰键编码数字：`1 + shift*1 + alt*2 + ctrl*4`，范围 1..=8。
//! 修饰键状态复用 [`crate::mouse::KeyMods`]。

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
            KeyInput::ArrowUp => Self::arrow_key(app_cursor, mods, b'A'),
            KeyInput::ArrowDown => Self::arrow_key(app_cursor, mods, b'B'),
            KeyInput::ArrowRight => Self::arrow_key(app_cursor, mods, b'C'),
            KeyInput::ArrowLeft => Self::arrow_key(app_cursor, mods, b'D'),
            KeyInput::Home => Self::arrow_key(app_cursor, mods, b'H'),
            KeyInput::End => Self::arrow_key(app_cursor, mods, b'F'),
            KeyInput::Insert => b"\x1b[2~".to_vec(),
            KeyInput::Delete => b"\x1b[3~".to_vec(),
            KeyInput::PageUp => b"\x1b[5~".to_vec(),
            KeyInput::PageDown => b"\x1b[6~".to_vec(),
            KeyInput::F1 => Self::f1_f4(mods, b'P'),
            KeyInput::F2 => Self::f1_f4(mods, b'Q'),
            KeyInput::F3 => Self::f1_f4(mods, b'R'),
            KeyInput::F4 => Self::f1_f4(mods, b'S'),
            KeyInput::F5 => Self::f5_f12(mods, 15),
            KeyInput::F6 => Self::f5_f12(mods, 17),
            KeyInput::F7 => Self::f5_f12(mods, 18),
            KeyInput::F8 => Self::f5_f12(mods, 19),
            KeyInput::F9 => Self::f5_f12(mods, 20),
            KeyInput::F10 => Self::f5_f12(mods, 21),
            KeyInput::F11 => Self::f5_f12(mods, 23),
            KeyInput::F12 => Self::f5_f12(mods, 24),
            KeyInput::Enter => b"\r".to_vec(),
            KeyInput::Backspace => b"\x7f".to_vec(),
            // Shift+Tab 输出标准 backtab 序列；普通 Tab 与 Ctrl+Tab 保持 `\t`
            KeyInput::Tab => {
                if mods.shift {
                    b"\x1b[Z".to_vec()
                } else {
                    b"\t".to_vec()
                }
            }
            KeyInput::Esc => b"\x1b".to_vec(),
        }
    }

    /// 方向键 / Home / End 的编码
    ///
    /// - 携带 ctrl/alt/shift 任一修饰键时输出 modifyOtherKeys 编码
    ///   `\x1b[1;<modifier><final>`
    /// - 否则 `app_cursor = true` → SS3（`\x1bO<final>`）；
    ///   `false` → CSI（`\x1b[<final>`）
    fn arrow_key(app_cursor: bool, mods: KeyMods, final_byte: u8) -> Vec<u8> {
        if mods.shift || mods.alt || mods.ctrl {
            Self::csi_modified(1, Self::modifier_code(mods), final_byte)
        } else if app_cursor {
            vec![0x1b, b'O', final_byte]
        } else {
            vec![0x1b, b'[', final_byte]
        }
    }

    /// F1-F4 的编码
    ///
    /// - 携带修饰键时改用 CSI `\x1b[1;<modifier><final>`（modifyOtherKeys）
    /// - 否则使用 SS3 `\x1bO<final>`
    fn f1_f4(mods: KeyMods, final_byte: u8) -> Vec<u8> {
        if mods.shift || mods.alt || mods.ctrl {
            Self::csi_modified(1, Self::modifier_code(mods), final_byte)
        } else {
            vec![0x1b, b'O', final_byte]
        }
    }

    /// F5-F12 的编码
    ///
    /// - 携带修饰键时输出 `\x1b[<param>;<modifier>~`
    /// - 否则输出 `\x1b[<param>~`
    fn f5_f12(mods: KeyMods, param: u8) -> Vec<u8> {
        if mods.shift || mods.alt || mods.ctrl {
            Self::csi_modified(param, Self::modifier_code(mods), b'~')
        } else {
            let mut out = vec![0x1b, b'['];
            if param >= 10 {
                out.push(b'0' + param / 10);
            }
            out.push(b'0' + param % 10);
            out.push(b'~');
            out
        }
    }

    /// 修饰键编码数字：`1 + shift*1 + alt*2 + ctrl*4`，范围 1..=8
    fn modifier_code(mods: KeyMods) -> u8 {
        1 + (mods.shift as u8) + (mods.alt as u8) * 2 + (mods.ctrl as u8) * 4
    }

    /// 构造带修饰键的 CSI 序列 `\x1b[<param>;<modifier><final>`
    ///
    /// `modifier` 限定 1..=8，输出单 ASCII 数字；`param` 支持 1-2 位十进制。
    fn csi_modified(param: u8, modifier: u8, final_byte: u8) -> Vec<u8> {
        let mut out = vec![0x1b, b'['];
        if param >= 10 {
            out.push(b'0' + param / 10);
        }
        out.push(b'0' + param % 10);
        out.push(b';');
        out.push(b'0' + modifier);
        out.push(final_byte);
        out
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

    #[test]
    fn test_ctrl_arrow() {
        // Ctrl+Right/Up/Left/Down → modifier = 1 + 0 + 0 + 4 = 5
        let mods = KeyMods {
            ctrl: true,
            ..Default::default()
        };
        assert_eq!(
            KeyMapping::encode_key(KeyInput::ArrowRight, mods, false),
            b"\x1b[1;5C"
        );
        assert_eq!(
            KeyMapping::encode_key(KeyInput::ArrowUp, mods, false),
            b"\x1b[1;5A"
        );
        assert_eq!(
            KeyMapping::encode_key(KeyInput::ArrowLeft, mods, false),
            b"\x1b[1;5D"
        );
        assert_eq!(
            KeyMapping::encode_key(KeyInput::ArrowDown, mods, false),
            b"\x1b[1;5B"
        );
        // app_cursor 在携带修饰键时被忽略，统一走 modifyOtherKeys
        assert_eq!(
            KeyMapping::encode_key(KeyInput::ArrowUp, mods, true),
            b"\x1b[1;5A"
        );
    }

    #[test]
    fn test_shift_tab() {
        // Shift+Tab → 标准 backtab 序列 \x1b[Z
        let mods = KeyMods {
            shift: true,
            ..Default::default()
        };
        assert_eq!(
            KeyMapping::encode_key(KeyInput::Tab, mods, false),
            b"\x1b[Z"
        );
        // 普通 Tab 保持 \t
        assert_eq!(
            KeyMapping::encode_key(KeyInput::Tab, KeyMods::default(), false),
            b"\t"
        );
        // Ctrl+Tab 也保持 \t（xterm 默认行为）
        let ctrl_mods = KeyMods {
            ctrl: true,
            ..Default::default()
        };
        assert_eq!(
            KeyMapping::encode_key(KeyInput::Tab, ctrl_mods, false),
            b"\t"
        );
    }

    #[test]
    fn test_ctrl_function_key() {
        // Ctrl+F1 → \x1b[1;5P（modifier=5）
        let mods = KeyMods {
            ctrl: true,
            ..Default::default()
        };
        assert_eq!(
            KeyMapping::encode_key(KeyInput::F1, mods, false),
            b"\x1b[1;5P"
        );
        // Ctrl+F5 → \x1b[15;5~
        assert_eq!(
            KeyMapping::encode_key(KeyInput::F5, mods, false),
            b"\x1b[15;5~"
        );
        // Ctrl+F12 → \x1b[24;5~
        assert_eq!(
            KeyMapping::encode_key(KeyInput::F12, mods, false),
            b"\x1b[24;5~"
        );
    }

    #[test]
    fn test_alt_home() {
        // Alt+Home → modifier = 1 + 0 + 2 + 0 = 3
        let mods = KeyMods {
            alt: true,
            ..Default::default()
        };
        assert_eq!(
            KeyMapping::encode_key(KeyInput::Home, mods, false),
            b"\x1b[1;3H"
        );
        // app_cursor 不影响修饰键路径
        assert_eq!(
            KeyMapping::encode_key(KeyInput::Home, mods, true),
            b"\x1b[1;3H"
        );
        // Alt+End → \x1b[1;3F
        assert_eq!(
            KeyMapping::encode_key(KeyInput::End, mods, false),
            b"\x1b[1;3F"
        );
    }

    #[test]
    fn test_shift_arrow() {
        // Shift+Right → modifier = 1 + 1 + 0 + 0 = 2
        let mods = KeyMods {
            shift: true,
            ..Default::default()
        };
        assert_eq!(
            KeyMapping::encode_key(KeyInput::ArrowRight, mods, false),
            b"\x1b[1;2C"
        );
        // Shift+Left → \x1b[1;2D
        assert_eq!(
            KeyMapping::encode_key(KeyInput::ArrowLeft, mods, false),
            b"\x1b[1;2D"
        );
    }

    #[test]
    fn test_plain_arrow_unchanged() {
        // 不带 modifier 的 ArrowUp 在 app_cursor=false 时仍为 \x1b[A
        let mods = KeyMods::default();
        assert_eq!(
            KeyMapping::encode_key(KeyInput::ArrowUp, mods, false),
            b"\x1b[A"
        );
        // app_cursor=true 时仍走 SS3 路径 \x1bOA
        assert_eq!(
            KeyMapping::encode_key(KeyInput::ArrowUp, mods, true),
            b"\x1bOA"
        );
        // Home/End 无修饰键时不受影响
        assert_eq!(
            KeyMapping::encode_key(KeyInput::Home, mods, false),
            b"\x1b[H"
        );
        assert_eq!(
            KeyMapping::encode_key(KeyInput::End, mods, false),
            b"\x1b[F"
        );
    }
}
