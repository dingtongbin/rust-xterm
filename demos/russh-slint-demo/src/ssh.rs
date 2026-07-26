//! SSH 桥接：用 russh 客户端连接远程服务器，把 channel 数据双向桥接到 TerminalManager
//!
//! ## 架构
//!
//! ```text
//! ┌────────────────────┐   SshEvent::Data    ┌──────────────────┐
//! │  SSH channel loop  │ ─────────────────→ │ TerminalManager  │
//! │  (tokio runtime    │                    │   (主线程)       │
//! │   独立线程)        │                    │                  │
//! └────────────────────┘                    └──────────────────┘
//!        ↑                                          │
//!        │  SshCommand::Input                       │  drain_output()
//!        │  (tokio mpsc::Sender)                    ↓
//! ┌──────┴───────────┐                      ┌──────────────────┐
//! │  command_rx     │ ←──────────────────  │   render tick    │
//! │  (async recv)   │   try_send            │   (主线程)       │
//! └──────────────────┘                      └──────────────────┘
//! ```
//!
//! - SSH → 主线程：`std::sync::mpsc`，主线程 `try_recv` 非阻塞读
//! - 主线程 → SSH：`tokio::sync::mpsc`，主线程 `try_send` 非阻塞写，SSH 端 `recv().await`
//! - SSH 线程：tokio runtime 独立线程，不阻塞 Slint 主线程

use crate::config::SshConfig;
use anyhow::Result;
use russh::client::{self, Handler};
use russh::{Channel, ChannelMsg, Disconnect};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use tokio::sync::mpsc as async_mpsc;

/// SSH 线程 → 主线程 的事件
#[derive(Debug)]
pub enum SshEvent {
    /// 进度上报（用于遮罩显示）
    Status(String),
    /// 连接成功（撤销遮罩，切换到终端视图）
    Connected,
    /// 收到 channel 数据
    Data(Vec<u8>),
    /// 连接失败（遮罩显示错误）
    Error(String),
    /// channel 已关闭（PTY EOF / 远端关闭）
    Closed,
}

/// 主线程 → SSH 线程 的命令
#[derive(Debug)]
pub enum SshCommand {
    /// 用户输入（键盘输入 / 粘贴）
    Input(Vec<u8>),
    /// 终端 resize（cols, rows）
    Resize(u16, u16),
    /// 关闭 SSH 连接
    Shutdown,
}

/// SSH 桥接器：主线程持有
///
/// 通过 `event_rx`（同步 mpsc）读 SSH 事件；
/// 通过 `command_tx`（tokio mpsc，可 try_send）发用户命令。
pub struct SshBridge {
    /// SSH 线程 → 主线程 事件接收端（同步 mpsc，主线程 try_recv）
    pub event_rx: Receiver<SshEvent>,
    /// 主线程 → SSH 线程 命令发送端（tokio mpsc，主线程 try_send）
    pub command_tx: async_mpsc::Sender<SshCommand>,
    /// SSH 线程句柄（drop 时 detach，线程随 channel 关闭自然退出）
    _join_handle: Option<JoinHandle<()>>,
}

impl SshBridge {
    /// 启动 SSH 连接（异步，立即返回）
    ///
    /// 内部 spawn 一个 std::thread，在线程内创建 tokio runtime 跑 SSH 连接流程。
    /// 调用方应通过 `event_rx` 监听进度/数据事件。
    pub fn connect(config: SshConfig, cols: u16, rows: u16) -> Self {
        let (event_tx, event_rx) = mpsc::channel::<SshEvent>();
        let (command_tx, command_rx) = async_mpsc::channel::<SshCommand>(64);

        let event_tx_clone = event_tx.clone();
        let handle = std::thread::Builder::new()
            .name("russh-ssh".into())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        let _ = event_tx_clone
                            .send(SshEvent::Error(format!("tokio runtime 启动失败: {e}")));
                        return;
                    }
                };
                rt.block_on(async move {
                    let err_tx = event_tx.clone();
                    if let Err(e) = run_ssh(config, cols, rows, event_tx, command_rx).await {
                        eprintln!("[russh-slint-demo] SSH 错误: {e:#}");
                        let _ = err_tx.send(SshEvent::Error(format!("{e:#}")));
                    }
                });
            })
            .expect("spawn ssh thread");

        Self {
            event_rx,
            command_tx,
            _join_handle: Some(handle),
        }
    }

    /// 发送用户输入到 SSH channel（非阻塞）
    ///
    /// 失败时（缓冲满）返回 Err；调用方通常忽略。
    pub fn send_input(
        &self,
        data: Vec<u8>,
    ) -> Result<(), tokio::sync::mpsc::error::TrySendError<SshCommand>> {
        self.command_tx.try_send(SshCommand::Input(data))
    }

    /// 请求 resize（非阻塞）
    pub fn send_resize(
        &self,
        cols: u16,
        rows: u16,
    ) -> Result<(), tokio::sync::mpsc::error::TrySendError<SshCommand>> {
        self.command_tx.try_send(SshCommand::Resize(cols, rows))
    }

    /// 请求关闭（非阻塞）
    pub fn send_shutdown(&self) {
        let _ = self.command_tx.try_send(SshCommand::Shutdown);
    }
}

/// russh 客户端 handler：demo 用，接受任何服务器公钥（生产环境应验证指纹）
struct DemoHandler;

impl Handler for DemoHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        // Demo 用：接受任何服务器公钥（避免 known_hosts 复杂度）
        // 生产环境应对比 known_hosts / 让用户确认指纹
        Ok(true)
    }
}

/// SSH 连接 + 数据循环
async fn run_ssh(
    config: SshConfig,
    cols: u16,
    rows: u16,
    event_tx: Sender<SshEvent>,
    mut command_rx: async_mpsc::Receiver<SshCommand>,
) -> Result<()> {
    // 1. 连接 TCP
    let _ = event_tx.send(SshEvent::Status(format!(
        "正在连接到 {}:{}...",
        config.host, config.port
    )));
    let ssh_config = Arc::new(client::Config::default());
    let addr = format!("{}:{}", config.host, config.port);
    let mut session = client::connect(ssh_config, &*addr, DemoHandler).await?;

    // 2. 密码认证
    let _ = event_tx.send(SshEvent::Status("正在认证（密码）...".into()));
    let auth_res = session
        .authenticate_password(&config.username, &config.password)
        .await?;
    if !auth_res.success() {
        anyhow::bail!("密码认证失败（用户名或密码错误）");
    }

    // 3. 打开 channel
    let _ = event_tx.send(SshEvent::Status("正在打开 channel...".into()));
    let mut channel = session.channel_open_session().await?;

    // 4. 请求 PTY
    let _ = event_tx.send(SshEvent::Status("正在请求 PTY...".into()));
    channel
        .request_pty(false, "xterm", cols as u32, rows as u32, 0, 0, &[])
        .await?;

    // 5. 请求 shell
    let _ = event_tx.send(SshEvent::Status("正在请求 shell...".into()));
    channel.request_shell(true).await?;

    // 6. 通知主线程：连接成功
    let _ = event_tx.send(SshEvent::Connected);

    // 7. 双向数据流循环
    let result = run_data_loop(&mut channel, &event_tx, &mut command_rx).await;

    // 8. 关闭 channel
    let _ = channel.close().await;
    let _ = event_tx.send(SshEvent::Closed);

    // 9. 断开 SSH session
    let _ = session
        .disconnect(Disconnect::ByApplication, "", "en")
        .await;

    result
}

/// 双向数据流循环：
/// - 从 SSH channel 读数据 → 发给主线程
/// - 从主线程读命令 → 发给 SSH channel
async fn run_data_loop(
    channel: &mut Channel<client::Msg>,
    event_tx: &Sender<SshEvent>,
    command_rx: &mut async_mpsc::Receiver<SshCommand>,
) -> Result<()> {
    loop {
        tokio::select! {
            // SSH channel → 主线程
            msg = channel.wait() => {
                match msg {
                    Some(ChannelMsg::Data { data }) => {
                        let bytes: Vec<u8> = data.to_vec();
                        let _ = event_tx.send(SshEvent::Data(bytes));
                    }
                    Some(ChannelMsg::ExtendedData { data, .. }) => {
                        let bytes: Vec<u8> = data.to_vec();
                        let _ = event_tx.send(SshEvent::Data(bytes));
                    }
                    Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => {
                        eprintln!("[russh-slint-demo] channel EOF/Close");
                        return Ok(());
                    }
                    _ => {
                        // 其他消息（ExitStatus、WindowAdjusted 等）忽略
                    }
                }
            }
            // 主线程 → SSH channel
            cmd = command_rx.recv() => {
                match cmd {
                    Some(SshCommand::Input(data)) => {
                        if channel.data(&data[..]).await.is_err() {
                            eprintln!("[russh-slint-demo] channel.data 失败");
                            return Ok(());
                        }
                    }
                    Some(SshCommand::Resize(w, h)) => {
                        let _ = channel.window_change(w as u32, h as u32, 0, 0).await;
                    }
                    Some(SshCommand::Shutdown) | None => {
                        eprintln!("[russh-slint-demo] 收到 Shutdown，退出数据循环");
                        return Ok(());
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ssh_event_status_variant() {
        let e = SshEvent::Status("test".into());
        assert!(matches!(e, SshEvent::Status(_)));
    }

    #[test]
    fn test_ssh_command_input_variant() {
        let c = SshCommand::Input(vec![1, 2, 3]);
        assert!(matches!(c, SshCommand::Input(_)));
    }

    #[test]
    fn test_ssh_event_channels_work() {
        // 不实际连接，仅验证 std::sync::mpsc 通道可用（SSH → 主线程方向）
        let (event_tx, event_rx) = mpsc::channel::<SshEvent>();
        event_tx.send(SshEvent::Status("hi".into())).unwrap();
        match event_rx.recv() {
            Ok(SshEvent::Status(s)) => assert_eq!(s, "hi"),
            _ => panic!("expect Status"),
        }
    }

    #[test]
    fn test_ssh_command_channels_async() {
        // 验证 tokio::sync::mpsc 通道可用（主线程 → SSH 方向）
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let (command_tx, mut command_rx) = async_mpsc::channel::<SshCommand>(8);
            command_tx.send(SshCommand::Resize(80, 24)).await.unwrap();
            let cmd = command_rx.recv().await;
            match cmd {
                Some(SshCommand::Resize(w, h)) => {
                    assert_eq!(w, 80);
                    assert_eq!(h, 24);
                }
                _ => panic!("expect Resize"),
            }
        });
    }

    #[test]
    fn test_demo_handler_can_construct() {
        // DemoHandler 应可构造（编译期 trait 检查）
        let _ = DemoHandler;
    }
}
