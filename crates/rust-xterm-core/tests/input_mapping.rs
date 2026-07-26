//! 键盘映射集成测试
//!
//! 覆盖 [`KeyMapping::encode_key`] 的所有变体，包括应用光标模式
//! 开关与 Ctrl/Alt 修饰键组合。

use rust_xterm_core::{KeyInput, KeyMapping, KeyMods};

#[test]
fn test_arrows_normal_mode() {
    let mods = KeyMods::default();
    assert_eq!(
        KeyMapping::encode_key(KeyInput::ArrowUp, mods, false),
        b"\x1b[A"
    );
    assert_eq!(
        KeyMapping::encode_key(KeyInput::ArrowDown, mods, false),
        b"\x1b[B"
    );
    assert_eq!(
        KeyMapping::encode_key(KeyInput::ArrowRight, mods, false),
        b"\x1b[C"
    );
    assert_eq!(
        KeyMapping::encode_key(KeyInput::ArrowLeft, mods, false),
        b"\x1b[D"
    );
}

#[test]
fn test_arrows_app_cursor_mode() {
    let mods = KeyMods::default();
    assert_eq!(
        KeyMapping::encode_key(KeyInput::ArrowUp, mods, true),
        b"\x1bOA"
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
fn test_home_end_normal_mode() {
    let mods = KeyMods::default();
    assert_eq!(
        KeyMapping::encode_key(KeyInput::Home, mods, false),
        b"\x1b[H"
    );
    assert_eq!(
        KeyMapping::encode_key(KeyInput::End, mods, false),
        b"\x1b[F"
    );
}

#[test]
fn test_home_end_app_cursor_mode() {
    let mods = KeyMods::default();
    assert_eq!(
        KeyMapping::encode_key(KeyInput::Home, mods, true),
        b"\x1bOH"
    );
    assert_eq!(KeyMapping::encode_key(KeyInput::End, mods, true), b"\x1bOF");
}

#[test]
fn test_function_keys_ss3() {
    let mods = KeyMods::default();
    assert_eq!(KeyMapping::encode_key(KeyInput::F1, mods, false), b"\x1bOP");
    assert_eq!(KeyMapping::encode_key(KeyInput::F2, mods, false), b"\x1bOQ");
    assert_eq!(KeyMapping::encode_key(KeyInput::F3, mods, false), b"\x1bOR");
    assert_eq!(KeyMapping::encode_key(KeyInput::F4, mods, false), b"\x1bOS");
}

#[test]
fn test_function_keys_csi() {
    let mods = KeyMods::default();
    assert_eq!(
        KeyMapping::encode_key(KeyInput::F5, mods, false),
        b"\x1b[15~"
    );
    assert_eq!(
        KeyMapping::encode_key(KeyInput::F6, mods, false),
        b"\x1b[17~"
    );
    assert_eq!(
        KeyMapping::encode_key(KeyInput::F7, mods, false),
        b"\x1b[18~"
    );
    assert_eq!(
        KeyMapping::encode_key(KeyInput::F8, mods, false),
        b"\x1b[19~"
    );
    assert_eq!(
        KeyMapping::encode_key(KeyInput::F9, mods, false),
        b"\x1b[20~"
    );
    assert_eq!(
        KeyMapping::encode_key(KeyInput::F10, mods, false),
        b"\x1b[21~"
    );
    assert_eq!(
        KeyMapping::encode_key(KeyInput::F11, mods, false),
        b"\x1b[23~"
    );
    assert_eq!(
        KeyMapping::encode_key(KeyInput::F12, mods, false),
        b"\x1b[24~"
    );
}

#[test]
fn test_edit_and_nav_keys() {
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
}

#[test]
fn test_control_keys() {
    let mods = KeyMods::default();
    assert_eq!(KeyMapping::encode_key(KeyInput::Enter, mods, false), b"\r");
    assert_eq!(
        KeyMapping::encode_key(KeyInput::Backspace, mods, false),
        b"\x7f"
    );
    assert_eq!(KeyMapping::encode_key(KeyInput::Tab, mods, false), b"\t");
    assert_eq!(KeyMapping::encode_key(KeyInput::Esc, mods, false), b"\x1b");
}

#[test]
fn test_char_plain_ascii() {
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
        KeyMapping::encode_key(KeyInput::Char('1'), mods, false),
        b"1"
    );
    assert_eq!(
        KeyMapping::encode_key(KeyInput::Char(' '), mods, false),
        b" "
    );
}

#[test]
fn test_char_plain_multibyte() {
    let mods = KeyMods::default();
    assert_eq!(
        KeyMapping::encode_key(KeyInput::Char('你'), mods, false),
        "你".as_bytes()
    );
    assert_eq!(
        KeyMapping::encode_key(KeyInput::Char('€'), mods, false),
        "€".as_bytes()
    );
}

#[test]
fn test_char_ctrl_letters() {
    let mods = KeyMods {
        ctrl: true,
        ..Default::default()
    };
    assert_eq!(
        KeyMapping::encode_key(KeyInput::Char('a'), mods, false),
        b"\x01"
    );
    assert_eq!(
        KeyMapping::encode_key(KeyInput::Char('c'), mods, false),
        b"\x03"
    );
    assert_eq!(
        KeyMapping::encode_key(KeyInput::Char('g'), mods, false),
        b"\x07"
    );
    assert_eq!(
        KeyMapping::encode_key(KeyInput::Char('z'), mods, false),
        b"\x1a"
    );
    // 大写字母产生相同控制码
    assert_eq!(
        KeyMapping::encode_key(KeyInput::Char('A'), mods, false),
        b"\x01"
    );
    assert_eq!(
        KeyMapping::encode_key(KeyInput::Char('C'), mods, false),
        b"\x03"
    );
}

#[test]
fn test_char_alt_prefix() {
    let mods = KeyMods {
        alt: true,
        ..Default::default()
    };
    assert_eq!(
        KeyMapping::encode_key(KeyInput::Char('a'), mods, false),
        b"\x1ba"
    );
    assert_eq!(
        KeyMapping::encode_key(KeyInput::Char('1'), mods, false),
        b"\x1b1"
    );
    // Alt + 多字节字符
    let mut expected = vec![0x1b];
    expected.extend_from_slice("你".as_bytes());
    assert_eq!(
        KeyMapping::encode_key(KeyInput::Char('你'), mods, false),
        expected
    );
}

#[test]
fn test_char_ctrl_alt_combo() {
    let mods = KeyMods {
        ctrl: true,
        alt: true,
        ..Default::default()
    };
    assert_eq!(
        KeyMapping::encode_key(KeyInput::Char('a'), mods, false),
        b"\x1b\x01"
    );
    assert_eq!(
        KeyMapping::encode_key(KeyInput::Char('c'), mods, false),
        b"\x1b\x03"
    );
}

#[test]
fn test_char_ctrl_non_letter_ignored() {
    // Ctrl + 非字母：ctrl 修饰不产生控制码，按普通字符编码
    let mods = KeyMods {
        ctrl: true,
        ..Default::default()
    };
    assert_eq!(
        KeyMapping::encode_key(KeyInput::Char('1'), mods, false),
        b"1"
    );
    assert_eq!(
        KeyMapping::encode_key(KeyInput::Char('-'), mods, false),
        b"-"
    );
}

#[test]
fn test_app_cursor_does_not_affect_other_keys() {
    let mods = KeyMods::default();
    // Insert/Delete/F-keys 不受 app_cursor 影响
    assert_eq!(
        KeyMapping::encode_key(KeyInput::Insert, mods, true),
        b"\x1b[2~"
    );
    assert_eq!(KeyMapping::encode_key(KeyInput::F1, mods, true), b"\x1bOP");
    assert_eq!(KeyMapping::encode_key(KeyInput::Enter, mods, true), b"\r");
}
