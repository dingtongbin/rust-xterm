//! 自定义序列解析器（xterm.js 风格）
//!
//! 类似 xterm.js 的 `IParser` 接口，
//! 允许宿主注册自定义 ANSI 序列处理器。
//!
//! ## 使用场景
//!
//! - 处理自定义 OSC 序列
//! - 拦截特定 CSI 序列
//! - 实现 DCS（Device Control String）回调

use std::collections::HashMap;

/// 序列处理器回调
pub type SequenceHandler = Box<dyn Fn(&[u8]) + Send + Sync>;

/// 自定义序列解析器
///
/// 允许宿主注册自定义 ANSI 序列处理器。
pub struct Parser {
    /// CSI 序列处理器（按 final byte 索引）
    csi_handlers: HashMap<u8, SequenceHandler>,
    /// OSC 序列处理器（按 OSC 编号索引）
    osc_handlers: HashMap<u32, SequenceHandler>,
    /// DCS 序列处理器
    dcs_handlers: HashMap<u8, SequenceHandler>,
}

impl Parser {
    /// 创建新的解析器
    pub fn new() -> Self {
        Self {
            csi_handlers: HashMap::new(),
            osc_handlers: HashMap::new(),
            dcs_handlers: HashMap::new(),
        }
    }

    /// 注册 CSI 序列处理器
    ///
    /// - `final_byte`: CSI 序列的最终字节（如 'H' = 0x48）
    /// - `handler`: 回调函数，接收序列参数
    pub fn register_csi<F>(&mut self, final_byte: u8, handler: F)
    where
        F: Fn(&[u8]) + Send + Sync + 'static,
    {
        self.csi_handlers.insert(final_byte, Box::new(handler));
    }

    /// 注册 OSC 序列处理器
    ///
    /// - `code`: OSC 编号（如 8 = 超链接）
    /// - `handler`: 回调函数，接收序列数据
    pub fn register_osc<F>(&mut self, code: u32, handler: F)
    where
        F: Fn(&[u8]) + Send + Sync + 'static,
    {
        self.osc_handlers.insert(code, Box::new(handler));
    }

    /// 注册 DCS 序列处理器
    pub fn register_dcs<F>(&mut self, final_byte: u8, handler: F)
    where
        F: Fn(&[u8]) + Send + Sync + 'static,
    {
        self.dcs_handlers.insert(final_byte, Box::new(handler));
    }

    /// 注销 CSI 序列处理器
    pub fn unregister_csi(&mut self, final_byte: u8) -> bool {
        self.csi_handlers.remove(&final_byte).is_some()
    }

    /// 注销 OSC 序列处理器
    pub fn unregister_osc(&mut self, code: u32) -> bool {
        self.osc_handlers.remove(&code).is_some()
    }

    /// 派发 CSI 序列
    pub fn dispatch_csi(&self, final_byte: u8, data: &[u8]) {
        if let Some(handler) = self.csi_handlers.get(&final_byte) {
            handler(data);
        }
    }

    /// 派发 OSC 序列
    pub fn dispatch_osc(&self, code: u32, data: &[u8]) {
        if let Some(handler) = self.osc_handlers.get(&code) {
            handler(data);
        }
    }

    /// 派发 DCS 序列
    pub fn dispatch_dcs(&self, final_byte: u8, data: &[u8]) {
        if let Some(handler) = self.dcs_handlers.get(&final_byte) {
            handler(data);
        }
    }

    /// 已注册的 CSI 处理器数量
    pub fn csi_handler_count(&self) -> usize {
        self.csi_handlers.len()
    }

    /// 已注册的 OSC 处理器数量
    pub fn osc_handler_count(&self) -> usize {
        self.osc_handlers.len()
    }
}

impl Default for Parser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn test_csi_handler() {
        let mut parser = Parser::new();
        let counter = Arc::new(AtomicUsize::new(0));
        let c = counter.clone();

        parser.register_csi(b'H', move |data| {
            c.fetch_add(1, Ordering::Relaxed);
            assert_eq!(data, b"10;20");
        });

        parser.dispatch_csi(b'H', b"10;20");
        assert_eq!(counter.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_osc_handler() {
        let mut parser = Parser::new();
        let received = Arc::new(std::sync::Mutex::new(Vec::new()));
        let r = received.clone();

        parser.register_osc(8, move |data| {
            r.lock().unwrap().push(data.to_vec());
        });

        parser.dispatch_osc(8, b"https://example.com");
        parser.dispatch_osc(8, b"https://rust-lang.org");

        let received = received.lock().unwrap();
        assert_eq!(received.len(), 2);
        assert_eq!(received[0], b"https://example.com");
    }

    #[test]
    fn test_unregister() {
        let mut parser = Parser::new();
        parser.register_csi(b'm', |_| {});
        assert_eq!(parser.csi_handler_count(), 1);

        assert!(parser.unregister_csi(b'm'));
        assert_eq!(parser.csi_handler_count(), 0);
    }
}
