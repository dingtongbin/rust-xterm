//! Windows Terminal 默认主题（Campbell 配色）
//!
//! 提供与 Windows Terminal 默认观感持平的配色方案。
//!
//! ## Campbell 配色
//!
//! Windows Terminal 的默认配色方案，源自 Windows 10 的控制台配色，
//! 经过精心调校，在深色背景下提供良好的对比度和可读性。
//!
//! ## 光标样式
//!
//! Windows Terminal 默认使用 BlinkingBlock（闪烁块状光标）。

use wezterm_term::color::{ColorPalette, Palette256, SrgbaTuple};

/// 将 8-bit RGB 转换为 SrgbaTuple（0.0-1.0 浮点）
const fn rgb(r: u8, g: u8, b: u8) -> SrgbaTuple {
    SrgbaTuple(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0)
}

/// Windows Terminal 默认主题
#[derive(Debug, Clone)]
pub struct WindowsTerminalTheme {
    /// Campbell 配色方案
    pub palette: ColorPalette,
    /// 默认前景色
    pub foreground: SrgbaTuple,
    /// 默认背景色
    pub background: SrgbaTuple,
    /// 默认光标颜色
    pub cursor: SrgbaTuple,
    /// 默认选择背景色
    pub selection: SrgbaTuple,
}

impl Default for WindowsTerminalTheme {
    fn default() -> Self {
        Self::campbell()
    }
}

impl WindowsTerminalTheme {
    /// 创建 Campbell 主题（Windows Terminal 默认）
    pub fn campbell() -> Self {
        // Campbell 16 色配色方案
        // 来源: https://docs.microsoft.com/en-us/windows/terminal/customize-settings/color-schemes
        let campbell: [SrgbaTuple; 256] = {
            let mut arr = [rgb(0, 0, 0); 256];
            // 0-15: 标准 16 色
            arr[0] = rgb(0x0C, 0x0C, 0x0C); // Black
            arr[1] = rgb(0xC5, 0x0F, 0x0F); // DarkRed
            arr[2] = rgb(0x13, 0xA1, 0x0E); // DarkGreen
            arr[3] = rgb(0xC1, 0x9C, 0x00); // DarkYellow
            arr[4] = rgb(0x00, 0x37, 0xDA); // DarkBlue
            arr[5] = rgb(0x88, 0x17, 0x98); // DarkMagenta
            arr[6] = rgb(0x3A, 0x96, 0xDD); // DarkCyan
            arr[7] = rgb(0xCC, 0xCC, 0xCC); // Gray
            arr[8] = rgb(0x76, 0x76, 0x76); // DarkGray
            arr[9] = rgb(0xE7, 0x48, 0x56); // Red
            arr[10] = rgb(0x16, 0xC6, 0x0C); // Green
            arr[11] = rgb(0xF9, 0xF1, 0xA5); // Yellow
            arr[12] = rgb(0x3B, 0x78, 0xFF); // Blue
            arr[13] = rgb(0xB4, 0x00, 0x9E); // Magenta
            arr[14] = rgb(0x61, 0xD6, 0xD6); // Cyan
            arr[15] = rgb(0xF2, 0xF2, 0xF2); // White
                                             // 16-231: 6x6x6 立方体（标准 xterm 256 色公式）
            let levels = [0, 95, 135, 175, 215, 255];
            let mut idx = 16;
            for r in 0..6 {
                for g in 0..6 {
                    for b in 0..6 {
                        arr[idx] = rgb(levels[r], levels[g], levels[b]);
                        idx += 1;
                    }
                }
            }
            // 232-255: 灰度渐变（24 级）
            for i in 0u8..24 {
                let v = 8 + i * 10;
                arr[232 + i as usize] = rgb(v, v, v);
            }
            arr
        };

        let palette = ColorPalette {
            colors: Palette256(campbell),
            foreground: rgb(0xCC, 0xCC, 0xCC),
            background: rgb(0x0C, 0x0C, 0x0C),
            cursor_fg: rgb(0x0C, 0x0C, 0x0C),
            cursor_bg: rgb(0xC0, 0xC0, 0xC0),
            cursor_border: rgb(0xC0, 0xC0, 0xC0),
            selection_fg: rgb(0x0C, 0x0C, 0x0C),
            selection_bg: rgb(0xC0, 0xC0, 0xC0),
            scrollbar_thumb: rgb(0x76, 0x76, 0x76),
            split: rgb(0x76, 0x76, 0x76),
        };

        Self {
            foreground: rgb(0xCC, 0xCC, 0xCC),
            background: rgb(0x0C, 0x0C, 0x0C),
            cursor: rgb(0xC0, 0xC0, 0xC0),
            selection: rgb(0xC0, 0xC0, 0xC0),
            palette,
        }
    }

    /// 创建 Vintage 主题（经典 Windows 控制台）
    pub fn vintage() -> Self {
        let mut theme = Self::campbell();
        // 经典黑底白字
        theme.foreground = rgb(0xC0, 0xC0, 0xC0);
        theme.background = rgb(0x00, 0x00, 0x00);
        theme.palette.foreground = rgb(0xC0, 0xC0, 0xC0);
        theme.palette.background = rgb(0x00, 0x00, 0x00);
        theme
    }

    /// 将主题应用到 WezTerm ColorPalette
    pub fn to_palette(&self) -> ColorPalette {
        self.palette.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_campbell_theme() {
        let theme = WindowsTerminalTheme::campbell();
        let palette = theme.to_palette();

        // 验证 Campbell 黑色
        let black = palette.colors.0[0];
        assert!((black.0 - 0x0C as f32 / 255.0).abs() < 0.01);

        // 验证 Campbell 白色
        let white = palette.colors.0[15];
        assert!((white.0 - 0xF2 as f32 / 255.0).abs() < 0.01);

        // 验证前景色
        let fg = palette.foreground;
        assert!((fg.0 - 0xCC as f32 / 255.0).abs() < 0.01);

        // 验证背景色
        let bg = palette.background;
        assert!((bg.0 - 0x0C as f32 / 255.0).abs() < 0.01);
    }

    #[test]
    fn test_vintage_theme() {
        let theme = WindowsTerminalTheme::vintage();
        let palette = theme.to_palette();

        let bg = palette.background;
        assert!((bg.0 - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_256_color_cubic() {
        let theme = WindowsTerminalTheme::campbell();
        let palette = theme.to_palette();

        // 验证 256 色立方体（索引 16-231）
        // 索引 16 = (0,0,0) = rgb(0,0,0)
        let c16 = palette.colors.0[16];
        assert!((c16.0 - 0.0).abs() < 0.01);

        // 索引 231 = (5,5,5) = rgb(255,255,255)
        let c231 = palette.colors.0[231];
        assert!((c231.0 - 1.0).abs() < 0.01);
    }
}
