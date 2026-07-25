//! 运行时状态
//!
//! 记录终端运行时的非持久化状态，如光标闪烁计时、
//! 上次渲染时间戳等。

use std::time::{Duration, Instant};

/// 光标闪烁间隔（毫秒）
const CURSOR_BLINK_INTERVAL_MS: u64 = 500;

/// 运行时状态
#[derive(Debug)]
pub struct RuntimeState {
    /// 上次渲染时间
    last_render: Option<Instant>,
    /// 上次光标闪烁切换时间
    last_blink: Instant,
    /// 光标当前是否可见（闪烁相位）
    cursor_blink_visible: bool,
    /// 是否启用了光标闪烁
    cursor_blinking: bool,
    /// 上次已渲染的 seqno
    last_rendered_seqno: u64,
}

impl RuntimeState {
    /// 创建新的运行时状态
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            last_render: None,
            last_blink: now,
            cursor_blink_visible: true,
            cursor_blinking: false,
            last_rendered_seqno: 0,
        }
    }

    /// 设置光标闪烁开关
    pub fn set_cursor_blinking(&mut self, enabled: bool) {
        self.cursor_blinking = enabled;
        if !enabled {
            self.cursor_blink_visible = true;
        }
    }

    /// 检查光标闪烁是否到期
    pub fn blink_due(&self, now: Instant) -> bool {
        if !self.cursor_blinking {
            return false;
        }
        now.duration_since(self.last_blink) >= Duration::from_millis(CURSOR_BLINK_INTERVAL_MS)
    }

    /// 推进光标闪烁相位
    pub fn advance_blink(&mut self, now: Instant) {
        if self.blink_due(now) {
            self.cursor_blink_visible = !self.cursor_blink_visible;
            self.last_blink = now;
        }
    }

    /// 获取光标当前可见性（考虑闪烁）
    pub fn cursor_visible(&self) -> bool {
        self.cursor_blink_visible
    }

    /// 记录渲染时间
    pub fn mark_rendered(&mut self, now: Instant, seqno: u64) {
        self.last_render = Some(now);
        self.last_rendered_seqno = seqno;
    }

    /// 获取上次渲染的 seqno
    pub fn last_rendered_seqno(&self) -> u64 {
        self.last_rendered_seqno
    }

    /// 获取上次渲染时间
    pub fn last_render(&self) -> Option<Instant> {
        self.last_render
    }
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self::new()
    }
}
