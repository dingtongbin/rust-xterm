//! 主状态：用 Rc<RefCell<...>> 让 Slint 回调闭包可共享可变状态
use crate::fps::FpsTracker;
use rust_xterm_core::mouse::KeyMods;
use rust_xterm_host::EventLoop;
use rust_xterm_renderer::Renderer;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// 剪贴板：arboard 实例需在主线程，用 Arc<Mutex> 共享
pub(crate) type ClipboardHandle = Arc<Mutex<arboard::Clipboard>>;

pub(crate) struct AppCtx {
    pub(crate) event_loop: EventLoop,
    pub(crate) renderer: Renderer,
    pub(crate) fps_tracker: FpsTracker,
    pub(crate) sys: sysinfo::System,
    /// 当前进程 PID，用于 refresh_process 读取自身 RSS
    pub(crate) pid: sysinfo::Pid,
    pub(crate) last_mem_refresh: Instant,
    pub(crate) last_mem_mb: f64,
    pub(crate) scroll_offset: usize,
    pub(crate) pty_alive: bool,
    /// 当前键盘修饰键状态（由 key-pressed 回调维护，供 mouse 回调读取）
    pub(crate) current_mods: KeyMods,
}

impl AppCtx {
    pub(crate) fn new(
        event_loop: EventLoop,
        renderer: Renderer,
        pty_alive: bool,
        pid: sysinfo::Pid,
    ) -> Self {
        Self {
            event_loop,
            renderer,
            fps_tracker: FpsTracker::new(),
            sys: sysinfo::System::new_all(),
            pid,
            last_mem_refresh: Instant::now(),
            last_mem_mb: 0.0,
            scroll_offset: 0,
            pty_alive,
            current_mods: KeyMods::default(),
        }
    }
}
