//! iTerm2 Inline Image 协议解析器
//!
//! 实现 OSC 1337 `File=...` 序列的解析：
//! - 手写 base64 解码（不引入 base64 crate）
//! - 通过 `image` crate 解码 PNG/JPEG 为 RGBA
//! - 返回 [`ImagePlacement`]（`row`/`col` 设为 0，由调用方设置位置）
//!
//! ## 协议格式
//!
//! ```text
//! ESC ] 1337 ; File=<params>:<base64_data> BEL
//! ```
//! 或用 ST 终止：
//! ```text
//! ESC ] 1337 ; File=<params>:<base64_data> ESC \
//! ```
//!
//! `<params>` 为 `key=value;key=value;...` 形式，常用键：
//! - `inline=1`：内联显示（本实现始终内联）
//! - `width` / `height`：像素或 cell 单位（如 `80px` 或 `10c`），本实现忽略，按原始像素尺寸放置
//! - `preserveAspectRatio`：保持比例（本实现忽略）
//!
//! `:` 之后为标准 base64（RFC 4648）编码的图像字节流。

use crate::image::ImagePlacement;

/// base64 解码表（RFC 4648 标准字母表）
///
/// - `A-Z` → 0..25
/// - `a-z` → 26..51
/// - `0-9` → 52..61
/// - `+` → 62
/// - `/` → 63
/// - 其余（含 `=` padding 与无效字符）→ -1
const BASE64_TABLE: [i8; 256] = {
    let mut t = [-1i8; 256];
    let mut i = 0u8;
    while i < 26 {
        t[(b'A' + i) as usize] = i as i8;
        i += 1;
    }
    let mut i = 0u8;
    while i < 26 {
        t[(b'a' + i) as usize] = (26 + i) as i8;
        i += 1;
    }
    let mut i = 0u8;
    while i < 10 {
        t[(b'0' + i) as usize] = (52 + i) as i8;
        i += 1;
    }
    t[b'+' as usize] = 62;
    t[b'/' as usize] = 63;
    t
};

/// 手写 base64 解码（RFC 4648 标准字母表）
///
/// 跳过 ASCII 空白字符；忽略 `=` padding 与其他非字母表字符。
/// 始终返回 `Some(Vec<u8>)`，无效字符不会导致失败（与 lenient 解码一致）。
pub fn decode_base64(input: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0u32;
    for &b in input.iter() {
        if b.is_ascii_whitespace() {
            continue;
        }
        let v = BASE64_TABLE[b as usize];
        if v < 0 {
            // `=` padding 或其他无效字符：跳过
            continue;
        }
        buf = (buf << 6) | (v as u32);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            // 仅保留低 `bits` 位，避免 buf 无界增长导致 u32 溢出
            buf &= (1u32 << bits).wrapping_sub(1);
        }
    }
    Some(out)
}

/// 解析 iTerm2 inline image 为 [`ImagePlacement`]
///
/// - `params`：`File=` 之后、`:` 之前的参数字符串（如 `inline=1;width=80px;height=40px`）
/// - `data_base64`：`:` 之后的 base64 编码字节
///
/// 当前实现忽略所有 params（按原始像素尺寸放置），仅做 base64 解码 + PNG/JPEG 解码。
/// 返回的 `ImagePlacement` 的 `row`/`col` 设为 (0, 0)，由调用方按光标位置设置。
pub fn parse_iterm2_image(params: &str, data_base64: &[u8]) -> Option<ImagePlacement> {
    // params 为显示提示（inline/width/height/preserveAspectRatio），
    // 当前实现按原始像素尺寸放置，忽略这些提示。
    let _ = params;

    let decoded = decode_base64(data_base64)?;
    if decoded.is_empty() {
        return None;
    }

    let img = image::load_from_memory(&decoded).ok()?;
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();
    if width == 0 || height == 0 {
        return None;
    }
    Some(ImagePlacement {
        rgba: rgba.into_raw(),
        width,
        height,
        row: 0,
        col: 0,
    })
}

/// 解析完整 OSC 1337 payload 为 [`ImagePlacement`]
///
/// payload 形如 `File=<params>:<base64_data>`（即 `1337;` 之后、终止符之前的全部字节）：
/// 1. 校验 `File=` 前缀
/// 2. 在剩余部分查找第一个 `:`，将参数与 base64 数据分离
/// 3. 调用 [`parse_iterm2_image`] 完成解码
pub fn parse_iterm2_osc_payload(payload: &[u8]) -> Option<ImagePlacement> {
    let s = std::str::from_utf8(payload).ok()?;
    let rest = s.strip_prefix("File=")?;
    let colon = rest.find(':')?;
    let params = &rest[..colon];
    let base64 = &rest.as_bytes()[colon + 1..];
    parse_iterm2_image(params, base64)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SubTask 10.4: 验证 base64 解码正确性
    #[test]
    fn test_base64_decode() {
        // "test" → "dGVzdA=="
        assert_eq!(decode_base64(b"dGVzdA==").unwrap(), b"test");
        // 空输入
        assert_eq!(decode_base64(b"").unwrap().as_slice(), b"");
        // "hello" → "aGVsbG8="
        assert_eq!(decode_base64(b"aGVsbG8=").unwrap(), b"hello");
        // "rust-xterm" → "cnVzdC14dGVybQ=="
        assert_eq!(decode_base64(b"cnVzdC14dGVybQ==").unwrap(), b"rust-xterm");
        // 含空白字符应被跳过
        assert_eq!(decode_base64(b"dGVz\r\ndA==").unwrap(), b"test");
        // 3 字节 → 4 字符（无 padding）
        assert_eq!(decode_base64(b"Zm9v").unwrap(), b"foo");
        // 2 字节 → 4 字符（含 2 个 padding）
        assert_eq!(decode_base64(b"Zm8=").unwrap(), b"fo");
        // 1 字节 → 4 字符（含 3 个 padding）
        assert_eq!(decode_base64(b"Zg==").unwrap(), b"f");
    }

    /// SubTask 10.4: 1x1 红色 PNG 应解码为 RGBA [255, 0, 0, 255]
    #[test]
    fn test_iterm2_png_decode() {
        // 1x1 红色不透明 PNG 的 base64
        let b64 = b"iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAAEElEQVR4AQEFAPr/AP8AAP8FAAH/+lyI0QAAAABJRU5ErkJggg==";
        let placement = parse_iterm2_image("", b64).expect("应解析成功");
        assert_eq!(placement.width, 1, "宽度应为 1 像素");
        assert_eq!(placement.height, 1, "高度应为 1 像素");
        assert_eq!(placement.rgba.len(), 4, "RGBA 数据长度应为 4");
        assert_eq!(placement.rgba[0], 255, "R 应为 255");
        assert_eq!(placement.rgba[1], 0, "G 应为 0");
        assert_eq!(placement.rgba[2], 0, "B 应为 0");
        assert_eq!(placement.rgba[3], 255, "A 应为 255");
        // row/col 应为 0（由调用方设置）
        assert_eq!(placement.row, 0);
        assert_eq!(placement.col, 0);
    }

    /// SubTask 10.4: 完整 OSC payload 解析（含 File= 前缀和参数）
    #[test]
    fn test_iterm2_osc_payload() {
        let payload = b"File=inline=1;width=1px;height=1px;preserveAspectRatio=1:iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAAEElEQVR4AQEFAPr/AP8AAP8FAAH/+lyI0QAAAABJRU5ErkJggg==";
        let placement = parse_iterm2_osc_payload(payload).expect("应解析成功");
        assert_eq!(placement.width, 1);
        assert_eq!(placement.height, 1);
        assert_eq!(&placement.rgba[..], &[255, 0, 0, 255]);
    }

    /// 无效 payload 应返回 None
    #[test]
    fn test_iterm2_invalid_payload() {
        // 缺少 File= 前缀
        assert!(parse_iterm2_osc_payload(b"foo=bar:abc").is_none());
        // 缺少冒号分隔符
        assert!(parse_iterm2_osc_payload(b"File=inline=1").is_none());
        // 空 base64 → 解码为空字节 → image 解码失败
        assert!(parse_iterm2_osc_payload(b"File=:").is_none());
        // 非 UTF-8 payload
        assert!(parse_iterm2_osc_payload(&[0xFF, 0xFE, 0xFD]).is_none());
        // 非图像数据（base64 解码成功但 image::load_from_memory 失败）
        assert!(parse_iterm2_osc_payload(b"File=:dGVzdA==").is_none());
    }
}
