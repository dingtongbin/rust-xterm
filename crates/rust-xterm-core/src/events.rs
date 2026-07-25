//! 事件系统（xterm.js 风格）
//!
//! 提供类似 xterm.js 的事件回调机制。
//! 宿主可以注册回调函数，在终端状态变化时收到通知。
//!
//! ## 事件列表
//!
//! | 事件 | 触发时机 | xterm.js 对应 |
//! |------|---------|--------------|
//! | `on_data` | 终端产生输出数据 | `onData` |
//! | `on_title_change` | 窗口标题变更 (OSC 0/2) | `onTitleChange` |
//! | `on_bell` | 终端响铃 (BEL) | `onBell` |
//! | `on_cursor_move` | 光标位置变更 | `onCursorMove` |
//! | `on_resize` | 终端尺寸变更 | `onResize` |
//! | `on_selection_change` | 选区变更 | `onSelectionChange` |

use crate::{CursorMeta, TerminalSize};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// 终端事件
#[derive(Debug, Clone)]
pub enum TerminalEvent {
    /// 终端产生输出数据（如响应 OSC 查询）
    Data(String),
    /// 窗口标题变更
    TitleChange(String),
    /// 图标名称变更
    IconNameChange(String),
    /// 终端响铃
    Bell,
    /// 光标位置变更
    CursorMove(CursorMeta),
    /// 终端尺寸变更
    Resize(TerminalSize),
    /// 选区变更
    SelectionChange,
    /// 剪贴板请求
    ClipboardRequest(String),
    /// 文本属性查询完成
    ColorRequest {
        /// 请求索引
        index: u32,
        /// 返回的 RGB 颜色
        color: (u8, u8, u8),
    },
}

/// 事件回调函数类型
pub type EventCallback = Arc<dyn Fn(&TerminalEvent) + Send + Sync>;

/// 事件订阅器
///
/// 类似 xterm.js 的 `terminal.onData(cb)` 返回的订阅器。
/// 当前实现为简化版：订阅在 EventBus 存活期间有效。
/// Drop 时通过 ID 标记移除（惰性清理）。
pub struct EventSubscription {
    id: usize,
    bus: Arc<EventBusInner>,
}

impl Drop for EventSubscription {
    fn drop(&mut self) {
        if let Ok(mut callbacks) = self.bus.callbacks.lock() {
            callbacks.retain(|(id, _)| *id != self.id);
        }
    }
}

/// 事件总线内部实现
struct EventBusInner {
    callbacks: Mutex<Vec<(usize, EventCallback)>>,
    next_id: AtomicUsize,
}

/// 事件总线
///
/// 管理事件回调的注册和派发。
/// 使用 `Arc` 允许多个所有者共享事件总线。
#[derive(Clone)]
pub struct EventBus {
    inner: Arc<EventBusInner>,
}

impl EventBus {
    /// 创建新的事件总线
    pub fn new() -> Self {
        Self {
            inner: Arc::new(EventBusInner {
                callbacks: Mutex::new(Vec::new()),
                next_id: AtomicUsize::new(0),
            }),
        }
    }

    /// 注册事件回调
    ///
    /// 返回 `EventSubscription`，drop 时自动取消订阅。
    pub fn on<F>(&self, callback: F) -> EventSubscription
    where
        F: Fn(&TerminalEvent) + Send + Sync + 'static,
    {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let cb: EventCallback = Arc::new(callback);
        self.inner.callbacks.lock().unwrap().push((id, cb));
        EventSubscription {
            id,
            bus: self.inner.clone(),
        }
    }

    /// 派发事件给所有订阅者
    pub fn emit(&self, event: &TerminalEvent) {
        let callbacks = self.inner.callbacks.lock().unwrap();
        for (_, cb) in callbacks.iter() {
            cb(event);
        }
    }

    /// 获取订阅者数量
    pub fn subscriber_count(&self) -> usize {
        self.inner.callbacks.lock().unwrap().len()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn test_event_dispatch() {
        let bus = EventBus::new();
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let _sub = bus.on(move |_event| {
            counter_clone.fetch_add(1, Ordering::Relaxed);
        });

        bus.emit(&TerminalEvent::Bell);
        bus.emit(&TerminalEvent::Bell);
        bus.emit(&TerminalEvent::Bell);

        assert_eq!(counter.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn test_multiple_subscribers() {
        let bus = EventBus::new();
        let counter = Arc::new(AtomicUsize::new(0));
        let mut _subs = Vec::new();

        for _ in 0..3 {
            let c = counter.clone();
            let sub = bus.on(move |_| {
                c.fetch_add(1, Ordering::Relaxed);
            });
            _subs.push(sub);
        }

        bus.emit(&TerminalEvent::Bell);
        assert_eq!(counter.load(Ordering::Relaxed), 3);
        assert_eq!(bus.subscriber_count(), 3);
    }

    #[test]
    fn test_title_change_event() {
        let bus = EventBus::new();
        let received = Arc::new(std::sync::Mutex::new(None::<String>));
        let received_clone = received.clone();

        let _sub = bus.on(move |event| {
            if let TerminalEvent::TitleChange(title) = event {
                *received_clone.lock().unwrap() = Some(title.clone());
            }
        });

        bus.emit(&TerminalEvent::TitleChange("My Title".to_string()));

        assert_eq!(*received.lock().unwrap(), Some("My Title".to_string()));
    }

    #[test]
    fn test_unsubscribe_on_drop() {
        let bus = EventBus::new();
        let counter = Arc::new(AtomicUsize::new(0));
        let c = counter.clone();

        {
            let _sub = bus.on(move |_| {
                c.fetch_add(1, Ordering::Relaxed);
            });
            assert_eq!(bus.subscriber_count(), 1);
        } // _sub dropped here

        assert_eq!(bus.subscriber_count(), 0);
        bus.emit(&TerminalEvent::Bell);
        assert_eq!(counter.load(Ordering::Relaxed), 0);
    }
}
