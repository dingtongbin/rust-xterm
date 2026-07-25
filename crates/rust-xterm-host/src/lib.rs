//! # rust-xterm-host: 宿主集成层
//!
//! 提供 PTY 桥接与宿主集成示例。
//!
//! ## 职责
//!
//! - 使用 `portable-pty` 管理 PTY 子进程
//! - 将 PTY 输出桥接到 `TerminalManager`
//! - 将用户输入桥接到 PTY
//! - 提供事件循环骨架（可适配 Slint / winit / 等）
//!
//! ## 设计原则
//!
//! - **核心与宿主解耦**：`TerminalManager` 不直接依赖 PTY
//! - **线程安全**：PTY 读取在独立线程，通过 channel 传递数据
//! - **零拷贝**：PTY 数据直接传递给 `TerminalManager::write`

#![forbid(unsafe_code)]
#![warn(missing_docs)]

// ============================================================================
// 模块声明
// ============================================================================

/// PTY 桥接
pub mod pty;

/// 事件循环骨架
pub mod event_loop;

// ============================================================================
// 公共重导出
// ============================================================================

pub use event_loop::{Event, EventLoop, EventLoopConfig};
pub use pty::{PtyBridge, PtyConfig, PtyError};
