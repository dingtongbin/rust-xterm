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
    /// 上一次光标可见性（用于检测闪烁/移动，触发像素上传）
    pub(crate) last_cursor_visible: Option<bool>,
    /// 状态栏文本 dirty 检查缓存
    pub(crate) last_fps_text: Option<String>,
    pub(crate) last_mem_text: Option<String>,
    pub(crate) last_scroll_text: Option<String>,
    /// 上一次检测到的窗口物理像素尺寸（用于实时 resize 同步）
    pub(crate) last_window_size: (u32, u32),
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
            sys: sysinfo::System::new(),
            pid,
            last_mem_refresh: Instant::now(),
            last_mem_mb: 0.0,
            scroll_offset: 0,
            pty_alive,
            current_mods: KeyMods::default(),
            last_cursor_visible: None,
            last_fps_text: None,
            last_mem_text: None,
            last_scroll_text: None,
            last_window_size: (0, 0),
        }
    }
}
