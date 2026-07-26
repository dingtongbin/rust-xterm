//! 主状态：用 Rc<RefCell<...>> 让 Slint 回调闭包可共享可变状态
//!
//! 与 slint-demo 不同之处：本 demo 不使用 `rust-xterm-host::EventLoop`
//! （因为数据源是 SSH channel，不是本地 PTY）。所有 PTY↔Terminal 之间的
//! 桥接由本 demo 自己实现：tick 中 drain `SshBridge.event_rx` 并调用
//! `manager.write(...)`，drain `manager.drain_output()` 后用 `SshBridge.send_input` 回环。

use crate::fps::FpsTracker;
use crate::ssh::SshBridge;
use rust_xterm_core::mouse::KeyMods;
use rust_xterm_core::TerminalManager;
use rust_xterm_renderer::Renderer;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// 剪贴板：arboard 实例需在主线程，用 Arc<Mutex> 共享
pub(crate) type ClipboardHandle = Arc<Mutex<arboard::Clipboard>>;

pub(crate) struct AppCtx {
    /// 终端管理器（本地 Grid 状态）
    pub(crate) manager: TerminalManager,
    /// 渲染器
    pub(crate) renderer: Renderer,
    /// FPS 滑动平均
    pub(crate) fps_tracker: FpsTracker,
    /// 进程信息
    pub(crate) sys: sysinfo::System,
    /// 当前进程 PID
    pub(crate) pid: sysinfo::Pid,
    pub(crate) last_mem_refresh: Instant,
    pub(crate) last_mem_mb: f64,
    /// scrollback 滚动偏移（0 = 实时可视窗口）
    pub(crate) scroll_offset: usize,
    /// SSH channel 是否仍然存活（EOF/Close 后置 false）
    pub(crate) channel_alive: bool,
    /// 是否已经收到 `SshEvent::Connected`，决定是否开始 poll_frame
    pub(crate) connected: bool,
    /// 当前键盘修饰键状态（由 key-pressed 回调维护，供 mouse 回调读取）
    pub(crate) current_mods: KeyMods,
    /// SSH 桥接器
    pub(crate) bridge: Option<SshBridge>,
}

impl AppCtx {
    pub(crate) fn new(manager: TerminalManager, renderer: Renderer, pid: sysinfo::Pid) -> Self {
        Self {
            manager,
            renderer,
            fps_tracker: FpsTracker::new(),
            sys: sysinfo::System::new_all(),
            pid,
            last_mem_refresh: Instant::now(),
            last_mem_mb: 0.0,
            scroll_offset: 0,
            channel_alive: false,
            connected: false,
            current_mods: KeyMods::default(),
            bridge: None,
        }
    }
}
