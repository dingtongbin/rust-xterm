//! 冒烟测试 2：GBK 编码测试
//!
//! 验证 CodecGate 闸门正确处理 GBK 编码的中文输出。
//!
//! 测试场景：
//! 1. 模拟 `chcp 936` 环境下的中文输出
//! 2. 验证 GBK 字节流被正确解码为 UTF-8
//! 3. 验证断包处理（半个 GBK 字符跨包）
//! 4. 验证非法序列不导致 panic

use rust_xterm_core::{Codec, CodecGate, TerminalManager, TerminalSize};
use std::time::Instant;

#[test]
fn test_gbk_chinese_output() {
    let mut mgr = TerminalManager::gbk(TerminalSize::new(24, 80));

    // "你好世界" 的 GBK 编码
    // 你 = C4E3, 好 = BAC3, 世 = CAC0, 界 = BDE7
    let gkb_bytes: Vec<u8> = vec![0xC4, 0xE3, 0xBA, 0xC3, 0xCA, 0xC0, 0xBD, 0xE7];

    mgr.write(&gkb_bytes);

    // 轮询帧
    let frame = mgr.poll_frame(Instant::now());
    assert!(frame.is_some());

    // 验证屏幕上包含中文
    let snapshot = mgr.screen_snapshot();
    let full_text: String = snapshot
        .rows
        .iter()
        .flat_map(|row| row.iter().map(|c| c.text.as_str()))
        .collect();

    assert!(
        full_text.contains("你好世界"),
        "屏幕应包含'你好世界'，实际: '{full_text}'"
    );
}

#[test]
fn test_gbk_split_packet_handling() {
    let mut mgr = TerminalManager::gbk(TerminalSize::new(24, 80));

    // "你好" 的 GBK 编码，拆成 4 个单字节包
    let bytes: Vec<u8> = vec![0xC4, 0xE3, 0xBA, 0xC3];

    // 逐字节写入，模拟最极端的断包
    for byte in &bytes {
        mgr.write(&[*byte]);
    }

    let snapshot = mgr.screen_snapshot();
    let full_text: String = snapshot
        .rows
        .iter()
        .flat_map(|row| row.iter().map(|c| c.text.as_str()))
        .collect();

    assert!(
        full_text.contains("你好"),
        "断包后应正确解码'你好'，实际: '{full_text}'"
    );
}

#[test]
fn test_gbk_mixed_ascii_chinese() {
    let mut mgr = TerminalManager::gbk(TerminalSize::new(24, 80));

    // 混合 ASCII 和 GBK 中文
    // "Hello 你好 World" 的 GBK 编码
    let mut data: Vec<u8> = b"Hello ".to_vec();
    data.extend_from_slice(&[0xC4, 0xE3, 0xBA, 0xC3]); // 你好
    data.extend_from_slice(b" World");

    mgr.write(&data);

    let snapshot = mgr.screen_snapshot();
    let full_text: String = snapshot
        .rows
        .iter()
        .flat_map(|row| row.iter().map(|c| c.text.as_str()))
        .collect();

    assert!(
        full_text.contains("Hello") && full_text.contains("你好") && full_text.contains("World"),
        "应正确解码混合内容，实际: '{full_text}'"
    );
}

#[test]
fn test_gbk_invalid_sequence_no_crash() {
    let mut gate = CodecGate::gbk();

    // 各种非法序列
    let invalid_sequences: Vec<Vec<u8>> = vec![
        vec![0xFF, 0xFE],
        vec![0x80, 0x80],
        vec![0xFE, 0xFF],
        vec![0x00, 0x80],
    ];

    for seq in &invalid_sequences {
        // 不应 panic
        let result = gate.decode(seq);
        println!("解码 {seq:?} -> {result:?}");
    }
}

#[test]
fn test_gbk_ansi_escape_sequences() {
    let mut mgr = TerminalManager::gbk(TerminalSize::new(24, 80));

    // ANSI 转义序列 + GBK 中文
    // \x1b[31m (红色) + 你好 + \x1b[0m (重置)
    let mut data: Vec<u8> = b"\x1b[31m".to_vec();
    data.extend_from_slice(&[0xC4, 0xE3, 0xBA, 0xC3]); // 你好
    data.extend_from_slice(b"\x1b[0m");

    mgr.write(&data);

    let frame = mgr.poll_frame(Instant::now());
    assert!(frame.is_some());

    let snapshot = mgr.screen_snapshot();
    let full_text: String = snapshot
        .rows
        .iter()
        .flat_map(|row| row.iter().map(|c| c.text.as_str()))
        .collect();

    assert!(full_text.contains("你好"));
}

#[test]
fn test_codec_switch() {
    // 验证可以动态切换编码
    let mut gate = CodecGate::utf8();
    assert_eq!(gate.codec(), Codec::Utf8);

    gate.set_codec(Codec::Gbk);
    assert_eq!(gate.codec(), Codec::Gbk);

    // 切换后应正确解码 GBK
    let gbk_bytes = [0xC4, 0xE3, 0xBA, 0xC3];
    let result = gate.decode(&gbk_bytes);
    assert_eq!(result, "你好");
}

#[test]
fn test_gbk_encode_user_input() {
    let mut gate = CodecGate::gbk();

    // 验证 UTF-8 -> GBK 编码
    let encoded = gate.encode("你好");
    assert_eq!(encoded, vec![0xC4, 0xE3, 0xBA, 0xC3]);
}
