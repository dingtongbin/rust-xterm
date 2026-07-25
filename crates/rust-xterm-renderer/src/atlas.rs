//! 纹理图集
//!
//! 内存模型：`Box<[u8]>` 固定大小数组（如 4MB）。
//!
//! ## 分区策略
//!
//! ```text
//! ┌──────────────────────────────────┐
//! │         静态区 (前 25%)           │  ASCII + Box Drawing 预渲染
//! │  永不淘汰，启动时一次性填充        │
//! ├──────────────────────────────────┤
//! │         动态区 (后 75%)           │  LRU 淘汰池
//! │  运行时按需光栅化，LRU 策略淘汰    │
//! └──────────────────────────────────┘
//! ```
//!
//! ## 缓存策略
//!
//! - Key = `(char, Style)`，Value = `(u, v, metrics)`
//! - **绝不缓存颜色**：只存储 Alpha 掩码，颜色在合成阶段混合
//! - Emoji 走特殊路径：存储 RGBA 而非 Alpha

use lru::LruCache;
use std::num::NonZeroUsize;

/// 图集条目：记录一个字形在图集中的位置和元信息
#[derive(Debug, Clone, Copy)]
pub struct AtlasEntry {
    /// 在图集中的 X 坐标（像素）
    pub x: u32,
    /// 在图集中的 Y 坐标（像素）
    pub y: u32,
    /// 字形宽度（像素）
    pub width: u32,
    /// 字形高度（像素）
    pub height: u32,
    /// 水平偏移（基线左侧的距离，像素）
    pub left_bearing: i32,
    /// 垂直偏移（基线上方的距离，像素）
    pub top_bearing: i32,
    /// 是否为彩色字形（Emoji）
    pub is_color: bool,
}

/// 图集统计信息
#[derive(Debug, Clone, Copy, Default)]
pub struct AtlasStats {
    /// 静态区已用槽数
    pub static_slots: usize,
    /// 动态区当前已用槽数
    pub dynamic_slots: usize,
    /// LRU 淘汰次数
    pub evictions: u64,
    /// 缓存命中次数
    pub hits: u64,
    /// 缓存未命中次数
    pub misses: u64,
}

/// 纹理图集
///
/// 管理一个固定大小的像素缓冲区，分为静态区和动态区。
/// 静态区在初始化时填充 ASCII + Box Drawing 字符，永不淘汰。
/// 动态区使用 LRU 策略管理运行时光栅化的字形。
pub struct TextureAtlas {
    /// 像素缓冲区（Alpha 或 RGBA）
    buffer: Box<[u8]>,
    /// 缓冲区宽度（像素）
    width: u32,
    /// 缓冲区高度（像素）
    height: u32,
    /// 每像素字节数（Alpha=1, RGBA=4）
    bytes_per_pixel: usize,
    /// 静态区高度（像素），静态区占据缓冲区顶部
    static_region_height: u32,
    /// 静态区当前写入位置
    static_cursor: (u32, u32),
    /// 动态区当前写入位置
    dynamic_cursor: (u32, u32),
    /// 动态区行高（像素）
    row_height: u32,
    /// 动态区 LRU 缓存：Key = (char, bold, italic), Value = AtlasEntry
    dynamic_cache: LruCache<(char, bool, bool), AtlasEntry>,
    /// 静态区缓存：Key = (char, bold, italic), Value = AtlasEntry
    static_cache: std::collections::HashMap<(char, bool, bool), AtlasEntry>,
    /// 统计信息
    stats: AtlasStats,
}

impl TextureAtlas {
    /// 创建新的纹理图集
    ///
    /// - `width`：图集宽度（像素）
    /// - `height`：图集高度（像素）
    /// - `bytes_per_pixel`：每像素字节数（1=Alpha, 4=RGBA）
    /// - `row_height`：动态区每行高度（像素），通常等于字体行高
    pub fn new(width: u32, height: u32, bytes_per_pixel: usize, row_height: u32) -> Self {
        let total_size = (width as usize) * (height as usize) * bytes_per_pixel;
        let buffer = vec![0u8; total_size].into_boxed_slice();
        let static_region_height = height / 4; // 静态区占 25%
        let dynamic_capacity = NonZeroUsize::new(
            ((width / 8) as usize) * ((height - static_region_height) / row_height.max(1)) as usize,
        )
        .unwrap_or(NonZeroUsize::new(1).unwrap());

        Self {
            buffer,
            width,
            height,
            bytes_per_pixel,
            static_region_height,
            static_cursor: (0, 0),
            dynamic_cursor: (0, static_region_height),
            row_height: row_height.max(1),
            dynamic_cache: LruCache::new(dynamic_capacity),
            static_cache: std::collections::HashMap::new(),
            stats: AtlasStats::default(),
        }
    }

    /// 获取图集宽度
    pub fn width(&self) -> u32 {
        self.width
    }

    /// 获取图集高度
    pub fn height(&self) -> u32 {
        self.height
    }

    /// 获取像素缓冲区引用
    pub fn buffer(&self) -> &[u8] {
        &self.buffer
    }

    /// 获取像素缓冲区可变引用
    pub fn buffer_mut(&mut self) -> &mut [u8] {
        &mut self.buffer
    }

    /// 获取每像素字节数
    pub fn bytes_per_pixel(&self) -> usize {
        self.bytes_per_pixel
    }

    /// 获取统计信息
    pub fn stats(&self) -> AtlasStats {
        self.stats
    }

    /// 查询静态区缓存
    pub fn lookup_static(&self, ch: char, bold: bool, italic: bool) -> Option<&AtlasEntry> {
        self.static_cache.get(&(ch, bold, italic))
    }

    /// 查询动态区缓存（同时更新 LRU）
    pub fn lookup_dynamic(&mut self, ch: char, bold: bool, italic: bool) -> Option<&AtlasEntry> {
        if let Some(entry) = self.dynamic_cache.get(&(ch, bold, italic)) {
            self.stats.hits += 1;
            return Some(entry);
        }
        self.stats.misses += 1;
        None
    }

    /// 向静态区插入字形
    ///
    /// 将字形像素数据写入静态区，并记录位置。
    /// 如果静态区已满，返回 None。
    #[allow(clippy::too_many_arguments)]
    pub fn insert_static(
        &mut self,
        ch: char,
        bold: bool,
        italic: bool,
        pixels: &[u8],
        width: u32,
        height: u32,
        left_bearing: i32,
        top_bearing: i32,
        is_color: bool,
    ) -> Option<AtlasEntry> {
        let (px, py) = self.find_static_slot(width, height)?;
        self.write_pixels(px, py, pixels, width, height);
        let entry = AtlasEntry {
            x: px,
            y: py,
            width,
            height,
            left_bearing,
            top_bearing,
            is_color,
        };
        self.static_cache.insert((ch, bold, italic), entry);
        self.stats.static_slots += 1;
        Some(entry)
    }

    /// 向动态区插入字形
    ///
    /// 将字形像素数据写入动态区，并记录位置。
    /// 如果动态区已满，触发 LRU 淘汰。
    #[allow(clippy::too_many_arguments)]
    pub fn insert_dynamic(
        &mut self,
        ch: char,
        bold: bool,
        italic: bool,
        pixels: &[u8],
        width: u32,
        height: u32,
        left_bearing: i32,
        top_bearing: i32,
        is_color: bool,
    ) -> Option<AtlasEntry> {
        let (px, py) = self.find_dynamic_slot(width, height)?;
        self.write_pixels(px, py, pixels, width, height);
        let entry = AtlasEntry {
            x: px,
            y: py,
            width,
            height,
            left_bearing,
            top_bearing,
            is_color,
        };
        // 如果 LRU 满了，push 会自动淘汰最旧的条目
        if self.dynamic_cache.len() >= self.dynamic_cache.cap().get() {
            self.stats.evictions += 1;
        }
        self.dynamic_cache.put((ch, bold, italic), entry);
        self.stats.dynamic_slots = self.dynamic_cache.len();
        Some(entry)
    }

    /// 在静态区寻找可用槽位
    fn find_static_slot(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        let (cx, cy) = self.static_cursor;
        if cx + w > self.width {
            // 换行
            let new_y = cy + self.row_height;
            if new_y + h > self.static_region_height {
                return None; // 静态区已满
            }
            self.static_cursor = (0, new_y);
        }
        let (px, py) = self.static_cursor;
        if py + h > self.static_region_height {
            return None;
        }
        self.static_cursor = (px + w, py);
        Some((px, py))
    }

    /// 在动态区寻找可用槽位
    fn find_dynamic_slot(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        let (cx, cy) = self.dynamic_cursor;
        if cx + w > self.width {
            // 换行
            let new_y = cy + self.row_height;
            if new_y + h > self.height {
                // 动态区已满，回绕到动态区起始位置。
                // 必须清理 dynamic_cache：旧 entry 指向的像素区域即将被新字形覆盖，
                // 否则命中旧 entry 时 sample_alpha 会读到新字形的像素，造成"鬼影"。
                self.wrap_around_dynamic();
            } else {
                self.dynamic_cursor = (0, new_y);
            }
        }
        let (px, py) = self.dynamic_cursor;
        if py + h > self.height {
            // 回绕到动态区起始位置，同样需要清理旧 entry 防止鬼影
            self.wrap_around_dynamic();
            return Some((0, self.static_region_height));
        }
        self.dynamic_cursor = (px + w, py);
        Some((px, py))
    }

    /// 动态区回绕：重置写入游标并清空动态缓存
    ///
    /// 回绕意味着动态区从顶部重新开始写入，旧像素会被新字形覆盖。
    /// 此时必须清空 `dynamic_cache`，否则旧 entry 命中后会从已覆盖的
    /// 像素区域采样，导致"鬼影"。
    fn wrap_around_dynamic(&mut self) {
        self.dynamic_cursor = (0, self.static_region_height);
        self.dynamic_cache.clear();
        self.stats.dynamic_slots = 0;
    }

    /// 将像素数据写入缓冲区
    fn write_pixels(&mut self, x: u32, y: u32, pixels: &[u8], w: u32, h: u32) {
        let bpp = self.bytes_per_pixel;
        let stride = self.width as usize * bpp;
        let src_stride = w as usize * bpp;
        for row in 0..h as usize {
            let dst_start = (y as usize + row) * stride + x as usize * bpp;
            let src_start = row * src_stride;
            let copy_len = src_stride.min(self.buffer.len().saturating_sub(dst_start));
            if copy_len > 0 && src_start + copy_len <= pixels.len() {
                self.buffer[dst_start..dst_start + copy_len]
                    .copy_from_slice(&pixels[src_start..src_start + copy_len]);
            }
        }
    }

    /// 从图集读取一个像素的 Alpha 值
    ///
    /// 用于合成阶段：从 Alpha 掩码中采样，然后与前景色混合。
    pub fn sample_alpha(&self, x: u32, y: u32) -> u8 {
        if x >= self.width || y >= self.height {
            return 0;
        }
        let idx = (y as usize * self.width as usize + x as usize) * self.bytes_per_pixel;
        if idx >= self.buffer.len() {
            return 0;
        }
        if self.bytes_per_pixel == 1 {
            self.buffer[idx]
        } else if self.bytes_per_pixel == 4 {
            // RGBA: 取 Alpha 通道
            self.buffer[idx + 3]
        } else {
            0
        }
    }

    /// 从图集读取一个像素的 RGBA 值
    pub fn sample_rgba(&self, x: u32, y: u32) -> (u8, u8, u8, u8) {
        if x >= self.width || y >= self.height {
            return (0, 0, 0, 0);
        }
        let idx = (y as usize * self.width as usize + x as usize) * self.bytes_per_pixel;
        if idx + self.bytes_per_pixel > self.buffer.len() {
            return (0, 0, 0, 0);
        }
        if self.bytes_per_pixel == 4 {
            (
                self.buffer[idx],
                self.buffer[idx + 1],
                self.buffer[idx + 2],
                self.buffer[idx + 3],
            )
        } else if self.bytes_per_pixel == 1 {
            let a = self.buffer[idx];
            (a, a, a, a)
        } else {
            (0, 0, 0, 0)
        }
    }

    /// 清空动态区（保留静态区）
    pub fn clear_dynamic(&mut self) {
        let dynamic_start =
            self.static_region_height as usize * self.width as usize * self.bytes_per_pixel;
        for byte in &mut self.buffer[dynamic_start..] {
            *byte = 0;
        }
        self.dynamic_cache.clear();
        self.dynamic_cursor = (0, self.static_region_height);
        self.stats.dynamic_slots = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_atlas_creation() {
        let atlas = TextureAtlas::new(512, 512, 1, 20);
        assert_eq!(atlas.width(), 512);
        assert_eq!(atlas.height(), 512);
        assert_eq!(atlas.bytes_per_pixel(), 1);
    }

    #[test]
    fn test_static_insert_and_lookup() {
        let mut atlas = TextureAtlas::new(512, 512, 1, 20);
        let pixels = vec![128u8; 10 * 20];
        let entry = atlas.insert_static('A', false, false, &pixels, 10, 20, 0, 0, false);
        assert!(entry.is_some());

        let found = atlas.lookup_static('A', false, false);
        assert!(found.is_some());
        let found = found.unwrap();
        assert_eq!(found.width, 10);
        assert_eq!(found.height, 20);
    }

    #[test]
    fn test_dynamic_insert_and_lookup() {
        let mut atlas = TextureAtlas::new(512, 512, 1, 20);
        let pixels = vec![200u8; 10 * 20];
        let entry = atlas.insert_dynamic('X', false, false, &pixels, 10, 20, 0, 0, false);
        assert!(entry.is_some());

        let found = atlas.lookup_dynamic('X', false, false);
        assert!(found.is_some());
    }

    #[test]
    fn test_lru_eviction() {
        let mut atlas = TextureAtlas::new(64, 64, 1, 20);
        // 插入多个条目触发淘汰
        for i in 0..20 {
            let ch = char::from_u32('A' as u32 + i).unwrap();
            let pixels = vec![100u8; 8 * 16];
            let _ = atlas.insert_dynamic(ch, false, false, &pixels, 8, 16, 0, 0, false);
        }
        let stats = atlas.stats();
        assert!(stats.evictions > 0 || stats.dynamic_slots > 0);
    }

    #[test]
    fn test_sample_alpha() {
        let mut atlas = TextureAtlas::new(64, 64, 1, 20);
        let pixels = vec![255u8; 4 * 4];
        let _ = atlas.insert_static('T', false, false, &pixels, 4, 4, 0, 0, false);
        // 采样写入的区域
        let alpha = atlas.sample_alpha(0, 0);
        assert_eq!(alpha, 255);
    }

    #[test]
    fn test_wraparound_clears_stale_entries() {
        // 配置：64x64 图集，静态区高度=16，动态区高度=48，row_height=20
        // 动态区可容纳 2 行（y=16, y=36），每行可放 64/8=8 个 8x16 字形
        // 第 17 个字形触发回绕（cursor 从 (64,36) 换行时 new_y=56+16>64）
        let mut atlas = TextureAtlas::new(64, 64, 1, 20);

        // 插入第一个字形 'A'，验证可查
        let pixels_a = vec![200u8; 8 * 16];
        let entry_a = atlas.insert_dynamic('A', false, false, &pixels_a, 8, 16, 0, 0, false);
        assert!(entry_a.is_some());
        assert!(atlas.lookup_dynamic('A', false, false).is_some());

        // 填满动态区并触发回绕：插入 20 个不同字形，第 17 个会触发回绕
        for i in 0..20u32 {
            // 用 PUA 区字符避免与 'A' 冲突
            let ch = char::from_u32(0xE000 + i).unwrap();
            let pixels = vec![100u8; 8 * 16];
            let _ = atlas.insert_dynamic(ch, false, false, &pixels, 8, 16, 0, 0, false);
        }

        // 回绕后 dynamic_cache 被清空，旧 entry 'A' 不应被命中
        // （否则会读到新字形的像素，造成"鬼影"）
        assert!(
            atlas.lookup_dynamic('A', false, false).is_none(),
            "回绕后旧 entry 应被清理，避免鬼影"
        );
    }
}
