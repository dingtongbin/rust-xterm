//! NullWriter / CapturingWriter：终端输出写入器
//!
//! WezTerm 的 `Terminal::new` 要求一个 `Box<dyn Write + Send>` 作为
//! writer，用于接收终端产生的输出（鼠标报告、CSI 查询响应、OSC 响应等）。
//!
//! ## 两种实现
//!
//! - [`NullWriter`]：丢弃所有输出，用于纯展示场景（无 PTY 回写需求）
//! - [`CapturingWriter`]：将输出缓存到共享缓冲区，宿主层可取出并发送给 PTY。
//!   这使得鼠标报告（SGR mouse）、光标位置查询（CSI 6n）、颜色查询等终端
//!   响应能够正确回传给子进程。
//!
//! ## 设计原则
//!
//! - 核心层不直接依赖 PTY：写入器仅缓存，由宿主层负责取出并转发
//! - 线程安全：`CapturingWriter` 使用 `Arc<Mutex<Vec<u8>>>`，因为 WezTerm
//!   可能在内部线程中写入（如响应生成）。该锁仅持有极短时间，不在热路径上

use std::io::{self, Write};
use std::sync::{Arc, Mutex};

/// 一个丢弃所有写入数据的空写入器。
///
/// 所有写入操作都返回 `Ok(n)`，其中 `n` 等于输入字节长度，
/// 模拟"成功写入"但不产生任何副作用。
#[derive(Debug, Default, Clone)]
pub struct NullWriter;

impl NullWriter {
    /// 创建一个新的 NullWriter
    pub fn new() -> Self {
        Self
    }
}

impl Write for NullWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // 假装写入成功，返回完整长度
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// 共享输出缓冲区类型
pub type OutputBuffer = Arc<Mutex<Vec<u8>>>;

/// 一个捕获终端输出的写入器。
///
/// 终端产生的输出（鼠标报告、查询响应等）会被缓存到共享缓冲区，
/// 宿主层可通过 [`CapturingWriter::drain`] 取出并转发给 PTY。
///
/// 这使得以下功能得以工作：
/// - SGR 鼠标模式（DECSET 1006）
/// - 光标位置查询响应（CSI ?6n / CSI 6n）
/// - 颜色查询响应（OSC 4 / OSC 10 / OSC 11）
/// - 终端属性查询响应（CSI 0c）
#[derive(Debug, Clone)]
pub struct CapturingWriter {
    buffer: OutputBuffer,
}

impl CapturingWriter {
    /// 创建新的捕获写入器，返回写入器与共享缓冲区句柄
    pub fn new() -> (Self, OutputBuffer) {
        let buffer: OutputBuffer = Arc::new(Mutex::new(Vec::with_capacity(256)));
        (
            Self {
                buffer: buffer.clone(),
            },
            buffer,
        )
    }

    /// 从共享缓冲区取出所有已缓存的数据（非阻塞）
    ///
    /// 宿主层应在每次 `tick` 后调用此方法，将终端响应转发给 PTY。
    pub fn drain(buffer: &OutputBuffer) -> Vec<u8> {
        match buffer.lock() {
            Ok(mut buf) => {
                let data = buf.clone();
                buf.clear();
                data
            }
            Err(_) => Vec::new(),
        }
    }
}

impl Default for CapturingWriter {
    fn default() -> Self {
        Self::new().0
    }
}

impl Write for CapturingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self.buffer.lock() {
            Ok(mut target) => {
                target.extend_from_slice(buf);
                Ok(buf.len())
            }
            Err(_) => Err(io::Error::other("buffer poisoned")),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
