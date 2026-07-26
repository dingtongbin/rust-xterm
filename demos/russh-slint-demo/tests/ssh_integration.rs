//! 集成测试：启动 mock SSH server，验证 SshBridge 完整连接流程。
//!
//! 覆盖场景：
//! 1. 密码认证成功 → 收到 greeting → echo 输入 → 优雅关闭
//! 2. 密码认证失败 → SshEvent::Error
//! 3. resize 命令 → 服务器收到 window_change_request
//! 4. SSH channel 关闭 → SshEvent::Closed

use rand_core::OsRng;
use russh::server::{self, Auth, Server as _};
use russh::{ChannelId, CryptoVec, Pty};
use russh_slint_demo::config::SshConfig;
use russh_slint_demo::ssh::{SshBridge, SshCommand, SshEvent};
use std::sync::Arc;
use std::time::Duration;

// -----------------------------------------------------------------------------
// Mock SSH Server
// -----------------------------------------------------------------------------

/// 服务端观察到的 resize 信号：`Some((cols, rows))` 表示收到 window_change。
type ResizeSignal = Arc<std::sync::Mutex<Option<(u32, u32)>>>;
/// 服务端观察到的 channel 关闭信号：`true` 表示 channel 已关闭。
type ClosedSignal = Arc<std::sync::Mutex<bool>>;

#[derive(Clone)]
struct MockServer {
    username: String,
    password: String,
    greeting: String,
    /// 用于检测 resize 是否到达
    resize_signal: ResizeSignal,
    /// 用于检测 channel 关闭
    closed_signal: ClosedSignal,
}

impl MockServer {
    fn new(username: &str, password: &str, greeting: &str) -> (Self, ResizeSignal, ClosedSignal) {
        let resize_signal: ResizeSignal = Arc::new(std::sync::Mutex::new(None));
        let closed_signal: ClosedSignal = Arc::new(std::sync::Mutex::new(false));
        let server = Self {
            username: username.into(),
            password: password.into(),
            greeting: greeting.into(),
            resize_signal: resize_signal.clone(),
            closed_signal: closed_signal.clone(),
        };
        (server, resize_signal, closed_signal)
    }
}

impl server::Server for MockServer {
    type Handler = MockHandler;
    fn new_client(&mut self, _peer: Option<std::net::SocketAddr>) -> MockHandler {
        MockHandler {
            config: self.clone(),
            channel: None,
        }
    }
}

struct MockHandler {
    config: MockServer,
    channel: Option<ChannelId>,
}

impl server::Handler for MockHandler {
    type Error = russh::Error;

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        if user == self.config.username && password == self.config.password {
            Ok(Auth::Accept)
        } else {
            Ok(Auth::Reject {
                proceed_with_methods: None,
            })
        }
    }

    async fn channel_open_session(
        &mut self,
        channel: russh::Channel<russh::server::Msg>,
        _session: &mut russh::server::Session,
    ) -> Result<bool, Self::Error> {
        self.channel = Some(channel.id());
        Ok(true)
    }

    async fn pty_request(
        &mut self,
        channel: ChannelId,
        _term: &str,
        _col_width: u32,
        _row_height: u32,
        _pix_width: u32,
        _pix_height: u32,
        _modes: &[(Pty, u32)],
        session: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)?;
        Ok(())
    }

    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)?;
        // 发送 greeting
        let greeting = self.config.greeting.clone();
        session.data(channel, CryptoVec::from(greeting.as_bytes()))?;
        Ok(())
    }

    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        // Echo 回客户端
        session.data(channel, CryptoVec::from(data))?;
        Ok(())
    }

    async fn window_change_request(
        &mut self,
        _channel: ChannelId,
        col_width: u32,
        row_height: u32,
        _pix_width: u32,
        _pix_height: u32,
        _session: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        if let Ok(mut g) = self.config.resize_signal.lock() {
            *g = Some((col_width, row_height));
        }
        Ok(())
    }
}

impl Drop for MockHandler {
    fn drop(&mut self) {
        if self.channel.is_some() {
            if let Ok(mut g) = self.config.closed_signal.lock() {
                *g = true;
            }
        }
    }
}

// -----------------------------------------------------------------------------
// 测试辅助：启动 mock server
// -----------------------------------------------------------------------------

/// 启动一个 mock SSH server，返回 (port, resize_signal, closed_signal, join_handle)
fn spawn_mock_server(
    username: &str,
    password: &str,
    greeting: &str,
) -> (u16, ResizeSignal, ClosedSignal, std::thread::JoinHandle<()>) {
    let (mut server, resize_signal, closed_signal) = MockServer::new(username, password, greeting);
    // 在 tokio runtime 内绑定 listener，避免 `TcpListener::from_std` 在 current_thread
    // runtime 下报 "Registering a blocking socket" panic
    let (port_tx, port_rx) = std::sync::mpsc::channel();

    let handle = std::thread::Builder::new()
        .name("mock-ssh-server".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("build tokio runtime");
            rt.block_on(async move {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                    .await
                    .expect("bind mock server");
                let port = listener.local_addr().expect("local_addr").port();
                // 端口就绪后通知主线程，使其可以开始连接
                let _ = port_tx.send(port);

                let (socket, _) = listener.accept().await.expect("accept");

                let config = russh::server::Config {
                    inactivity_timeout: Some(Duration::from_secs(60)),
                    auth_rejection_time: Duration::from_secs(0),
                    auth_rejection_time_initial: Some(Duration::from_secs(0)),
                    keys: vec![russh::keys::PrivateKey::random(
                        &mut OsRng,
                        russh::keys::Algorithm::Ed25519,
                    )
                    .expect("generate ed25519 key")],
                    ..Default::default()
                };
                let config = Arc::new(config);

                let handler = server.new_client(socket.peer_addr().ok());
                // run_stream 返回 RunningSession，必须继续 await 才能维持 session
                // 否则 session 任务会被立即 cancel，导致连接 reset
                match russh::server::run_stream(config, socket, handler).await {
                    Ok(session) => {
                        // 阻塞直到 session 结束（客户端断开 / EOF）
                        if let Err(e) = session.await {
                            eprintln!("[mock-ssh-server] session 结束出错: {e}");
                        }
                    }
                    Err(e) => eprintln!("[mock-ssh-server] run_stream 启动失败: {e}"),
                }
            });
        })
        .expect("spawn server thread");

    let port = port_rx.recv().expect("recv port");
    (port, resize_signal, closed_signal, handle)
}

fn make_config(port: u16, username: &str, password: &str) -> SshConfig {
    SshConfig {
        host: "127.0.0.1".into(),
        port,
        username: username.into(),
        password: password.into(),
    }
}

/// drain 事件直到 predicate 返回 true，或超时 panic
fn drain_until<F>(bridge: &SshBridge, predicate: F, timeout_label: &str)
where
    F: Fn(&SshEvent) -> bool,
{
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            panic!("drain_until 超时（{timeout_label}）");
        }
        match bridge.event_rx.recv_timeout(remaining) {
            Ok(event) => {
                eprintln!("[test] event: {event:?}");
                if predicate(&event) {
                    return;
                }
                // 错误事件直接 panic
                if let SshEvent::Error(msg) = event {
                    panic!("收到 SshEvent::Error: {msg}");
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                panic!("recv_timeout 超时（{timeout_label}）");
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                panic!("event_rx disconnected（{timeout_label}）");
            }
        }
    }
}

// -----------------------------------------------------------------------------
// 集成测试用例
// -----------------------------------------------------------------------------

#[test]
fn test_ssh_bridge_connect_greeting_echo_shutdown() {
    let (port, _rs, _cs, server_thread) =
        spawn_mock_server("testuser", "testpass", "hello from mock ssh\r\n");
    let config = make_config(port, "testuser", "testpass");
    let bridge = SshBridge::connect(config, 80, 24);

    // 等待 Connected
    drain_until(
        &bridge,
        |e| matches!(e, SshEvent::Connected),
        "等待 SshEvent::Connected",
    );

    // 等待 greeting
    let mut received = Vec::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            panic!("等待 greeting 超时");
        }
        match bridge.event_rx.recv_timeout(remaining) {
            Ok(SshEvent::Data(bytes)) => {
                received.extend_from_slice(&bytes);
                if String::from_utf8_lossy(&received).contains("hello from mock ssh") {
                    break;
                }
            }
            Ok(other) => {
                eprintln!("[test] other event while waiting greeting: {other:?}");
            }
            Err(_) => panic!("recv 失败"),
        }
    }

    // 发送输入，等待 echo
    bridge
        .command_tx
        .try_send(SshCommand::Input(b"echo_test\r".to_vec()))
        .expect("send input");

    let mut echoed = Vec::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            panic!("等待 echo 超时");
        }
        match bridge.event_rx.recv_timeout(remaining) {
            Ok(SshEvent::Data(bytes)) => {
                echoed.extend_from_slice(&bytes);
                if String::from_utf8_lossy(&echoed).contains("echo_test") {
                    break;
                }
            }
            Ok(other) => {
                eprintln!("[test] other event while waiting echo: {other:?}");
            }
            Err(_) => panic!("recv 失败"),
        }
    }

    // 发送 Shutdown，等待 Closed
    bridge.send_shutdown();
    drain_until(
        &bridge,
        |e| matches!(e, SshEvent::Closed),
        "等待 SshEvent::Closed",
    );

    let _ = server_thread.join();
}

#[test]
fn test_ssh_bridge_wrong_password_returns_error() {
    let (port, _rs, _cs, server_thread) =
        spawn_mock_server("testuser", "correctpass", "greeting\r\n");
    let config = make_config(port, "testuser", "wrongpass");
    let bridge = SshBridge::connect(config, 80, 24);

    // 应该收到 Error 事件（认证失败）
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            panic!("等待 Error 超时（应该认证失败）");
        }
        match bridge.event_rx.recv_timeout(remaining) {
            Ok(SshEvent::Error(msg)) => {
                eprintln!("[test] got expected error: {msg}");
                assert!(
                    msg.contains("认证失败") || msg.contains("auth"),
                    "错误信息应提到认证失败，实际: {msg}"
                );
                break;
            }
            Ok(SshEvent::Status(_)) => continue,
            Ok(SshEvent::Connected) => panic!("错误：错误密码不应连接成功"),
            Ok(other) => {
                eprintln!("[test] unexpected event: {other:?}");
            }
            Err(_) => panic!("recv 失败"),
        }
    }

    let _ = server_thread.join();
}

#[test]
fn test_ssh_bridge_resize_command_reaches_server() {
    let (port, resize_signal, _cs, server_thread) =
        spawn_mock_server("resize_user", "resize_pass", "ready\r\n");
    let config = make_config(port, "resize_user", "resize_pass");
    let bridge = SshBridge::connect(config, 80, 24);

    drain_until(
        &bridge,
        |e| matches!(e, SshEvent::Connected),
        "等待 Connected",
    );

    // 等待 greeting
    drain_until(
        &bridge,
        |e| matches!(e, SshEvent::Data(_)),
        "等待 greeting Data",
    );

    // 发送 Resize
    bridge
        .command_tx
        .try_send(SshCommand::Resize(120, 40))
        .expect("send resize");

    // 等待服务端收到 window_change_request
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let now = std::time::Instant::now();
        if now >= deadline {
            panic!("等待 resize 信号超时");
        }
        if let Ok(g) = resize_signal.lock() {
            if let Some((w, h)) = *g {
                assert_eq!(w, 120, "resize 宽度应为 120");
                assert_eq!(h, 40, "resize 高度应为 40");
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    // 关闭
    bridge.send_shutdown();
    drain_until(&bridge, |e| matches!(e, SshEvent::Closed), "等待 Closed");

    let _ = server_thread.join();
}

#[test]
fn test_ssh_bridge_status_progression_before_connected() {
    // 验证连接前会收到多条 Status 事件（连接中、认证中、PTY、shell 等）
    let (port, _rs, _cs, server_thread) = spawn_mock_server("stat_user", "stat_pass", "ok\r\n");
    let config = make_config(port, "stat_user", "stat_pass");
    let bridge = SshBridge::connect(config, 80, 24);

    let mut status_count = 0;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            panic!("等待 Connected 超时");
        }
        match bridge.event_rx.recv_timeout(remaining) {
            Ok(SshEvent::Status(_)) => {
                status_count += 1;
            }
            Ok(SshEvent::Connected) => break,
            Ok(SshEvent::Error(msg)) => panic!("收到错误: {msg}"),
            Ok(other) => {
                eprintln!("[test] other event: {other:?}");
            }
            Err(_) => panic!("recv 失败"),
        }
    }
    assert!(
        status_count >= 3,
        "应至少收到 3 条 Status（连接中、认证中、PTY/shell），实际收到 {status_count}"
    );

    bridge.send_shutdown();
    let _ = server_thread.join();
}

#[test]
fn test_ssh_bridge_invalid_host_returns_error() {
    // 故意连接不存在的端口，应收到 Error 而非 panic
    let config = SshConfig {
        host: "127.0.0.1".into(),
        port: 1, // 1 号端口通常无服务
        username: "x".into(),
        password: "x".into(),
    };
    let bridge = SshBridge::connect(config, 80, 24);

    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            panic!("等待 Error 超时（应连接失败）");
        }
        match bridge.event_rx.recv_timeout(remaining) {
            Ok(SshEvent::Error(msg)) => {
                eprintln!("[test] got expected connection error: {msg}");
                break;
            }
            Ok(SshEvent::Status(_)) => continue,
            Ok(SshEvent::Connected) => panic!("错误：不应连接成功"),
            Ok(other) => {
                eprintln!("[test] unexpected event: {other:?}");
            }
            Err(_) => panic!("recv 失败"),
        }
    }
}
