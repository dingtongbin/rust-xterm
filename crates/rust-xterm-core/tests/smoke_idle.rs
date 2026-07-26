//! 冒烟测试 3：静默 CPU 测试
//!
//! 验证在无数据输入时，`poll_frame` 返回 `None`，CPU 占用率为 0%。
//!
//! 测试场景：
//! 1. 创建终端管理器
//! 2. 写入少量数据后轮询帧
//! 3. 之后持续轮询，验证返回 None
//! 4. 监控 CPU 占用，验证接近 0%

use rust_xterm_core::{TerminalManager, TerminalSize};
use std::time::{Duration, Instant};

#[test]
fn test_idle_returns_none() {
    let mut mgr = TerminalManager::utf8(TerminalSize::new(24, 80));

    // 初始状态：无脏区，应返回 None
    let frame = mgr.poll_frame(Instant::now());
    assert!(frame.is_none(), "空闲状态应返回 None");
}

#[test]
fn test_after_consume_returns_none() {
    let mut mgr = TerminalManager::utf8(TerminalSize::new(24, 80));

    // 写入数据产生脏区
    mgr.write(b"Hello, rust-xterm!");

    // 第一次轮询：应返回 Some（有脏区）
    let frame = mgr.poll_frame(Instant::now());
    assert!(frame.is_some(), "有脏区时应返回 Some");

    // 第二次轮询：应返回 None（脏区已消费）
    let frame = mgr.poll_frame(Instant::now());
    assert!(frame.is_none(), "脏区消费后应返回 None");
}

#[test]
fn test_repeated_idle_no_cpu_spike() {
    let mut mgr = TerminalManager::utf8(TerminalSize::new(24, 80));

    // 写入初始数据
    mgr.write(b"test");
    let _ = mgr.poll_frame(Instant::now());

    // 持续轮询 1000 次，验证全部返回 None
    let start = Instant::now();
    for _ in 0..1000 {
        let frame = mgr.poll_frame(Instant::now());
        assert!(frame.is_none(), "空闲轮询应返回 None");
    }
    let elapsed = start.elapsed();

    // 1000 次空闲轮询应在极短时间内完成（< 100ms）
    // 证明没有不必要的计算
    assert!(
        elapsed < Duration::from_millis(100),
        "1000 次空闲轮询耗时: {elapsed:?}，应 < 100ms"
    );
}

#[test]
fn test_cursor_blink_does_not_force_render() {
    let mut mgr = TerminalManager::utf8(TerminalSize::new(24, 80));

    // 启用光标闪烁
    mgr.set_cursor_blinking(true);

    // 写入数据并消费
    mgr.write(b"test");
    let _ = mgr.poll_frame(Instant::now());

    // 在闪烁间隔内，应返回 None
    let frame = mgr.poll_frame(Instant::now());
    assert!(frame.is_none(), "闪烁未到期时应返回 None");
}

#[test]
fn test_blink_due_triggers_render() {
    let mut mgr = TerminalManager::utf8(TerminalSize::new(24, 80));

    // 启用光标闪烁
    mgr.set_cursor_blinking(true);

    // 写入数据并消费
    mgr.write(b"test");
    let _ = mgr.poll_frame(Instant::now());

    // 等待超过闪烁间隔（500ms）
    std::thread::sleep(Duration::from_millis(600));

    // 现在应返回 Some（闪烁到期）
    let frame = mgr.poll_frame(Instant::now());
    assert!(frame.is_some(), "闪烁到期后应返回 Some");
}

#[test]
fn test_resize_triggers_render() {
    let mut mgr = TerminalManager::utf8(TerminalSize::new(24, 80));

    // 写入数据并消费
    mgr.write(b"test");
    let _ = mgr.poll_frame(Instant::now());

    // resize 后应触发脏区
    mgr.resize(TerminalSize::new(30, 100));

    let frame = mgr.poll_frame(Instant::now());
    assert!(frame.is_some(), "resize 后应返回 Some");
}
