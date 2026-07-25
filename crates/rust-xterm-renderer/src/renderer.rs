//! 渲染引擎
//!
//! 光栅化 + 合成 + 脏区输出。
//!
//! ## 渲染管线
//!
//! 1. 遍历脏区 Cell
//! 2. 查 Atlas -> 若 Miss，调用 swash 光栅化 -> 写入 Atlas -> 更新索引
//! 3. 合成：读取 Cell 前景色/背景色，从 Atlas 取 Alpha 掩码，混合写入像素缓冲区
//! 4. 输出：返回 `RenderResult { dirty_rects, cursor_meta }`
//!
//! ## 视觉特性
//!
//! - **回绕**：WezTerm 核心内置处理，渲染器仅负责渲染折行后的 Grid
//! - **Undercurl**：正弦波算法实时绘制，参数 (Amplitude=2px, Period=auto)
//! - **Emoji**：检测 Unicode Range -> 走 ColorGlyph 路径 -> 直接写入 RGBA
//! - **CJK 宽字符**：WezTerm 提供 width 属性，渲染器绘制 width * cell_width 宽度的字形

use crate::atlas::{AtlasEntry, TextureAtlas};
use crate::canvas::{Canvas, PixelFormat};
use crate::font_tree::FontTree;
use rust_xterm_core::{CellFlags, Color, CursorMeta, CursorShape, RustXtermCell};

/// 渲染度量信息
#[derive(Debug, Clone, Copy)]
pub struct RenderMetrics {
    /// 单元格宽度（像素）
    pub cell_width: u32,
    /// 单元格高度（像素）
    pub cell_height: u32,
    /// 基线 Y 坐标（相对于单元格顶部）
    pub baseline: u32,
    /// 每英寸像素数
    pub dpi: f32,
    /// 字体大小（像素）
    pub font_size: f32,
}

impl Default for RenderMetrics {
    fn default() -> Self {
        Self {
            cell_width: 8,
            cell_height: 16,
            baseline: 13,
            dpi: 96.0,
            font_size: 14.0,
        }
    }
}

/// 渲染器配置
#[derive(Debug, Clone)]
pub struct RendererConfig {
    /// 渲染度量
    pub metrics: RenderMetrics,
    /// 图集宽度
    pub atlas_width: u32,
    /// 图集高度
    pub atlas_height: u32,
    /// 画布宽度
    pub canvas_width: u32,
    /// 画布高度
    pub canvas_height: u32,
    /// 默认前景色
    pub default_fg: Color,
    /// 默认背景色
    pub default_bg: Color,
}

impl Default for RendererConfig {
    fn default() -> Self {
        Self {
            metrics: RenderMetrics::default(),
            atlas_width: 1024,
            atlas_height: 1024,
            canvas_width: 640,
            canvas_height: 384,
            default_fg: Color::WHITE,
            default_bg: Color::BLACK,
        }
    }
}

/// 渲染结果
#[derive(Debug, Clone)]
pub struct RenderResult {
    /// 脏矩形列表
    pub dirty_rects: Vec<(u32, u32, u32, u32)>,
    /// 光标元信息
    pub cursor: Option<CursorMeta>,
}

/// 渲染引擎
///
/// 协调纹理图集、字体树和画布，完成终端内容的渲染。
pub struct Renderer {
    /// 渲染器配置
    config: RendererConfig,
    /// 纹理图集
    atlas: TextureAtlas,
    /// 字体树
    font_tree: FontTree,
    /// 像素画布
    canvas: Canvas,
    /// 渲染统计
    stats: RenderStats,
}

/// 渲染统计
#[derive(Debug, Clone, Copy, Default)]
pub struct RenderStats {
    /// 光栅化次数
    pub rasterizations: u64,
    /// 合成次数
    pub composites: u64,
    /// 图集命中次数
    pub atlas_hits: u64,
    /// 图集未命中次数
    pub atlas_misses: u64,
}

impl Renderer {
    /// 创建新的渲染器
    pub fn new(config: RendererConfig) -> Self {
        let atlas = TextureAtlas::new(config.atlas_width, config.atlas_height, 1, 1024);
        let canvas = Canvas::new(config.canvas_width, config.canvas_height, PixelFormat::Rgba);
        let font_tree = FontTree::new();

        Self {
            config,
            atlas,
            font_tree,
            canvas,
            stats: RenderStats::default(),
        }
    }

    /// 获取渲染器配置引用
    pub fn config(&self) -> &RendererConfig {
        &self.config
    }

    /// 获取纹理图集引用
    pub fn atlas(&self) -> &TextureAtlas {
        &self.atlas
    }

    /// 获取纹理图集可变引用
    pub fn atlas_mut(&mut self) -> &mut TextureAtlas {
        &mut self.atlas
    }

    /// 获取字体树引用
    pub fn font_tree(&self) -> &FontTree {
        &self.font_tree
    }

    /// 获取字体树可变引用
    pub fn font_tree_mut(&mut self) -> &mut FontTree {
        &mut self.font_tree
    }

    /// 获取画布引用
    pub fn canvas(&self) -> &Canvas {
        &self.canvas
    }

    /// 获取画布可变引用
    pub fn canvas_mut(&mut self) -> &mut Canvas {
        &mut self.canvas
    }

    /// 获取渲染统计
    pub fn stats(&self) -> RenderStats {
        self.stats
    }

    /// 渲染单行 Cell
    ///
    /// 将一行 Cell 渲染到画布上，返回脏矩形 (x, y, width, height)。
    pub fn render_row(&mut self, row: u32, cells: &[RustXtermCell]) -> (u32, u32, u32, u32) {
        let metrics = self.config.metrics;
        let y = row * metrics.cell_height;
        let max_width = self.canvas.width();

        // 先填充背景色
        let mut x = 0u32;
        for cell in cells {
            let cell_pixel_width = (cell.width as u32) * metrics.cell_width;
            if x >= max_width {
                break;
            }

            // 填充背景
            let bg = if cell.flags.contains(CellFlags::REVERSE) {
                cell.fg
            } else {
                cell.bg
            };
            self.canvas.fill_rect(
                x,
                y,
                cell_pixel_width.min(max_width - x),
                metrics.cell_height,
                bg.r,
                bg.g,
                bg.b,
                bg.a,
            );

            x += cell_pixel_width;
        }

        // 再渲染前景文本
        x = 0;
        for cell in cells {
            let cell_pixel_width = (cell.width as u32) * metrics.cell_width;
            if x >= max_width {
                break;
            }

            if !cell.is_blank() {
                self.render_cell_text(x, y, cell);
            }

            // 渲染装饰（下划线、删除线等）
            self.render_decorations(x, y, cell);

            x += cell_pixel_width;
        }

        self.stats.composites += 1;
        (0, y, max_width, metrics.cell_height)
    }

    /// 渲染一帧：仅渲染指定的脏行，并收集脏矩形
    ///
    /// 对 `dirty_rows` 中的每一行调用 `render_row`，将返回的矩形
    /// 累积到 `RenderResult.dirty_rects` 中返回给上层。
    ///
    /// # 参数
    ///
    /// - `dirty_rows`：需要重绘的行号列表（0-based）
    /// - `cells`：按行索引取用的 Cell 行切片集合（`cells[row]` 为该行的 Cell）
    ///
    /// # 返回
    ///
    /// 填充好 `dirty_rects` 的 `RenderResult`，每个脏行对应一个矩形
    /// `(x, y, width, height)`，其中 `y = row * cell_height`。
    pub fn render_frame(&mut self, dirty_rows: &[u32], cells: &[&[RustXtermCell]]) -> RenderResult {
        let mut result = RenderResult {
            dirty_rects: Vec::with_capacity(dirty_rows.len()),
            cursor: None,
        };
        for &row in dirty_rows {
            // 跳过超出 cells 范围的行号，避免越界
            let row_cells = match cells.get(row as usize) {
                Some(c) => c,
                None => continue,
            };
            let rect = self.render_row(row, row_cells);
            result.dirty_rects.push(rect);
        }
        result
    }

    /// 渲染单个 Cell 的文本
    fn render_cell_text(&mut self, x: u32, y: u32, cell: &RustXtermCell) {
        // 获取字符
        let ch = if let Some(ch) = cell.text.chars().next() {
            ch
        } else {
            return;
        };

        // 查找字形：传入 cell.width（WezTerm 权威宽度）与 is_color 提示。
        // 不再依赖 font_tree 内部的硬编码 Unicode 宽度判定。
        // is_color 默认 false：当前 CellFlags 没有 emoji 标志位，
        // 未来扩展时可由 cell flags 覆盖。
        let is_color_hint = false;
        let glyph_info = match self.font_tree.lookup_glyph(ch, cell.width, is_color_hint) {
            Some(info) => info,
            None => return,
        };

        // Task 12: 缺字画 notdef 方块
        // glyph_id == 0 表示 .notdef（所有字体都未覆盖该字符），
        // 此时画一个方块以提示缺字，而不是静默跳过留下空白。
        if glyph_info.glyph_id == 0 {
            self.render_missing_glyph_box(x, y, cell);
            return;
        }

        // 查找图集
        let bold = cell.flags.contains(CellFlags::BOLD);
        let italic = cell.flags.contains(CellFlags::ITALIC);
        let entry = self.atlas.lookup_dynamic(ch, bold, italic);

        let entry = if let Some(e) = entry {
            self.stats.atlas_hits += 1;
            *e
        } else {
            // Miss: 光栅化并插入图集
            self.stats.atlas_misses += 1;
            match self.rasterize_glyph(ch, glyph_info.is_color) {
                Some(pixels) => {
                    let entry = self.atlas.insert_dynamic(
                        ch,
                        bold,
                        italic,
                        &pixels.data,
                        pixels.width,
                        pixels.height,
                        pixels.left_bearing,
                        pixels.top_bearing,
                        pixels.is_color,
                    );
                    if let Some(e) = entry {
                        e
                    } else {
                        return;
                    }
                }
                None => return,
            }
        };

        // 合成到画布
        let fg = if cell.flags.contains(CellFlags::REVERSE) {
            cell.bg
        } else {
            cell.fg
        };

        self.composite_glyph(x, y, entry, fg, cell.flags.contains(CellFlags::DIM));
    }

    /// 渲染缺字方块（.notdef 占位符）
    ///
    /// 当 `lookup_glyph` 返回 `glyph_id == 0`（所有字体都未覆盖该字符）时调用。
    /// 用前景色在 cell 内画一个略小于 cell 的实心方块，提示缺字。
    fn render_missing_glyph_box(&mut self, x: u32, y: u32, cell: &RustXtermCell) {
        let metrics = self.config.metrics;
        let cell_w = (cell.width as u32) * metrics.cell_width;
        let cell_h = metrics.cell_height;

        let fg = if cell.flags.contains(CellFlags::REVERSE) {
            cell.bg
        } else {
            cell.fg
        };

        // 留 1px 边距，与背景区分
        let box_x = x.saturating_add(1);
        let box_y = y.saturating_add(1);
        let box_w = cell_w.saturating_sub(2);
        let box_h = cell_h.saturating_sub(2);
        self.canvas
            .fill_rect(box_x, box_y, box_w, box_h, fg.r, fg.g, fg.b, fg.a);
    }

    /// 光栅化字形
    fn rasterize_glyph(&mut self, ch: char, is_color: bool) -> Option<RasterizedGlyph> {
        let metrics = self.config.metrics;
        let ppem = metrics.font_size;

        // 获取字体数据
        // 注意：此处 width/is_color 仅用于复用缓存条目，实际渲染宽度由 cell 决定。
        // rasterize_glyph 仅在 render_cell_text 通过 notdef 检查后调用，
        // 故 glyph_id 必然 > 0，不会触发 notdef 回退路径。
        let glyph_info = self.font_tree.lookup_glyph(ch, 1, is_color)?;
        let ids = self.font_tree.all_ids();
        let face_id = ids.get(glyph_info.face_index)?;
        let data = self
            .font_tree
            .database()
            .with_face_data(*face_id, |data, _index| data.to_vec())?;

        // 解析字体
        let font = swash::FontRef::from_index(&data, 0)?;

        // 创建缩放器
        let mut scaler = self
            .font_tree
            .scale_context_mut()
            .builder(font)
            .size(ppem)
            .hint(true)
            .build();

        // 渲染字形
        use swash::scale::{Render, Source, StrikeWith};
        use swash::zeno::Format;

        let sources: &[Source] = if is_color {
            &[
                Source::ColorOutline(0),
                Source::ColorBitmap(StrikeWith::BestFit),
                Source::Outline,
            ]
        } else {
            &[Source::Outline]
        };

        let mut render = Render::new(sources);
        render.format(Format::Alpha);

        let image = render.render(&mut scaler, glyph_info.glyph_id)?;

        self.stats.rasterizations += 1;

        let placement = image.placement;
        Some(RasterizedGlyph {
            data: image.data,
            width: placement.width,
            height: placement.height,
            left_bearing: placement.left,
            top_bearing: placement.top,
            is_color: image.content == swash::scale::image::Content::Color,
        })
    }

    /// 合成字形到画布
    fn composite_glyph(&mut self, x: u32, y: u32, entry: AtlasEntry, color: Color, dim: bool) {
        let metrics = self.config.metrics;
        let baseline = metrics.baseline;

        // 计算目标位置
        let dest_x = x as i32 + entry.left_bearing;
        let dest_y = y as i32 + baseline as i32 - entry.top_bearing;

        // 裁剪到画布范围
        if dest_x < 0 || dest_y < 0 {
            return;
        }

        let dest_x = dest_x as u32;
        let dest_y = dest_y as u32;

        if dest_x >= self.canvas.width() || dest_y >= self.canvas.height() {
            return;
        }

        let max_w = (self.canvas.width() - dest_x).min(entry.width);
        let max_h = (self.canvas.height() - dest_y).min(entry.height);

        // 混合像素
        let color_factor = if dim { 0.5 } else { 1.0 };
        let r = (color.r as f32 * color_factor) as u8;
        let g = (color.g as f32 * color_factor) as u8;
        let b = (color.b as f32 * color_factor) as u8;

        for py in 0..max_h {
            for px in 0..max_w {
                let alpha = self.atlas.sample_alpha(dest_x + px, dest_y + py);
                if alpha > 0 {
                    self.canvas
                        .blend_pixel(dest_x + px, dest_y + py, r, g, b, alpha);
                }
            }
        }
    }

    /// 渲染装饰（下划线、删除线、波浪线等）
    fn render_decorations(&mut self, x: u32, y: u32, cell: &RustXtermCell) {
        let metrics = self.config.metrics;
        let cell_w = (cell.width as u32) * metrics.cell_width;
        let cell_h = metrics.cell_height;

        let fg = if cell.flags.contains(CellFlags::REVERSE) {
            cell.bg
        } else {
            cell.fg
        };

        // 下划线
        if cell.flags.contains(CellFlags::UNDERLINE) {
            let uy = y + cell_h - 2;
            self.canvas
                .fill_rect(x, uy, cell_w, 1, fg.r, fg.g, fg.b, fg.a);
        }

        // 双下划线
        if cell.flags.contains(CellFlags::DOUBLE_UNDERLINE) {
            let uy1 = y + cell_h - 3;
            let uy2 = y + cell_h - 1;
            self.canvas
                .fill_rect(x, uy1, cell_w, 1, fg.r, fg.g, fg.b, fg.a);
            self.canvas
                .fill_rect(x, uy2, cell_w, 1, fg.r, fg.g, fg.b, fg.a);
        }

        // 波浪下划线（Undercurl）：正弦波算法
        if cell.flags.contains(CellFlags::UNDERCURL) {
            self.render_undercurl(x, y, cell_w, cell_h, fg);
        }

        // 删除线
        if cell.flags.contains(CellFlags::STRIKETHROUGH) {
            let sy = y + cell_h / 2;
            self.canvas
                .fill_rect(x, sy, cell_w, 1, fg.r, fg.g, fg.b, fg.a);
        }
    }

    /// 渲染波浪下划线
    ///
    /// 正弦波算法实时绘制，参数 (Amplitude=2px, Period=auto)
    fn render_undercurl(&mut self, x: u32, y: u32, width: u32, height: u32, color: Color) {
        let amplitude = 2u32;
        let period = 8u32; // 自动周期
        let baseline_y = y + height - 2;

        for px in 0..width {
            // 正弦波计算
            let phase = (px % period) as f32 / period as f32 * std::f32::consts::TAU;
            let offset = (phase.sin() * amplitude as f32) as i32;
            let py = (baseline_y as i32 + offset) as u32;

            if py < self.canvas.height() {
                self.canvas
                    .put_pixel(x + px, py, color.r, color.g, color.b, color.a);
            }
        }
    }

    /// 渲染光标
    pub fn render_cursor(&mut self, cursor: &CursorMeta) {
        if !cursor.visible {
            return;
        }

        let metrics = self.config.metrics;
        let x = cursor.x as u32 * metrics.cell_width;
        let y = cursor.y as u32 * metrics.cell_height;

        match cursor.shape {
            CursorShape::Block | CursorShape::Default => {
                // 块状光标：填充整个单元格
                self.canvas.fill_rect(
                    x,
                    y,
                    metrics.cell_width,
                    metrics.cell_height,
                    255,
                    255,
                    255,
                    128, // 半透明
                );
            }
            CursorShape::Bar => {
                // 竖线光标
                self.canvas
                    .fill_rect(x, y, 2, metrics.cell_height, 255, 255, 255, 255);
            }
            CursorShape::Underline => {
                // 下划线光标
                self.canvas.fill_rect(
                    x,
                    y + metrics.cell_height - 2,
                    metrics.cell_width,
                    2,
                    255,
                    255,
                    255,
                    255,
                );
            }
        }
    }

    /// 清空画布
    pub fn clear(&mut self) {
        let bg = self.config.default_bg;
        self.canvas.clear(bg.r, bg.g, bg.b, bg.a);
    }

    /// 调整画布大小
    pub fn resize(&mut self, width: u32, height: u32) {
        self.canvas.resize(width, height);
        self.config.canvas_width = width;
        self.config.canvas_height = height;
    }

    /// 获取渲染度量
    pub fn metrics(&self) -> RenderMetrics {
        self.config.metrics
    }
}

/// 光栅化后的字形数据
struct RasterizedGlyph {
    data: Vec<u8>,
    width: u32,
    height: u32,
    left_bearing: i32,
    top_bearing: i32,
    is_color: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_renderer_creation() {
        let renderer = Renderer::new(RendererConfig::default());
        assert_eq!(renderer.canvas().width(), 640);
        assert_eq!(renderer.canvas().height(), 384);
    }

    #[test]
    fn test_render_blank_row() {
        let mut renderer = Renderer::new(RendererConfig::default());
        let cells = vec![RustXtermCell::blank(); 10];
        let rect = renderer.render_row(0, &cells);
        assert_eq!(rect.3, 16); // height = cell_height
    }

    #[test]
    fn test_render_text_row() {
        let mut renderer = Renderer::new(RendererConfig::default());
        let cells: Vec<RustXtermCell> = "Hello"
            .chars()
            .map(|ch| RustXtermCell {
                text: ch.to_string(),
                width: 1,
                fg: Color::WHITE,
                bg: Color::BLACK,
                flags: CellFlags(0),
                hyperlink: None,
            })
            .collect();
        let rect = renderer.render_row(0, &cells);
        assert!(rect.2 > 0); // width > 0
    }

    #[test]
    fn test_cursor_rendering() {
        let mut renderer = Renderer::new(RendererConfig::default());
        let cursor = CursorMeta {
            x: 5,
            y: 3,
            visible: true,
            shape: CursorShape::Block,
        };
        renderer.render_cursor(&cursor);
        // 不 panic 即通过
    }

    #[test]
    fn test_undercurl() {
        let mut renderer = Renderer::new(RendererConfig::default());
        let cell = RustXtermCell {
            text: "test".to_string(),
            width: 1,
            fg: Color::WHITE,
            bg: Color::BLACK,
            flags: CellFlags(CellFlags::UNDERCURL),
            hyperlink: None,
        };
        renderer.render_decorations(0, 0, &cell);
        // 不 panic 即通过
    }

    #[test]
    fn test_render_frame_dirty_rects() {
        // 仅渲染指定的几行，断言 dirty_rects 长度等于行数，
        // 且每行矩形 y 坐标 = row * cell_height
        let mut renderer = Renderer::new(RendererConfig::default());
        let cell_height = renderer.metrics().cell_height;

        // 准备 10 行空白 Cell
        let rows: Vec<Vec<RustXtermCell>> =
            (0..10).map(|_| vec![RustXtermCell::blank(); 5]).collect();
        let cells: Vec<&[RustXtermCell]> = rows.iter().map(|r| r.as_slice()).collect();

        // 仅渲染第 2、5、8 行
        let dirty_rows: [u32; 3] = [2, 5, 8];
        let result = renderer.render_frame(&dirty_rows, &cells);

        assert_eq!(
            result.dirty_rects.len(),
            dirty_rows.len(),
            "dirty_rects 数量应等于脏行数"
        );
        for (i, &row) in dirty_rows.iter().enumerate() {
            let rect = result.dirty_rects[i];
            assert_eq!(
                rect.1,
                row * cell_height,
                "第 {} 个矩形 y 坐标应为 row({}) * cell_height({}) = {}",
                i,
                row,
                cell_height,
                row * cell_height
            );
            assert_eq!(rect.3, cell_height, "矩形高度应等于 cell_height");
        }
    }

    #[test]
    fn test_missing_glyph_renders_box() {
        // 渲染一个系统字体几乎不可能覆盖的字符（Supplementary PUA-B 末尾），
        // 触发 .notdef 路径，断言 canvas 对应位置有非零像素（前景色方块）。
        let mut renderer = Renderer::new(RendererConfig::default());
        let metrics = renderer.metrics();

        // U+10FFFD：Supplementary Private Use Area-B 末尾，系统字体通常不覆盖
        let ch = '\u{10FFFD}';
        let cell = RustXtermCell {
            text: ch.to_string(),
            width: 1,
            fg: Color::WHITE,
            bg: Color::BLACK,
            flags: CellFlags(0),
            hyperlink: None,
        };
        let cells = vec![cell];
        renderer.render_row(0, &cells);

        // 检查 cell 区域有非零像素（notdef 方块用前景色白色绘制）
        let canvas = renderer.canvas();
        let mut found_non_zero = false;
        for py in 0..metrics.cell_height {
            for px in 0..metrics.cell_width {
                let (r, g, b, _) = canvas.get_pixel(px, py);
                if r > 0 || g > 0 || b > 0 {
                    found_non_zero = true;
                    break;
                }
            }
            if found_non_zero {
                break;
            }
        }
        assert!(
            found_non_zero,
            "缺字位置应画出 notdef 方块，但 canvas 全黑（背景色）"
        );
    }
}
