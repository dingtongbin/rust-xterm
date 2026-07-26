//! PTY 桥接
//!
//! 使用 `portable-pty` 管理 PTY 子进程，
//! 将 PTY 输出桥接到 `TerminalManager`，
//! 将用户输入桥接到 PTY。
//!
//! ## 架构
//!
//! ```text
//! ┌──────────────┐     write(bytes)     ┌──────────────────┐
//! │  PTY Reader  │ ──────────────────→  │ TerminalManager  │
//! │  (线程)      │                      │                  │
//! └──────────────┘                      └──────────────────┘
//!        ↑                                       │
//!        │  write(bytes)                         │  poll_frame()
//!        │                                       ↓
//! ┌──────┴───────┐                      ┌──────────────────┐
//! │  PTY Writer  │ ←──────────────────  │   Event Loop     │
//! │              │   user input         │                  │
//! └──────────────┘                      └──────────────────┘
//! ```

use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::io::{self, Read, Write};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

/// PTY 配置
#[derive(Debug, Clone)]
pub struct PtyConfig {
    /// Shell 命令
    pub shell: String,
    /// 终端列数
    pub cols: u16,
    /// 终端行数
    pub rows: u16,
    /// 工作目录
    pub cwd: Option<String>,
}

impl Default for PtyConfig {
    fn default() -> Self {
        Self {
            shell: Self::detect_default_shell(),
            cols: 80,
            rows: 24,
            cwd: None,
        }
    }
}

impl PtyConfig {
    /// 自动检测当前操作系统上可用的 shell
    ///
    /// 检测顺序：
    /// - **Unix (Linux)**：优先采纳 `SHELL` 环境变量（仅当指向的可执行文件存在），
    ///   否则按 `/bin/zsh` → `/usr/bin/zsh` → `/bin/bash` → `/usr/bin/bash`
    ///   → `/bin/sh` → `/usr/bin/sh` 顺序探测文件存在性，返回第一个命中。
    /// - **Unix (macOS)**：优先采纳 `SHELL` 环境变量，否则按 `/bin/zsh`
    ///   → `/usr/local/bin/zsh` → `/opt/homebrew/bin/zsh` → `/bin/bash`
    ///   → `/usr/local/bin/bash` → `/bin/sh` 顺序探测。
    /// - **Windows**：依次用 `where` 探测 `pwsh.exe`（PowerShell 7+）/
    ///   `powershell.exe`（Windows PowerShell）/ `cmd.exe`，返回第一个命中。
    ///
    /// 兜底：Unix 回退到 `/bin/sh`，Windows 回退到 `cmd.exe`。
    ///
    /// 本方法不引入任何新依赖，仅使用 `std::path::Path::exists`（Unix）
    /// 与 `std::process::Command::new("where")`（Windows）。
    pub fn detect_default_shell() -> String {
        #[cfg(unix)]
        {
            use std::path::Path;

            // 1. 优先采纳 SHELL 环境变量（仅当指向的可执行文件确实存在，
            //    避免 $SHELL 指向已卸载的 shell 时启动失败）
            if let Ok(shell) = std::env::var("SHELL") {
                if !shell.is_empty() && Path::new(&shell).exists() {
                    return shell;
                }
            }

            // 2. 按候选路径探测
            let candidates: &[&str] = if cfg!(target_os = "macos") {
                &[
                    "/bin/zsh",
                    "/usr/local/bin/zsh",
                    "/opt/homebrew/bin/zsh",
                    "/bin/bash",
                    "/usr/local/bin/bash",
                    "/bin/sh",
                ]
            } else {
                &[
                    "/bin/zsh",
                    "/usr/bin/zsh",
                    "/bin/bash",
                    "/usr/bin/bash",
                    "/bin/sh",
                    "/usr/bin/sh",
                ]
            };
            for c in candidates {
                if Path::new(c).exists() {
                    return (*c).to_string();
                }
            }

            // 3. 兜底
            "/bin/sh".to_string()
        }
        #[cfg(windows)]
        {
            // Windows: 依次用 where 探测 pwsh.exe / powershell.exe / cmd.exe
            for c in &["pwsh.exe", "powershell.exe", "cmd.exe"] {
                if let Ok(out) = std::process::Command::new("where").arg(c).output() {
                    if out.status.success() {
                        let s = String::from_utf8_lossy(&out.stdout);
                        if let Some(line) = s.lines().next() {
                            let trimmed = line.trim();
                            if !trimmed.is_empty() {
                                return trimmed.to_string();
                            }
                        }
                    }
                }
            }
            // 兜底
            "cmd.exe".to_string()
        }
        #[cfg(not(any(unix, windows)))]
        {
            "/bin/sh".to_string()
        }
    }
}

/// PTY 错误类型
#[derive(Debug)]
pub enum PtyError {
    /// PTY 创建失败
    CreateFailed(String),
    /// 子进程启动失败
    SpawnFailed(String),
    /// IO 错误
    Io(io::Error),
}

impl std::fmt::Display for PtyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PtyError::CreateFailed(msg) => write!(f, "PTY creation failed: {msg}"),
            PtyError::SpawnFailed(msg) => write!(f, "Spawn failed: {msg}"),
            PtyError::Io(e) => write!(f, "IO error: {e}"),
        }
    }
}

impl std::error::Error for PtyError {}

impl From<io::Error> for PtyError {
    fn from(e: io::Error) -> Self {
        PtyError::Io(e)
    }
}

/// PTY 桥接器
///
/// 管理 PTY 子进程，在独立线程中读取 PTY 输出，
/// 通过 channel 传递给主线程的 `TerminalManager`。
pub struct PtyBridge {
    /// PTY master 端
    master: Box<dyn MasterPty + Send>,
    /// PTY writer（用于向子进程发送输入）
    writer: Box<dyn Write + Send>,
    /// 数据接收通道
    rx: Receiver<Vec<u8>>,
    /// 读取线程句柄
    reader_thread: Option<thread::JoinHandle<()>>,
}

impl PtyBridge {
    /// 创建新的 PTY 桥接
    pub fn new(config: &PtyConfig) -> Result<Self, PtyError> {
        let pty_system = native_pty_system();

        let pty_pair = pty_system
            .openpty(PtySize {
                rows: config.rows,
                cols: config.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| PtyError::CreateFailed(e.to_string()))?;

        let mut cmd = CommandBuilder::new(&config.shell);
        if let Some(cwd) = &config.cwd {
            cmd.cwd(cwd);
        }

        let _child = pty_pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| PtyError::SpawnFailed(e.to_string()))?;

        let writer = pty_pair
            .master
            .take_writer()
            .map_err(|e| PtyError::CreateFailed(e.to_string()))?;

        let mut reader = pty_pair
            .master
            .try_clone_reader()
            .map_err(|e| PtyError::CreateFailed(e.to_string()))?;

        let (tx, rx): (Sender<Vec<u8>>, Receiver<Vec<u8>>) = mpsc::channel();

        let reader_thread = thread::spawn(move || {
            let mut buf = vec![0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            master: pty_pair.master,
            writer,
            rx,
            reader_thread: Some(reader_thread),
        })
    }

    /// 拉取 PTY 输出数据
    ///
    /// 返回自上次调用以来收到的所有数据块。
    pub fn drain(&self) -> Vec<Vec<u8>> {
        let mut chunks = Vec::new();
        while let Ok(data) = self.rx.try_recv() {
            chunks.push(data);
        }
        chunks
    }

    /// 向 PTY 发送数据
    pub fn write_input(&mut self, data: &[u8]) -> io::Result<()> {
        self.writer.write_all(data)?;
        self.writer.flush()
    }

    /// 调整 PTY 大小
    pub fn resize(&self, rows: u16, cols: u16) -> Result<(), PtyError> {
        self.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| PtyError::Io(io::Error::other(e.to_string())))
    }

    /// 检查读取线程是否存活
    pub fn is_alive(&self) -> bool {
        self.reader_thread
            .as_ref()
            .map(|t| !t.is_finished())
            .unwrap_or(false)
    }
}

impl Drop for PtyBridge {
    fn drop(&mut self) {
        // 关闭 writer 以触发子进程退出
        let _ = self.writer.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_default_shell_returns_nonempty() {
        let shell = PtyConfig::detect_default_shell();
        assert!(!shell.is_empty(), "detect_default_shell 不应返回空字符串");
    }

    #[test]
    fn test_detect_default_shell_exists_on_unix() {
        // Unix 平台：检测到的 shell 路径应确实存在（除非是兜底值）。
        #[cfg(unix)]
        {
            let shell = PtyConfig::detect_default_shell();
            let exists = std::path::Path::new(&shell).exists();
            assert!(
                exists || shell == "/bin/sh",
                "shell '{shell}' 既不存在也不是兜底值 /bin/sh"
            );
        }
        #[cfg(not(unix))]
        {
            // 非 Unix 平台：仅断言非空
            let _ = PtyConfig::detect_default_shell();
        }
    }

    #[test]
    fn test_default_config_uses_detected_shell() {
        let cfg = PtyConfig::default();
        assert!(!cfg.shell.is_empty());
        assert_eq!(cfg.cols, 80);
        assert_eq!(cfg.rows, 24);
        assert!(cfg.cwd.is_none());
    }
}
