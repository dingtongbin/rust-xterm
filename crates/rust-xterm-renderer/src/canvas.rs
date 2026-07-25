//! 像素缓冲区
//!
//! 渲染目标画布，存储最终合成的 RGBA 像素数据。
//!
//! ## 设计
//!
//! - 使用 `Box<[u8]>` 固定大小数组
//! - 支持 RGBA 格式（4 字节/像素）
//! - 提供混合写入接口，用于字形合成

/// 像素格式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// RGBA 格式（4 字节/像素）
    Rgba,
    /// BGRA 格式（4 字节/像素，用于某些 GPU 后端）
    Bgra,
}

impl PixelFormat {
    /// 每像素字节数
    pub fn bytes_per_pixel(&self) -> usize {
        4
    }
}

/// 像素画布
///
/// 存储渲染结果的 RGBA 像素缓冲区。
pub struct Canvas {
    /// 像素数据
    buffer: Box<[u8]>,
    /// 宽度（像素）
    width: u32,
    /// 高度（像素）
    height: u32,
    /// 像素格式
    format: PixelFormat,
}

impl Canvas {
    /// 创建新的画布
    pub fn new(width: u32, height: u32, format: PixelFormat) -> Self {
        let bpp = format.bytes_per_pixel();
        let size = width as usize * height as usize * bpp;
        let buffer = vec![0u8; size].into_boxed_slice();
        Self {
            buffer,
            width,
            height,
            format,
        }
    }

    /// 获取宽度
    pub fn width(&self) -> u32 {
        self.width
    }

    /// 获取高度
    pub fn height(&self) -> u32 {
        self.height
    }

    /// 获取像素格式
    pub fn format(&self) -> PixelFormat {
        self.format
    }

    /// 获取像素缓冲区引用
    pub fn buffer(&self) -> &[u8] {
        &self.buffer
    }

    /// 获取像素缓冲区可变引用
    pub fn buffer_mut(&mut self) -> &mut [u8] {
        &mut self.buffer
    }

    /// 清空画布为指定颜色
    pub fn clear(&mut self, r: u8, g: u8, b: u8, a: u8) {
        for chunk in self.buffer.chunks_exact_mut(4) {
            chunk[0] = r;
            chunk[1] = g;
            chunk[2] = b;
            chunk[3] = a;
        }
    }

    /// 写入一个 RGBA 像素
    #[inline]
    pub fn put_pixel(&mut self, x: u32, y: u32, r: u8, g: u8, b: u8, a: u8) {
        if x >= self.width || y >= self.height {
            return;
        }
        let idx = (y as usize * self.width as usize + x as usize) * 4;
        if idx + 3 >= self.buffer.len() {
            return;
        }
        match self.format {
            PixelFormat::Rgba => {
                self.buffer[idx] = r;
                self.buffer[idx + 1] = g;
                self.buffer[idx + 2] = b;
                self.buffer[idx + 3] = a;
            }
            PixelFormat::Bgra => {
                self.buffer[idx] = b;
                self.buffer[idx + 1] = g;
                self.buffer[idx + 2] = r;
                self.buffer[idx + 3] = a;
            }
        }
    }

    /// 读取一个像素的 RGBA 值
    #[inline]
    pub fn get_pixel(&self, x: u32, y: u32) -> (u8, u8, u8, u8) {
        if x >= self.width || y >= self.height {
            return (0, 0, 0, 0);
        }
        let idx = (y as usize * self.width as usize + x as usize) * 4;
        if idx + 3 >= self.buffer.len() {
            return (0, 0, 0, 0);
        }
        match self.format {
            PixelFormat::Rgba => (
                self.buffer[idx],
                self.buffer[idx + 1],
                self.buffer[idx + 2],
                self.buffer[idx + 3],
            ),
            PixelFormat::Bgra => (
                self.buffer[idx + 2],
                self.buffer[idx + 1],
                self.buffer[idx],
                self.buffer[idx + 3],
            ),
        }
    }

    /// Alpha 混合写入一个像素
    ///
    /// 将前景色按 alpha 值混合到目标像素上。
    #[inline]
    pub fn blend_pixel(&mut self, x: u32, y: u32, r: u8, g: u8, b: u8, a: u8) {
        if a == 0 {
            return;
        }
        let (dr, dg, db, da) = self.get_pixel(x, y);
        if a == 255 {
            self.put_pixel(x, y, r, g, b, da.max(a));
            return;
        }
        let alpha = a as u32;
        let inv_alpha = 255 - alpha;
        let nr = (r as u32 * alpha + dr as u32 * inv_alpha) / 255;
        let ng = (g as u32 * alpha + dg as u32 * inv_alpha) / 255;
        let nb = (b as u32 * alpha + db as u32 * inv_alpha) / 255;
        let na = da.max(a);
        self.put_pixel(x, y, nr as u8, ng as u8, nb as u8, na);
    }

    /// 填充矩形区域
    #[allow(clippy::too_many_arguments)]
    pub fn fill_rect(&mut self, x: u32, y: u32, w: u32, h: u32, r: u8, g: u8, b: u8, a: u8) {
        for py in y..(y + h).min(self.height) {
            for px in x..(x + w).min(self.width) {
                self.put_pixel(px, py, r, g, b, a);
            }
        }
    }

    /// 调整画布大小
    pub fn resize(&mut self, width: u32, height: u32) {
        if self.width == width && self.height == height {
            return;
        }
        let bpp = self.format.bytes_per_pixel();
        let size = width as usize * height as usize * bpp;
        self.buffer = vec![0u8; size].into_boxed_slice();
        self.width = width;
        self.height = height;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canvas_creation() {
        let canvas = Canvas::new(100, 50, PixelFormat::Rgba);
        assert_eq!(canvas.width(), 100);
        assert_eq!(canvas.height(), 50);
        assert_eq!(canvas.buffer().len(), 100 * 50 * 4);
    }

    #[test]
    fn test_put_get_pixel() {
        let mut canvas = Canvas::new(10, 10, PixelFormat::Rgba);
        canvas.put_pixel(5, 5, 255, 128, 64, 255);
        let (r, g, b, a) = canvas.get_pixel(5, 5);
        assert_eq!((r, g, b, a), (255, 128, 64, 255));
    }

    #[test]
    fn test_blend_pixel() {
        let mut canvas = Canvas::new(10, 10, PixelFormat::Rgba);
        canvas.put_pixel(0, 0, 0, 0, 0, 255);
        canvas.blend_pixel(0, 0, 255, 255, 255, 128);
        let (r, g, b, _) = canvas.get_pixel(0, 0);
        assert_eq!(r, 128);
        assert_eq!(g, 128);
        assert_eq!(b, 128);
    }

    #[test]
    fn test_fill_rect() {
        let mut canvas = Canvas::new(20, 20, PixelFormat::Rgba);
        canvas.fill_rect(5, 5, 10, 10, 255, 0, 0, 255);
        let (r, _, _, _) = canvas.get_pixel(10, 10);
        assert_eq!(r, 255);
    }

    #[test]
    fn test_resize() {
        let mut canvas = Canvas::new(10, 10, PixelFormat::Rgba);
        canvas.resize(20, 20);
        assert_eq!(canvas.width(), 20);
        assert_eq!(canvas.height(), 20);
        assert_eq!(canvas.buffer().len(), 20 * 20 * 4);
    }
}
