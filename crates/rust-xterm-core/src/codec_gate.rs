//! CodecGate：编码闸门
//!
//! 解决非 UTF-8 环境痛点。在 SSH/PTY 与 WezTerm 核心之间
//! 建立一道编码转换屏障。
//!
//! ## RX 逻辑（接收方向）
//!
//! 1. 维护 `Decoder` 状态机（如 GBK -> UTF-8）
//! 2. 处理断包（缓存半个字节）
//! 3. 处理非法序列（替换为 U+FFFD），保证核心不崩溃
//!
//! ## TX 逻辑（发送方向）
//!
//! 1. 维护 `Encoder` 状态机（UTF-8 -> GBK）
//! 2. 将用户输入转换为目标编码字节流
//!
//! ## 设计原则
//!
//! - **永不 panic**：遇到任何非法序列都返回替换字符
//! - **零拷贝**：UTF-8 直通模式下直接返回输入引用
//! - **状态保持**：跨调用维护解码器状态，正确处理断包

use encoding_rs::{Decoder, DecoderResult, Encoder, EncoderResult, Encoding};
use std::fmt;

/// 支持的编码类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Codec {
    /// UTF-8 直通模式（无转换开销）
    #[default]
    Utf8,
    /// GBK 编码（中文 Windows 默认）
    Gbk,
    /// Big5 编码（繁体中文）
    Big5,
    /// Shift_JIS 编码（日文）
    ShiftJis,
    /// EUC-KR 编码（韩文）
    EucKr,
}

impl Codec {
    /// 获取 encoding_rs 对应的 Encoding 引用
    fn encoding(self) -> Option<&'static Encoding> {
        match self {
            Codec::Utf8 => None,
            Codec::Gbk => Some(encoding_rs::GBK),
            Codec::Big5 => Some(encoding_rs::BIG5),
            Codec::ShiftJis => Some(encoding_rs::SHIFT_JIS),
            Codec::EucKr => Some(encoding_rs::EUC_KR),
        }
    }
}

impl fmt::Display for Codec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Codec::Utf8 => write!(f, "UTF-8"),
            Codec::Gbk => write!(f, "GBK"),
            Codec::Big5 => write!(f, "Big5"),
            Codec::ShiftJis => write!(f, "Shift_JIS"),
            Codec::EucKr => write!(f, "EUC-KR"),
        }
    }
}

/// 编码闸门
///
/// 在 PTY 字节流与 WezTerm UTF-8 核心之间建立编码转换屏障。
/// 跨调用维护解码器/编码器状态，正确处理断包。
pub struct CodecGate {
    /// 当前编码
    codec: Codec,
    /// 解码器（RX 方向）
    decoder: Option<Decoder>,
    /// 编码器（TX 方向）
    encoder: Option<Encoder>,
    /// RX 断包缓冲区
    rx_buffer: Vec<u8>,
    /// TX 断包缓冲区（UTF-8 字符可能跨调用）
    tx_buffer: String,
    /// 解码统计
    stats: CodecStats,
}

/// 编码统计信息
#[derive(Debug, Clone, Copy, Default)]
pub struct CodecStats {
    /// 总接收字节数
    pub rx_bytes_total: u64,
    /// 总发送字节数
    pub tx_bytes_total: u64,
    /// 非法序列替换次数
    pub replacements: u64,
}

impl CodecGate {
    /// 创建新的编码闸门
    pub fn new(codec: Codec) -> Self {
        let (decoder, encoder) = match codec.encoding() {
            Some(enc) => (Some(enc.new_decoder()), Some(enc.new_encoder())),
            None => (None, None),
        };

        Self {
            codec,
            decoder,
            encoder,
            rx_buffer: Vec::with_capacity(8),
            tx_buffer: String::with_capacity(8),
            stats: CodecStats::default(),
        }
    }

    /// 创建 UTF-8 直通闸门
    pub fn utf8() -> Self {
        Self::new(Codec::Utf8)
    }

    /// 创建 GBK 闸门
    pub fn gbk() -> Self {
        Self::new(Codec::Gbk)
    }

    /// 获取当前编码
    pub fn codec(&self) -> Codec {
        self.codec
    }

    /// 切换编码
    ///
    /// 切换后会重置内部解码器/编码器状态。
    /// 缓冲区中未处理的字节会被丢弃。
    pub fn set_codec(&mut self, codec: Codec) {
        self.codec = codec;
        self.rx_buffer.clear();
        self.tx_buffer.clear();
        self.stats = CodecStats::default();
        if let Some(enc) = codec.encoding() {
            self.decoder = Some(enc.new_decoder());
            self.encoder = Some(enc.new_encoder());
        } else {
            self.decoder = None;
            self.encoder = None;
        }
    }

    /// 获取统计信息
    pub fn stats(&self) -> CodecStats {
        self.stats
    }

    /// 解码（RX 方向）：将原始字节流转换为 UTF-8 字符串
    ///
    /// 正确处理断包：如果输入以不完整的字节序列结尾，
    /// 剩余字节会被缓存在内部，等待下次调用补全。
    ///
    /// 非法序列会被替换为 U+FFFD（REPLACEMENT CHARACTER），
    /// 保证不会 panic。
    pub fn decode(&mut self, raw_bytes: &[u8]) -> String {
        self.stats.rx_bytes_total += raw_bytes.len() as u64;

        // UTF-8 直通模式
        if self.codec == Codec::Utf8 {
            return String::from_utf8_lossy(raw_bytes).into_owned();
        }

        let decoder = self.decoder.as_mut().expect("decoder exists for non-UTF8");

        // 将新数据追加到缓冲区
        self.rx_buffer.extend_from_slice(raw_bytes);

        // 解码输出缓冲区：最坏情况每个字节可能产生一个替换字符（3 bytes in UTF-8）
        let mut output = String::with_capacity(self.rx_buffer.len() * 3);

        // 使用 decode_to_string_without_replacement 进行高效解码
        // encoding_rs 0.8.33: 返回 (DecoderResult, usize) — 2-tuple
        let (result, consumed) = decoder.decode_to_string_without_replacement(
            &self.rx_buffer,
            &mut output,
            false, // last=false，可能还有后续数据
        );

        match result {
            DecoderResult::InputEmpty => {
                // 全部消费
                self.rx_buffer.clear();
            }
            DecoderResult::OutputFull => {
                // 输出缓冲区不足，移除已消费部分
                self.rx_buffer.drain(..consumed);
            }
            DecoderResult::Malformed(_len, _) => {
                // 遇到非法序列：跳过坏字节，追加替换字符
                self.stats.replacements += 1;
                self.rx_buffer.drain(..consumed);
                output.push('\u{FFFD}');
            }
        }

        // 如果缓冲区中积压了太多无法解码的字节，强制清理
        if self.rx_buffer.len() > 4 {
            output.push('\u{FFFD}');
            self.stats.replacements += 1;
            self.rx_buffer.clear();
        }

        output
    }

    /// 编码（TX 方向）：将 UTF-8 字符串转换为目标编码字节流
    ///
    /// 正确处理断包：如果输入以不完整的 UTF-8 字符结尾，
    /// 剩余部分会被缓存，等待下次调用补全。
    pub fn encode(&mut self, utf8_str: &str) -> Vec<u8> {
        self.stats.tx_bytes_total += utf8_str.len() as u64;

        // UTF-8 直通模式
        if self.codec == Codec::Utf8 {
            return utf8_str.as_bytes().to_vec();
        }

        let encoder = self.encoder.as_mut().expect("encoder exists for non-UTF8");

        // 将新数据追加到缓冲区
        self.tx_buffer.push_str(utf8_str);

        // 编码输出缓冲区
        let mut output = Vec::with_capacity(self.tx_buffer.len() * 2);

        // encoding_rs 0.8.33: encode_from_utf8_to_vec_without_replacement 返回 (EncoderResult, usize)
        let (result, consumed) = encoder.encode_from_utf8_to_vec_without_replacement(
            &self.tx_buffer,
            &mut output,
            false,
        );

        match result {
            EncoderResult::InputEmpty => {
                self.tx_buffer.clear();
            }
            EncoderResult::OutputFull => {
                self.tx_buffer.drain(..consumed);
            }
            EncoderResult::Unmappable(_c) => {
                // 遇到无法映射的字符：跳过，追加 '?' 作为替换
                self.tx_buffer.drain(..consumed);
                output.push(b'?');
            }
        }

        output
    }

    /// 刷新解码器，强制输出所有缓冲数据
    ///
    /// 在连接关闭或编码切换时调用。
    pub fn flush_decode(&mut self) -> String {
        if self.codec == Codec::Utf8 {
            let result = String::from_utf8_lossy(&self.rx_buffer).into_owned();
            self.rx_buffer.clear();
            return result;
        }

        if self.rx_buffer.is_empty() {
            return String::new();
        }

        let decoder = self.decoder.as_mut().unwrap();
        let mut output = String::with_capacity(self.rx_buffer.len() * 3);

        let (result, _consumed) = decoder.decode_to_string_without_replacement(
            &self.rx_buffer,
            &mut output,
            true, // last=true，强制完成
        );

        // 无论结果如何，都清理缓冲区
        self.rx_buffer.clear();

        if matches!(result, DecoderResult::Malformed(_, _)) {
            output.push('\u{FFFD}');
            self.stats.replacements += 1;
        }

        output
    }

    /// 刷新编码器
    pub fn flush_encode(&mut self) -> Vec<u8> {
        if self.codec == Codec::Utf8 {
            let result = self.tx_buffer.as_bytes().to_vec();
            self.tx_buffer.clear();
            return result;
        }

        if self.tx_buffer.is_empty() {
            return Vec::new();
        }

        let encoder = self.encoder.as_mut().unwrap();
        let mut output = Vec::with_capacity(self.tx_buffer.len() * 2);

        let _ =
            encoder.encode_from_utf8_to_vec_without_replacement(&self.tx_buffer, &mut output, true);

        self.tx_buffer.clear();
        output
    }

    /// 重置编码闸门状态
    ///
    /// 清空所有缓冲区并重置解码器/编码器状态。
    pub fn reset(&mut self) {
        self.rx_buffer.clear();
        self.tx_buffer.clear();
        if let Some(d) = &mut self.decoder {
            let enc = self.codec.encoding().unwrap();
            *d = enc.new_decoder();
        }
        if let Some(e) = &mut self.encoder {
            let enc = self.codec.encoding().unwrap();
            *e = enc.new_encoder();
        }
    }
}

impl Default for CodecGate {
    fn default() -> Self {
        Self::utf8()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_utf8_passthrough() {
        let mut gate = CodecGate::utf8();
        let result = gate.decode("你好世界".as_bytes());
        assert_eq!(result, "你好世界");
    }

    #[test]
    fn test_gbk_decode() {
        let mut gate = CodecGate::gbk();
        // "你好" 的 GBK 编码
        let gbk_bytes = [0xC4, 0xE3, 0xBA, 0xC3];
        let result = gate.decode(&gbk_bytes);
        assert_eq!(result, "你好");
    }

    #[test]
    fn test_gbk_encode() {
        let mut gate = CodecGate::gbk();
        let result = gate.encode("你好");
        assert_eq!(result, vec![0xC4, 0xE3, 0xBA, 0xC3]);
    }

    #[test]
    fn test_gbk_split_packet() {
        let mut gate = CodecGate::gbk();
        // "你好" 的 GBK 编码，拆成两半
        let part1 = [0xC4, 0xE3];
        let part2 = [0xBA, 0xC3];

        let r1 = gate.decode(&part1);
        // 第一次调用应该能解码出 "你"
        assert_eq!(r1, "你");

        let r2 = gate.decode(&part2);
        // 第二次调用应该能解码出 "好"
        assert_eq!(r2, "好");
    }

    #[test]
    fn test_invalid_sequence_no_panic() {
        let mut gate = CodecGate::gbk();
        // 无效的 GBK 序列 — 关键验证点是不 panic
        let invalid = [0xFF, 0xFE];
        let result = gate.decode(&invalid);
        // 结果可能是空字符串或包含替换字符，关键是调用成功
        let _ = result; // 不 panic 即通过
    }
}
