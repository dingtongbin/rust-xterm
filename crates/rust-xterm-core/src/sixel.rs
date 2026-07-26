//! Sixel 图像协议解析器
//!
//! 手写实现 DCS + Sixel 数据解析，输出 RGBA 像素。不依赖外部图像库。
//!
//! ## Sixel 协议格式
//!
//! ```text
//! DCS P1;P2;P3 q <sixel_data> ST
//! ```
//!
//! - DCS 开始：`\x1bP`（0x1b 0x50）
//! - 参数 P1;P2;P3：背景色模式、私有模式、水平网格（本实现跳过）
//! - 数据结束：ST `\x1b\\` 或 BEL `\x07`
//! - Sixel 数据：
//!   - `!` + 数字 + 字符 = RLE 重复（如 `!5~` 表示 5 个 `~`）
//!   - `?`..`~` = sixel 字符（6 像素列，每 bit 一行）
//!   - `#` + 数字 = 选择/定义颜色寄存器
//!   - `-` = 回车换行（新六行）
//!   - `$` = 回车（回到行首）

use crate::image::ImagePlacement;

/// 默认调色板：前 16 色为 ANSI 标准色，其余为黑
const DEFAULT_PALETTE: [(u8, u8, u8); 16] = [
    (0, 0, 0),       // 0  black
    (205, 0, 0),     // 1  red
    (0, 205, 0),     // 2  green
    (205, 205, 0),   // 3  yellow
    (0, 0, 238),     // 4  blue
    (205, 0, 205),   // 5  magenta
    (0, 205, 205),   // 6  cyan
    (229, 229, 229), // 7  white
    (127, 127, 127), // 8  bright black
    (255, 0, 0),     // 9  bright red
    (0, 255, 0),     // 10 bright green
    (255, 255, 0),   // 11 bright yellow
    (92, 92, 255),   // 12 bright blue
    (255, 0, 255),   // 13 bright magenta
    (0, 255, 255),   // 14 bright cyan
    (255, 255, 255), // 15 bright white
];

/// 扩展像素缓冲到可寻址 (px, py)
///
/// `pixels` 是按行存储的二维缓冲（每行一个 `Vec<(r,g,b)>`），
/// 行长按需扩展，行数也按需追加。新增像素初始化为 (0, 0, 0)。
fn ensure_size(pixels: &mut Vec<Vec<(u8, u8, u8)>>, px: u32, py: u32) {
    let need_rows = (py as usize) + 1;
    while pixels.len() < need_rows {
        pixels.push(Vec::new());
    }
    let need_cols = (px as usize) + 1;
    for row in pixels.iter_mut() {
        if row.len() < need_cols {
            row.resize(need_cols, (0, 0, 0));
        }
    }
}

/// 解析 Sixel DCS 序列为 RGBA 图像
///
/// 输入：完整的 DCS 序列 `\x1bP<p1>;<p2>;<p3>q<data>\x1b\\` 或以 BEL 结尾。
/// 也可接受不含 DCS 前缀的裸 sixel 数据（直接从参数或 sixel 字符开始）。
///
/// 输出：[`ImagePlacement`]，`row`/`col` 设为 (0, 0)，由调用方按需设置位置。
///
/// # 简化实现
///
/// - 默认调色板：前 16 色 ANSI 标准色，其余为黑
/// - 仅支持 RGB 颜色定义 `#n;2;r;g;b`（0-100 各分量），不支持 HLS
/// - 颜色寄存器上限 256（index 按 `& 0xFF` 折叠）
/// - 未设置的像素 alpha=0（透明），已设置像素 alpha=255
pub fn parse_sixel(data: &[u8]) -> Option<ImagePlacement> {
    let len = data.len();
    let mut i = 0usize;

    // 1. 跳过 DCS 起始符 \x1bP（容错也接受 \x1b[）
    if i + 1 < len && data[i] == 0x1b {
        match data[i + 1] {
            0x50 | 0x5b => i += 2,
            _ => return None,
        }
    }

    // 2. 跳过 DCS 中间参数 (P1;P2;P3) 直到 'q'
    while i < len && data[i] != b'q' {
        i += 1;
    }
    if i >= len {
        return None;
    }
    i += 1; // 跳过 'q'

    // 3. 初始化颜色寄存器
    let mut color_registers: [(u8, u8, u8); 256] = [(0, 0, 0); 256];
    for (idx, &c) in DEFAULT_PALETTE.iter().enumerate() {
        color_registers[idx] = c;
    }
    let mut current_color: u8 = 0;

    // 4. 像素缓冲：每行一个 Vec<(r,g,b)>，按需扩展
    let mut pixels: Vec<Vec<(u8, u8, u8)>> = Vec::new();
    let mut x: u32 = 0;
    let mut y: u32 = 0; // sixel-row 索引（实际像素 y = y * 6）
    let mut max_x: u32 = 0;
    let mut max_y: u32 = 0; // sixel-row 单位

    while i < len {
        let c = data[i];
        match c {
            // ST (String Terminator): \x1b\\
            0x1b => {
                if i + 1 < len && data[i + 1] == 0x5c {
                    break;
                }
                i += 1;
            }
            // BEL 终止
            0x07 => break,

            // 颜色寄存器选择/定义
            b'#' => {
                i += 1;
                let mut reg: u32 = 0;
                let mut has_digit = false;
                while i < len && data[i].is_ascii_digit() {
                    reg = reg
                        .saturating_mul(10)
                        .saturating_add((data[i] - b'0') as u32);
                    has_digit = true;
                    i += 1;
                }
                if !has_digit {
                    continue;
                }
                let reg = (reg & 0xFF) as u8;

                // 检查是否带颜色定义 (#n;co;...)
                if i < len && data[i] == b';' {
                    i += 1; // skip ';'
                    let mut co: u32 = 0;
                    while i < len && data[i].is_ascii_digit() {
                        co = co
                            .saturating_mul(10)
                            .saturating_add((data[i] - b'0') as u32);
                        i += 1;
                    }
                    if co == 2 {
                        // RGB: #n;2;r;g;b (0-100 each)
                        let mut comps = [0u32; 3];
                        for comp in comps.iter_mut() {
                            if i < len && data[i] == b';' {
                                i += 1;
                            }
                            let mut v: u32 = 0;
                            while i < len && data[i].is_ascii_digit() {
                                v = v.saturating_mul(10).saturating_add((data[i] - b'0') as u32);
                                i += 1;
                            }
                            *comp = v;
                        }
                        let r = (comps[0].min(100) * 255 / 100) as u8;
                        let g = (comps[1].min(100) * 255 / 100) as u8;
                        let b = (comps[2].min(100) * 255 / 100) as u8;
                        color_registers[reg as usize] = (r, g, b);
                    } else {
                        // HLS 或未知色彩空间：跳过剩余参数直到下一个 sixel-data 字符
                        while i < len {
                            let c2 = data[i];
                            if matches!(c2, b'#' | b'!' | b'-' | b'$' | 0x1b | 0x07)
                                || (0x3f..=0x7e).contains(&c2)
                            {
                                break;
                            }
                            i += 1;
                        }
                    }
                }
                current_color = reg;
            }

            // RLE: !n<char>
            b'!' => {
                i += 1;
                let mut count: u32 = 0;
                let mut has_digit = false;
                while i < len && data[i].is_ascii_digit() {
                    count = count
                        .saturating_mul(10)
                        .saturating_add((data[i] - b'0') as u32);
                    has_digit = true;
                    i += 1;
                }
                if !has_digit || count == 0 {
                    count = 1;
                }
                if i >= len {
                    break;
                }
                let sc = data[i];
                if !(0x3f..=0x7e).contains(&sc) {
                    i += 1;
                    continue;
                }
                let sixel_val = sc - 0x3f;
                let (r, g, b) = color_registers[current_color as usize];
                let base_y = y * 6;
                for _ in 0..count {
                    for bit in 0..6u32 {
                        if (sixel_val as u32) & (1 << bit) != 0 {
                            let px = x;
                            let py = base_y + bit;
                            ensure_size(&mut pixels, px, py);
                            pixels[py as usize][px as usize] = (r, g, b);
                        }
                    }
                    x += 1;
                    if x > max_x {
                        max_x = x;
                    }
                }
                if y + 1 > max_y {
                    max_y = y + 1;
                }
                i += 1;
            }

            // 换行（新六行）
            b'-' => {
                y += 1;
                x = 0;
                i += 1;
            }

            // 回车（回到行首，同一 sixel 行）
            b'$' => {
                x = 0;
                i += 1;
            }

            // sixel 字符 (0x3f..=0x7e)
            0x3f..=0x7e => {
                let sixel_val = c - 0x3f;
                let (r, g, b) = color_registers[current_color as usize];
                let base_y = y * 6;
                for bit in 0..6u32 {
                    if (sixel_val as u32) & (1 << bit) != 0 {
                        let px = x;
                        let py = base_y + bit;
                        ensure_size(&mut pixels, px, py);
                        pixels[py as usize][px as usize] = (r, g, b);
                    }
                }
                x += 1;
                if x > max_x {
                    max_x = x;
                }
                if y + 1 > max_y {
                    max_y = y + 1;
                }
                i += 1;
            }

            // 其他字符（空格、控制字符等）忽略
            _ => {
                i += 1;
            }
        }
    }

    // 5. 计算最终尺寸
    let width = max_x;
    let height = max_y * 6;
    if width == 0 || height == 0 {
        return None;
    }

    // 6. 转换为 RGBA 缓冲
    let mut rgba = vec![0u8; (width as usize) * (height as usize) * 4];
    for (py, row) in pixels.iter().enumerate() {
        for (px, &(r, g, b)) in row.iter().enumerate() {
            let idx = (py * width as usize + px) * 4;
            if idx + 4 > rgba.len() {
                continue;
            }
            rgba[idx] = r;
            rgba[idx + 1] = g;
            rgba[idx + 2] = b;
            rgba[idx + 3] = 255; // 已设置像素不透明
        }
    }

    Some(ImagePlacement {
        rgba,
        width,
        height,
        row: 0,
        col: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SubTask 9.6: 简单 Sixel 解析
    ///
    /// 构造 1 列 × 6 行的红色 Sixel 序列：
    /// - `#0;2;100;0;0`：定义寄存器 0 为 RGB(100%, 0%, 0%) = (255, 0, 0)
    /// - `#0`：选择寄存器 0
    /// - `~`：sixel 字符 63（0b111111），6 行像素全部设置
    ///
    /// 注：spec 描述 `?` 表示"仅第 0 行"，但标准 Sixel 编码中 `?`=0（无像素），
    /// `@`=1（仅第 0 行）。这里使用 `~` 使所有 6 行均被设置为红色，
    /// 便于断言颜色。
    #[test]
    fn test_sixel_parse_simple() {
        let seq = b"\x1bPq#0;2;100;0;0#0~\x1b\\";
        let placement = parse_sixel(seq).expect("应解析成功");
        assert_eq!(placement.width, 1, "宽度应为 1 像素");
        assert_eq!(placement.height, 6, "高度应为 6 像素（一个 sixel 行）");
        assert_eq!(placement.rgba.len(), 24);
        // 所有 6 个像素应为红色 (255, 0, 0, 255)
        for py in 0..6 {
            let idx = py * 4;
            assert_eq!(placement.rgba[idx], 255, "py={py} R 应为 255");
            assert_eq!(placement.rgba[idx + 1], 0, "py={py} G 应为 0");
            assert_eq!(placement.rgba[idx + 2], 0, "py={py} B 应为 0");
            assert_eq!(placement.rgba[idx + 3], 255, "py={py} A 应为 255");
        }
    }

    /// SubTask 9.6: 2 列红色 Sixel（spec 原始意图）
    ///
    /// 2 个 sixel 字符 `~` 产生 2 列 × 6 行红色像素。
    #[test]
    fn test_sixel_parse_two_columns() {
        let seq = b"\x1bP1;0;0q#0;2;100;0;0#0~~\x1b\\";
        let placement = parse_sixel(seq).expect("应解析成功");
        assert_eq!(placement.width, 2);
        assert_eq!(placement.height, 6);
        assert_eq!(placement.rgba.len(), 2 * 6 * 4);
        for i in 0..12 {
            let idx = i * 4;
            assert_eq!(placement.rgba[idx], 255, "pixel {i} R");
            assert_eq!(placement.rgba[idx + 1], 0, "pixel {i} G");
            assert_eq!(placement.rgba[idx + 2], 0, "pixel {i} B");
            assert_eq!(placement.rgba[idx + 3], 255, "pixel {i} A");
        }
    }

    /// SubTask 9.6: RLE 重复展开
    ///
    /// `!5~` 展开为 5 个 `~` 字符，应产生 5 像素列 × 6 像素行。
    /// 使用默认颜色寄存器 0（黑色）。
    #[test]
    fn test_sixel_rle() {
        let seq = b"\x1bPq!5~\x1b\\";
        let placement = parse_sixel(seq).expect("应解析成功");
        assert_eq!(placement.width, 5, "RLE 应展开为 5 像素列");
        assert_eq!(placement.height, 6, "高度应为 6 像素");
        assert_eq!(placement.rgba.len(), 5 * 6 * 4);
        // 所有像素应为黑色 (0, 0, 0, 255)（默认寄存器 0）
        for py in 0..6 {
            for px in 0..5 {
                let idx = (py * 5 + px) * 4;
                assert_eq!(placement.rgba[idx], 0, "px={px} py={py} R");
                assert_eq!(placement.rgba[idx + 1], 0, "px={px} py={py} G");
                assert_eq!(placement.rgba[idx + 2], 0, "px={px} py={py} B");
                assert_eq!(placement.rgba[idx + 3], 255, "px={px} py={py} A");
            }
        }
    }

    /// RLE 用 `?`（无像素设置）—— 仅断言宽度
    ///
    /// 验证 RLE 即使在字符不设置像素时也正确推进 x 坐标。
    #[test]
    fn test_sixel_rle_zero_pixel() {
        let seq = b"\x1bPq!5?\x1b\\";
        let placement = parse_sixel(seq).expect("应解析成功");
        assert_eq!(placement.width, 5, "RLE 应展开为 5 像素列");
        assert_eq!(placement.height, 6, "高度应为 6 像素");
    }

    /// 多 sixel 行（`-` 换行）
    #[test]
    fn test_sixel_multi_row() {
        // 2 行 sixel，每行 1 列 `~`：总 1×12 像素
        let seq = b"\x1bPq~-~\x1b\\";
        let placement = parse_sixel(seq).expect("应解析成功");
        assert_eq!(placement.width, 1);
        assert_eq!(placement.height, 12, "2 个 sixel 行 = 12 像素高");
    }

    /// `$` 回车：在同一 sixel 行内回到列首
    #[test]
    fn test_sixel_carriage_return() {
        // `~~$~`：先 2 列黑，$ 回到列首，再 1 列 → 最大宽度仍为 2
        let seq = b"\x1bPq~~$~\x1b\\";
        let placement = parse_sixel(seq).expect("应解析成功");
        assert_eq!(placement.width, 2, "最大宽度为 2");
        assert_eq!(placement.height, 6);
    }

    /// BEL 终止符
    #[test]
    fn test_sixel_bel_terminator() {
        let seq = b"\x1bPq#0;2;0;100;0#0~\x07";
        let placement = parse_sixel(seq).expect("应解析成功");
        assert_eq!(placement.width, 1);
        assert_eq!(placement.height, 6);
        // 寄存器 0 = (0, 255, 0) 绿色
        let idx = 0;
        assert_eq!(placement.rgba[idx], 0, "R");
        assert_eq!(placement.rgba[idx + 1], 255, "G");
        assert_eq!(placement.rgba[idx + 2], 0, "B");
        assert_eq!(placement.rgba[idx + 3], 255, "A");
    }

    /// 无效输入返回 None
    #[test]
    fn test_sixel_invalid_input() {
        assert!(parse_sixel(b"").is_none());
        assert!(parse_sixel(b"\x1bP").is_none()); // 无 q
        assert!(parse_sixel(b"\x1bPq").is_none()); // 无 sixel 数据
        assert!(parse_sixel(b"\x1b[31m").is_none()); // CSI 不是 DCS（无 q）
    }

    /// 解析 16 色默认调色板：选择寄存器 1（红色）
    #[test]
    fn test_sixel_default_palette() {
        let seq = b"\x1bPq#1~\x1b\\";
        let placement = parse_sixel(seq).expect("应解析成功");
        assert_eq!(placement.width, 1);
        assert_eq!(placement.height, 6);
        // 寄存器 1 = ANSI red = (205, 0, 0)
        let idx = 0;
        assert_eq!(placement.rgba[idx], 205, "R");
        assert_eq!(placement.rgba[idx + 1], 0, "G");
        assert_eq!(placement.rgba[idx + 2], 0, "B");
        assert_eq!(placement.rgba[idx + 3], 255, "A");
    }
}
