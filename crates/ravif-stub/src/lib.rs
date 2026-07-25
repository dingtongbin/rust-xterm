//! ravif 0.13.0 的 MSRV 兼容桩
//!
//! 本 crate 仅用于让 `image` 0.25.10 的 `avif` feature 在 Rust 1.72 下编译通过。
//! 所有 encode 方法恒定返回 `Err`，因为 rust-xterm 的核心库（基于 wezterm-term）
//! 只用 `image::RgbaImage` / `image::imageops` 处理 sixel/iterm2 图像协议，
//! 从不进行 AVIF 编码，运行时永远不会走到这些方法。
//!
//! 公开 API 表面与上游 ravif 0.13.0 严格一致，详见
//! <https://docs.rs/ravif/0.13.0>

#![forbid(unsafe_code)]

// 重导出上游 ravif 同名类型，保持 `use ravif::{Img, RGB8, RGBA8}` 可用
pub use imgref::Img;
pub use rgb::{RGB8, RGBA8};

/// AVIF 编码器内部使用的颜色模型
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ColorModel {
    RGB,
    YCbCr,
}

/// 历史别名，与上游 `pub type ColorSpace = ColorModel;` 一致
pub type ColorSpace = ColorModel;

/// AVIF 编码的位深
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum BitDepth {
    Eight,
    Ten,
}

/// Alpha 通道处理模式（与上游一致，image 0.25.10 当前未直接使用）
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum AlphaColorMode {
    Unassociated,
    Associated,
}

/// 编码后的 AVIF 图像
#[derive(Debug, Clone)]
pub struct EncodedImage {
    /// AVIF (HEIF+AV1) 编码后的图像数据
    pub avif_file: Vec<u8>,
    /// 颜色通道 AV1 负载字节数
    pub color_byte_size: usize,
    /// Alpha 通道 AV1 负载字节数
    pub alpha_byte_size: usize,
}

/// 错误类型
///
/// 与上游 ravif 0.13.0 的 `Error` 枚举保持一致，便于 image 的 `From` 转换。
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// 输入缓冲区小于 width * height
    TooFewPixels,
    /// 不支持的操作
    Unsupported(&'static str),
    /// 编码错误（桩内不会产生，仅为 API 兼容）
    EncodingError,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::TooFewPixels => write!(f, "Provided buffer is smaller than width * height"),
            Error::Unsupported(msg) => write!(f, "Not supported: {}", msg),
            Error::EncodingError => write!(f, "Encoding error reported by rav1e (stub)"),
        }
    }
}

impl std::error::Error for Error {}

/// AVIF 编码器
///
/// 与上游 `ravif::Encoder<'exif_slice>` 一致的 builder API。
/// 桩实现：所有 builder 方法返回 `Self`（无副作用），`encode_*` 恒返回 `Err`。
#[derive(Debug, Clone)]
pub struct Encoder<'exif_slice> {
    _exif: std::marker::PhantomData<&'exif_slice [u8]>,
}

impl<'exif_slice> Encoder<'exif_slice> {
    /// 创建默认编码器
    pub fn new() -> Self {
        Self {
            _exif: std::marker::PhantomData,
        }
    }

    /// 设置质量（0.0-100.0）
    pub fn with_quality(self, _quality: f32) -> Self {
        self
    }

    /// 设置位深（Option<u8> 形式）
    pub fn with_depth(self, _depth: Option<u8>) -> Self {
        self
    }

    /// 设置位深
    pub fn with_bit_depth(self, _depth: BitDepth) -> Self {
        self
    }

    /// 设置 Alpha 通道质量
    pub fn with_alpha_quality(self, _quality: f32) -> Self {
        self
    }

    /// 设置编码速度（1-10）
    pub fn with_speed(self, _speed: u8) -> Self {
        self
    }

    /// 设置内部颜色模型
    pub fn with_internal_color_model(self, _color_model: ColorModel) -> Self {
        self
    }

    /// 设置内部颜色空间（历史别名）
    pub fn with_internal_color_space(self, color_model: ColorModel) -> Self {
        self.with_internal_color_model(color_model)
    }

    /// 配置 rayon 线程池大小
    pub fn with_num_threads(self, _num_threads: Option<usize>) -> Self {
        self
    }

    /// 设置 Alpha 颜色模式
    pub fn with_alpha_color_mode(self, _mode: AlphaColorMode) -> Self {
        self
    }

    /// 设置 EXIF 元数据
    pub fn with_exif(self, _exif_data: impl Into<std::borrow::Cow<'exif_slice, [u8]>>) -> Self {
        self
    }

    /// 编码 RGBA 图像
    ///
    /// 桩实现：恒返回 `Err`。rust-xterm/wezterm-term 从不调用此方法。
    pub fn encode_rgba(&self, _in_buffer: Img<&[RGBA8]>) -> Result<EncodedImage, Error> {
        Err(Error::Unsupported("ravif stub: AVIF encoding not available"))
    }

    /// 编码 RGB 图像
    ///
    /// 桩实现：恒返回 `Err`。rust-xterm/wezterm-term 从不调用此方法。
    pub fn encode_rgb(&self, _buffer: Img<&[RGB8]>) -> Result<EncodedImage, Error> {
        Err(Error::Unsupported("ravif stub: AVIF encoding not available"))
    }

    /// 直接编码 8 位平面（上游 API 兼容）
    pub fn encode_raw_planes_8_bit(
        &self,
        _width: usize,
        _height: usize,
        _y: &[u8],
        _u: &[u8],
        _v: &[u8],
        _a: Option<&[u8]>,
    ) -> Result<EncodedImage, Error> {
        Err(Error::Unsupported("ravif stub: AVIF encoding not available"))
    }

    /// 直接编码 10 位平面（上游 API 兼容）
    pub fn encode_raw_planes_10_bit(
        &self,
        _width: usize,
        _height: usize,
        _y: &[u16],
        _u: &[u16],
        _v: &[u16],
        _a: Option<&[u16]>,
    ) -> Result<EncodedImage, Error> {
        Err(Error::Unsupported("ravif stub: AVIF encoding not available"))
    }
}

impl<'exif_slice> Default for Encoder<'exif_slice> {
    fn default() -> Self {
        Self::new()
    }
}
