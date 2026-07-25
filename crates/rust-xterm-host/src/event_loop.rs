//! 事件循环骨架
//!
//! 提供 PTY 数据拉取 + TerminalManager 更新 + 帧轮询的循环骨架。
//! 可适配 Slint / winit / 等任意 GUI 后端。
//!
//! ## 使用方式
//!
//! ```rust,ignore
//! let mut event_loop = EventLoop::new(manager, pty_bridge, EventLoopConfig::default());
//!
//! // 在 GUI 定时器中调用
//! loop {
//!     if let Some(event) = event_loop.tick() {
//!         match event {
//!             Event::FrameUpdate(frame) => {
//!             }
//!             Event::Closed => break,
//!         }
//!     }
//! }
//! ```

use crate::pty::PtyBridge;
use rust_xterm_core::{FrameUpdate, TerminalManager, TerminalSize};
use std::time::{Duration, Instant};

/// 事件循环配置
#[derive(Debug, Clone)]
pub struct EventLoopConfig {
    /// 轮询间隔
    pub poll_interval: Duration,
    /// 最大帧率（FPS）
    pub max_fps: u32,
}

impl Default for EventLoopConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_millis(16), // ~60fps
            max_fps: 60,
        }
    }
}

/// 事件循环产生的事件
#[derive(Debug)]
pub enum Event {
    /// 帧更新（有脏区需要重绘）
    FrameUpdate(FrameUpdate),
    /// PTY 已关闭
    Closed,
}

/// 事件循环
///
/// 封装 PTY 数据拉取 + TerminalManager 更新 + 帧轮询逻辑。
pub struct EventLoop {
    /// 终端管理器
    manager: TerminalManager,
    /// PTY 桥接
    pty: Option<PtyBridge>,
    /// 配置
    config: EventLoopConfig,
    /// 上次帧时间
    last_frame: Instant,
}

impl EventLoop {
    /// 创建新的事件循环
    pub fn new(manager: TerminalManager, pty: Option<PtyBridge>, config: EventLoopConfig) -> Self {
        Self {
            manager,
            pty,
            config,
            last_frame: Instant::now(),
        }
    }

    /// 获取终端管理器的可变引用
    pub fn manager(&mut self) -> &mut TerminalManager {
        &mut self.manager
    }

    /// 获取终端管理器的不可变引用
    ///
    /// 适用于只读查询（如 `size`、`cursor`、`snapshot_scrolled`、
    /// `is_mouse_grabbed`、`max_scrollback`、`title` 等），
    /// 无需获取 `borrow_mut`，避免在 RefCell 场景下产生不必要的可变借用。
    pub fn manager_ref(&self) -> &TerminalManager {
        &self.manager
    }

    /// 执行一次事件循环 tick
    ///
    /// 应在 GUI 定时器中以 `config.poll_interval` 间隔调用。
    pub fn tick(&mut self) -> Option<Event> {
        let now = Instant::now();

        // 1. 拉取 PTY 数据
        if let Some(pty) = &self.pty {
            if !pty.is_alive() {
                return Some(Event::Closed);
            }
        }

        if let Some(pty) = &self.pty {
            let chunks = pty.drain();
            for chunk in chunks {
                self.manager.write(&chunk);
            }
        }

        // 1b. 将终端产生的响应（鼠标报告、CSI 6n 光标查询、OSC 颜色查询等）
        //     从捕获缓冲取出并回传给 PTY，闭环终端 ↔ 子进程的交互。
        //     详见 `CapturingWriter` / `WezTermCore::drain_output`。
        let output = self.manager.drain_output();
        if !output.is_empty() {
            if let Some(pty) = &mut self.pty {
                let _ = pty.write_input(&output);
            }
        }

        // 2. 帧率限制
        let elapsed = now.duration_since(self.last_frame);
        if elapsed < self.config.poll_interval {
            return None;
        }

        // 3. 轮询帧
        let frame = self.manager.poll_frame(now);
        self.last_frame = now;

        frame.map(Event::FrameUpdate)
    }

    /// 向 PTY 发送用户输入
    pub fn send_input(&mut self, data: &[u8]) -> std::io::Result<()> {
        if let Some(pty) = &mut self.pty {
            pty.write_input(data)
        } else {
            Ok(())
        }
    }

    /// 调整终端大小
    pub fn resize(&mut self, rows: usize, cols: usize) {
        self.manager.resize(TerminalSize::new(rows, cols));

        if let Some(pty) = &self.pty {
            let _ = pty.resize(rows as u16, cols as u16);
        }
    }
}
