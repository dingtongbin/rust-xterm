//! PTY 集成冒烟测试
//!
//! 验证 PTY 桥接与 TerminalManager 的端到端集成。
//!
//! 测试场景：
//! 1. 启动 echo 命令
//! 2. 发送输入
//! 3. 验证 PTY 输出被正确桥接到终端管理器

use rust_xterm_core::{TerminalManager, TerminalSize};
use rust_xterm_host::{Event, EventLoop, EventLoopConfig, PtyBridge, PtyConfig};
use std::time::{Duration, Instant};

#[test]
fn test_pty_echo_integration() {
    // 创建终端管理器
    let mgr = TerminalManager::utf8(TerminalSize::new(24, 80));

    // 创建 PTY 配置：使用 echo 命令
    let pty_config = PtyConfig {
        shell: "/bin/echo".to_string(),
        cols: 80,
        rows: 24,
        cwd: None,
    };

    // 启动 PTY
    let pty = PtyBridge::new(&pty_config).expect("PTY 启动失败");

    // 创建事件循环
    let mut event_loop = EventLoop::new(mgr, Some(pty), EventLoopConfig::default());

    // 等待 PTY 输出
    let start = Instant::now();
    let mut got_frame = false;

    while start.elapsed() < Duration::from_secs(2) {
        if let Some(event) = event_loop.tick() {
            match event {
                Event::FrameUpdate(frame) => {
                    got_frame = true;
                    // 验证有脏区
                    assert!(!frame.dirty_rects.is_empty());
                    break;
                }
                Event::Closed => {
                    // PTY 可能很快退出，这也是正常的
                    break;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    // echo 命令可能很快退出，所以 got_frame 可能为 false
    // 关键是不 panic
    println!("PTY 集成测试完成, got_frame={got_frame}");
}

#[test]
fn test_pty_resize() {
    let mgr = TerminalManager::utf8(TerminalSize::new(24, 80));

    let pty_config = PtyConfig {
        shell: "/bin/cat".to_string(),
        cols: 80,
        rows: 24,
        cwd: None,
    };

    let pty = PtyBridge::new(&pty_config).expect("PTY 启动失败");
    let mut event_loop = EventLoop::new(mgr, Some(pty), EventLoopConfig::default());

    // 调整大小
    event_loop.resize(30, 100);

    // 验证不 panic
    let _ = event_loop.tick();
}

#[test]
fn test_pty_send_input() {
    let mgr = TerminalManager::utf8(TerminalSize::new(24, 80));

    let pty_config = PtyConfig {
        shell: "/bin/cat".to_string(),
        cols: 80,
        rows: 24,
        cwd: None,
    };

    let pty = PtyBridge::new(&pty_config).expect("PTY 启动失败");
    let mut event_loop = EventLoop::new(mgr, Some(pty), EventLoopConfig::default());

    // 发送输入
    event_loop.send_input(b"hello\n").expect("发送失败");

    // 等待回显
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(1) {
        let _ = event_loop.tick();
        std::thread::sleep(Duration::from_millis(10));
    }

    // 验证屏幕上有内容
    let snapshot = event_loop.manager().screen_snapshot();
    let has_content = snapshot
        .rows
        .iter()
        .any(|row| row.iter().any(|c| !c.text.is_empty()));

    assert!(has_content, "屏幕应有回显内容");
}

#[test]
fn test_detect_default_shell_spawns_and_echoes() {
    // 验证 detect_default_shell() 返回的 shell 能被 PtyBridge 真正启动，
    // 且发送的输入能被回显到屏幕（端到端连接 + 渲染管线均可达）。
    let detected = PtyConfig::detect_default_shell();
    println!("detected shell: {detected}");
    assert!(!detected.is_empty(), "detect_default_shell 不应返回空");

    let mgr = TerminalManager::utf8(TerminalSize::new(24, 80));
    let pty_cfg = PtyConfig {
        shell: detected,
        cols: 80,
        rows: 24,
        cwd: None,
    };
    let pty = PtyBridge::new(&pty_cfg).expect("PTY 启动失败（detected shell 不可用）");
    let mut event_loop = EventLoop::new(mgr, Some(pty), EventLoopConfig::default());

    // 发送 echo 命令（兼容 sh / bash / zsh / pwsh 的 echo）
    event_loop
        .send_input(b"echo rust_xterm_demo_ok_42\n")
        .expect("发送失败");

    // 等待 shell 处理并回显
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(2) {
        let _ = event_loop.tick();
        std::thread::sleep(Duration::from_millis(20));
    }

    // 验证屏幕上能看到发送的标记字符串
    let snapshot = event_loop.manager().screen_snapshot();
    let screen: String = snapshot
        .rows
        .iter()
        .flat_map(|row| row.iter().map(|c| c.text.as_str()))
        .collect();
    assert!(
        screen.contains("rust_xterm_demo_ok_42"),
        "屏幕应包含 echo 的输出标记，实际:\n{screen}"
    );
}
