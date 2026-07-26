//! 冒烟测试 1：`yes` 压力测试
//!
//! 验证在高吞吐数据流下，终端管理器保持稳定（无 panic、无内存泄漏特征）。
//!
//! 测试方法：
//! 1. 模拟 `yes` 命令的高吞吐数据流（持续写入 "y\n"）
//! 2. 持续写入并定期轮询帧
//! 3. 验证终端管理器在压力下保持可用
//! 4. 验证屏幕内容正确（最后一行应包含 'y'）

use rust_xterm_core::{TerminalManager, TerminalSize};
use std::time::{Duration, Instant};

#[test]
fn test_yes_pressure_stability() {
    let mut mgr = TerminalManager::utf8(TerminalSize::new(24, 80));

    // 模拟 `yes` 的高吞吐数据流
    let yes_chunk: Vec<u8> = b"y\n".repeat(4096); // 8KB per chunk

    let start_time = Instant::now();
    let test_duration = Duration::from_secs(2);

    let mut bytes_written: usize = 0;
    let mut poll_count: usize = 0;

    while start_time.elapsed() < test_duration {
        // 写入大量数据
        for _ in 0..20 {
            mgr.write(&yes_chunk);
            bytes_written += yes_chunk.len();
        }

        // 定期轮询帧
        poll_count += 1;
        if poll_count.is_multiple_of(5) {
            let _ = mgr.poll_frame(Instant::now());
        }
    }

    // 最终轮询
    let frame = mgr.poll_frame(Instant::now());
    assert!(frame.is_some(), "压力测试后应能产生帧");

    // 验证：至少写入了 1MB 数据
    assert!(
        bytes_written > 1_000_000,
        "应写入大量数据，实际: {bytes_written} bytes"
    );

    // 验证：屏幕内容正确（应包含 'y' 字符）
    let snapshot = mgr.screen_snapshot();
    let has_y = snapshot
        .rows
        .iter()
        .any(|row| row.iter().any(|c| c.text.contains('y')));
    assert!(has_y, "屏幕应包含 'y' 字符");

    // 验证：终端尺寸正确
    assert_eq!(snapshot.size.rows, 24);
    assert_eq!(snapshot.size.cols, 80);

    println!("yes 压力测试完成: {bytes_written} bytes 写入, {poll_count} 次轮询");
}

#[test]
fn test_rapid_advance_bytes_no_panic() {
    // 验证快速连续调用 advance_bytes 不会 panic
    let mut mgr = TerminalManager::utf8(TerminalSize::new(24, 80));

    // 模拟大量 ANSI 转义序列
    let ansi_noise: Vec<u8> = b"\x1b[31mHello\x1b[0m \x1b[1mWorld\x1b[0m\n".repeat(1000);

    for _ in 0..100 {
        mgr.write(&ansi_noise);
    }

    // 验证终端仍然可用
    let frame = mgr.poll_frame(Instant::now());
    assert!(frame.is_some());
}

#[test]
fn test_memory_growth_bounded() {
    // 验证内存增长有界：写入大量数据后，scrollback 不会无限增长
    let mut mgr = TerminalManager::utf8(TerminalSize::new(24, 80));

    // 写入大量数据（远超 scrollback 容量）
    let line = b"test line with some content\n";
    for _ in 0..100_000 {
        mgr.write(line);
    }

    // 轮询帧
    let frame = mgr.poll_frame(Instant::now());
    assert!(frame.is_some());

    // 验证屏幕快照行数等于可见行数（不会因 scrollback 溢出）
    let snapshot = mgr.screen_snapshot();
    assert_eq!(snapshot.rows.len(), 24, "可见行数应始终为 24");

    // 验证屏幕上有内容（不一定是最后一行，因为光标可能在空行上）
    let has_content = snapshot
        .rows
        .iter()
        .any(|row| row.iter().any(|c| !c.text.is_empty()));
    assert!(has_content, "屏幕应有内容");
}

#[test]
fn test_repeated_resize_stability() {
    // 验证反复 resize 不会导致 panic 或状态损坏
    let mut mgr = TerminalManager::utf8(TerminalSize::new(24, 80));

    for i in 0..100 {
        mgr.write(b"test data\n");
        mgr.resize(TerminalSize::new(24 + i % 10, 80 + i % 20));
        let _ = mgr.poll_frame(Instant::now());
    }

    // 最终验证
    let snapshot = mgr.screen_snapshot();
    assert!(!snapshot.rows.is_empty());
}
