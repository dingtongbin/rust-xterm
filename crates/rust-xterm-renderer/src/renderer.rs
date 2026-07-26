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
use crate::font_tree::{FontTree, ShapeGlyph};
use crate::global_atlas::global_atlas;
use rust_xterm_core::{
    CellFlags, Color, CursorMeta, CursorShape, DirtySpan, ImagePlacement, RustXtermCell,
};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

/// 渲染层使用的字符宽度估算（用于 IME 预编辑文本绘制）
///
/// 与 WezTerm 的权威宽度表无关；仅用于预估字符占据的 cell 数。
/// CJK 区段 / 全角符号 / Emoji 返回 2，其余返回 1。
fn char_width_for_render(ch: char) -> usize {
    let c = ch as u32;
    // CJK Unified Ideographs / Hiragana / Katakana / Hangul
    if (0x3040..=0x30FF).contains(&c)
        || (0x3400..=0x4DBF).contains(&c)
        || (0x4E00..=0x9FFF).contains(&c)
        || (0xAC00..=0xD7AF).contains(&c)
        || (0xF900..=0xFAFF).contains(&c)
        || (0xFF00..=0xFFEF).contains(&c) // 全角符号
        || (0x1F300..=0x1FAFF).contains(&c) // Emoji
        || (0x2600..=0x27BF).contains(&c)
    {
        2
    } else {
        1
    }
}

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
    /// 是否启用连字（ligature）渲染
    ///
    /// - `true`（默认）：`render_row` 按 run 整形，调用 `shape_run` 启用
    ///   `liga` / `calt` 等 OpenType 连字 feature，对 `!=` / `=>` / `==` 等
    ///   序列输出合并字形。
    /// - `false`：`render_row` 走原有单字符路径（`render_cell_text`），
    ///   每个 cell 独立查表 + 光栅化 + 合成，与未启用本特性时行为一致。
    ///
    /// cell.width 布局权威：连字字形不改变 cell 宽度，仅在 cell 宽度内
    /// 调整字形绘制位置。
    pub enable_ligatures: bool,
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
            enable_ligatures: true,
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
    /// 全局共享纹理图集（跨实例 LRU）
    ///
    /// 由 [`Renderer::with_global_atlas`] 挂载。`Some` 时
    /// `render_cell_text` / `render_run_text` 优先从全局 atlas 查询，
    /// miss 时回退光栅化并回填全局 atlas。`None` 时仅使用 per-instance
    /// `atlas`。
    global_atlas: Option<Arc<Mutex<TextureAtlas>>>,
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
        // 图集使用 RGBA（4 bytes/pixel）以同时容纳：
        // - 普通字形的 alpha 掩码（写入 (a, a, a, a)，sample_alpha 取 A 通道）
        // - 彩色 emoji 的 RGBA 原色（sample_rgba 直接取色，跳过前景色混合）
        let atlas = TextureAtlas::new(config.atlas_width, config.atlas_height, 4, 1024);
        let canvas = Canvas::new(config.canvas_width, config.canvas_height, PixelFormat::Rgba);
        let mut font_tree = FontTree::new();
        // 整形字号与渲染度量一致，使 shape_run 产出的 advance 与
        // 光栅化后的字形宽度匹配（连字字形居中于 cell 宽度内时用到）。
        font_tree.set_shape_size(config.metrics.font_size);

        Self {
            config,
            atlas,
            global_atlas: None,
            font_tree,
            canvas,
            stats: RenderStats::default(),
        }
    }

    /// 创建挂载全局共享图集的渲染器
    ///
    /// 与 [`Renderer::new`] 的区别：纹理图集由进程级
    /// [`GlobalAtlas`](crate::global_atlas::GlobalAtlas) 单例提供，
    /// 多个 Renderer 共享同一份 `Arc<Mutex<TextureAtlas>>`，避免多终端
    /// 实例各自维护独立图集导致的重复光栅化与内存占用。
    ///
    /// - global atlas 尺寸由 `config` 决定（首次调用初始化，后续忽略尺寸参数）
    /// - per-instance canvas 仍按 `config` 创建
    /// - per-instance `atlas` 仅作占位（1x1），实际查询/插入走 global
    ///
    /// 渲染时 [`render_cell_text`](Self::render_cell_text) /
    /// [`render_run_text`](Self::render_run_text) 优先从 global atlas 查询，
    /// miss 时回退光栅化并回填 global atlas。
    pub fn with_global_atlas(config: RendererConfig) -> Self {
        let global = global_atlas().get_or_init(config.clone());
        let canvas = Canvas::new(config.canvas_width, config.canvas_height, PixelFormat::Rgba);
        let mut font_tree = FontTree::new();
        font_tree.set_shape_size(config.metrics.font_size);
        // global atlas 共享，per-instance 仅作占位（实际查询走 global）
        Self {
            config,
            atlas: TextureAtlas::new(1, 1, 4, 1),
            global_atlas: Some(global),
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
    ///
    /// 当 `RendererConfig::enable_ligatures` 为 `true` 时，按 run 整形：
    /// 收集连续同 (fg, bg, flags) 且主字体覆盖的非空白 cell，拼接为 run_str
    /// 调用 `FontTree::shape_run`，再按 cluster 映射回 cell 渲染。
    /// 否则回退到 `render_cell_text` 单字符路径。
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

        // 再渲染前景文本与装饰
        x = 0;
        let mut i = 0;
        while i < cells.len() {
            let cell = &cells[i];
            let cell_pixel_width = (cell.width as u32) * metrics.cell_width;
            if x >= max_width {
                break;
            }

            if cell.is_blank() {
                // 空白 cell：仅渲染装饰
                self.render_decorations(x, y, cell);
                x += cell_pixel_width;
                i += 1;
                continue;
            }

            // 尝试按 run 渲染连字（仅当启用连字、cell 文本单字符、主字体覆盖时）
            if self.config.enable_ligatures {
                let run = self.collect_run(cells, i);
                if run.end > i + 1 || (run.end == i + 1 && run.face_id.is_some()) {
                    // run 至少包含当前 cell：渲染 run 文本
                    let run_end = run.end;
                    self.render_run_text(&cells[i..run_end], x, y, &run);
                    // 渲染 run 内每个 cell 的装饰
                    let mut dx = x;
                    for c in &cells[i..run_end] {
                        self.render_decorations(dx, y, c);
                        dx += (c.width as u32) * metrics.cell_width;
                    }
                    // 推进 x 与 i
                    for c in &cells[i..run_end] {
                        x += (c.width as u32) * metrics.cell_width;
                    }
                    i = run_end;
                    continue;
                }
            }

            // 回退：单字符路径
            self.render_cell_text(x, y, cell);
            // 渲染装饰（下划线、删除线等）
            self.render_decorations(x, y, cell);
            x += cell_pixel_width;
            i += 1;
        }

        self.stats.composites += 1;
        (0, y, max_width, metrics.cell_height)
    }

    /// 渲染一行的部分区间（仅 cells[col_start..col_end]）
    ///
    /// 与 [`render_row`] 的区别：仅渲染 `[col_start, col_end)` 范围内的 cell，
    /// 用于子行/列级脏区，避免重绘整行。
    ///
    /// 返回脏矩形 `(x, y, width, height)`，其中 `x = col_start * cell_width`，
    /// `width = (col_end - col_start) * cell_width`。
    pub fn render_row_segment(
        &mut self,
        row: u32,
        col_start: usize,
        col_end: usize,
        cells: &[RustXtermCell],
    ) -> (u32, u32, u32, u32) {
        let metrics = self.config.metrics;
        let y = row * metrics.cell_height;
        let max_width = self.canvas.width();
        let row_len = cells.len();
        let cs = col_start.min(row_len);
        let ce = col_end.min(row_len).max(cs);

        // 填充背景（仅 dirty 区间）
        let mut x = (cs as u32) * metrics.cell_width;
        for cell in &cells[cs..ce] {
            let cell_pixel_width = (cell.width as u32) * metrics.cell_width;
            if x >= max_width {
                break;
            }
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

        // 渲染前景文本与装饰（仅 dirty 区间）
        // 注意：连字 run 可能跨越 col_start/col_end 边界，这里简化为按 cell 渲染
        // （连字渲染要求连续 run，跨边界处理复杂；当前实现接受 dirty 边界处连字回退到单字符路径）
        x = (cs as u32) * metrics.cell_width;
        let mut i = cs;
        while i < ce {
            let cell = &cells[i];
            let cell_pixel_width = (cell.width as u32) * metrics.cell_width;
            if x >= max_width {
                break;
            }
            if cell.is_blank() {
                self.render_decorations(x, y, cell);
                x += cell_pixel_width;
                i += 1;
                continue;
            }
            // 子行渲染禁用连字（避免 run 越过 dirty 边界）
            // 直接走单字符路径
            self.render_cell_text(x, y, cell);
            self.render_decorations(x, y, cell);
            x += cell_pixel_width;
            i += 1;
        }

        self.stats.composites += 1;
        let x0 = (cs as u32) * metrics.cell_width;
        let width = ((ce - cs) as u32) * metrics.cell_width;
        (x0, y, width.min(max_width - x0), metrics.cell_height)
    }

    /// 收集从 `start` 开始的一个 run
    ///
    /// 一个 run 由连续满足以下条件的 cell 组成：
    /// - 非空白
    /// - 文本为单字符（避免宽字符 / 多字符 cell 进入连字路径）
    /// - 与起始 cell 具有相同的 (fg, bg, flags)
    /// - 主字体覆盖该字符（`lookup_glyph` 返回 `face_index == 0` 且
    ///   `glyph_id != 0`）
    ///
    /// 返回的 `RunInfo` 包含 run 的 `[start, end)` 区间、拼接的 `run_str`、
    /// 每个 cell 在 `run_str` 中的字节偏移、以及主字体 ID（`None` 表示
    /// 当前 cell 不满足连字前置条件，调用方应回退到单字符路径）。
    fn collect_run(&mut self, cells: &[RustXtermCell], start: usize) -> RunInfo {
        let primary_id = self.font_tree.primary_id();
        let start_cell = &cells[start];

        // 主字体不可用：直接返回空 run，调用方回退
        let face_id = match primary_id {
            Some(id) => id,
            None => {
                return RunInfo {
                    end: start,
                    run_str: String::new(),
                    byte_offsets: Vec::new(),
                    face_id: None,
                };
            }
        };

        // 第一个 cell 必须满足前置条件
        let first_ch = match start_cell.text.chars().next() {
            Some(ch) if start_cell.text.chars().count() == 1 => ch,
            _ => {
                return RunInfo {
                    end: start,
                    run_str: String::new(),
                    byte_offsets: Vec::new(),
                    face_id: None,
                };
            }
        };
        let first_info = match self
            .font_tree
            .lookup_glyph(first_ch, start_cell.width, false)
        {
            Some(info) if info.face_index == 0 && info.glyph_id != 0 => info,
            _ => {
                return RunInfo {
                    end: start,
                    run_str: String::new(),
                    byte_offsets: Vec::new(),
                    face_id: None,
                };
            }
        };
        let _ = first_info; // 仅用于主字体覆盖检查

        // 收集 run_str 与 byte_offsets
        let mut run_str = String::new();
        let mut byte_offsets: Vec<u32> = Vec::new();
        let mut end = start;

        for cell in &cells[start..] {
            // 相同 (fg, bg, flags) 才能加入同一 run
            if cell.fg != start_cell.fg
                || cell.bg != start_cell.bg
                || cell.flags != start_cell.flags
            {
                break;
            }
            if cell.is_blank() {
                break;
            }
            // 仅单字符 cell 进入连字路径
            let ch = match cell.text.chars().next() {
                Some(ch) if cell.text.chars().count() == 1 => ch,
                _ => break,
            };
            // 仅主字体覆盖的字符进入连字路径（否则回退到单字符路径以走 fallback 链）
            let info = match self.font_tree.lookup_glyph(ch, cell.width, false) {
                Some(info) if info.face_index == 0 && info.glyph_id != 0 => info,
                _ => break,
            };
            let _ = info;
            byte_offsets.push(run_str.len() as u32);
            run_str.push(ch);
            end += 1;
        }

        RunInfo {
            end,
            run_str,
            byte_offsets,
            face_id: Some(face_id),
        }
    }

    /// 渲染一个 run 的前景文本（连字路径）
    ///
    /// 流程：
    /// 1. `shape_run(run_str, face_id)` 得到 `Vec<ShapeGlyph>`
    /// 2. 用 glyph_id 序列计算 `run_hash`
    /// 3. `lookup_run(run_hash, bold, italic)`
    ///    - 命中：直接复用 `Vec<AtlasEntry>`
    ///    - 未命中：对每个 ShapeGlyph 调用 `rasterize_shape_glyph` +
    ///      `allocate_dynamic` 收集 entries，再 `insert_run` 缓存
    /// 4. 对每个 (ShapeGlyph, AtlasEntry)，按 `cluster` 字段在
    ///    `byte_offsets` 中找到对应 cell，调 `composite_glyph` 在 cell 的
    ///    x 坐标处合成。cell 宽度由 `cell.width * cell_width` 决定，
    ///    连字字形自然延伸到相邻 cell（不改变 cell 宽度布局）。
    fn render_run_text(
        &mut self,
        run_cells: &[RustXtermCell],
        x_start: u32,
        y: u32,
        run: &RunInfo,
    ) {
        let face_id = match run.face_id {
            Some(id) => id,
            None => return,
        };
        if run.run_str.is_empty() {
            return;
        }

        // 1. 整形
        let shape_glyphs = self.font_tree.shape_run(&run.run_str, face_id);
        if shape_glyphs.is_empty() {
            return;
        }

        // 2. 计算 run_hash（用 glyph_id 序列）
        let run_hash = hash_glyph_ids(&shape_glyphs);

        // 3. 查 run 缓存
        let bold = run_cells[0].flags.contains(CellFlags::BOLD);
        let italic = run_cells[0].flags.contains(CellFlags::ITALIC);
        // 克隆 Arc 释放对 self 的借用，以便 miss 时调用 self.rasterize_shape_glyph。
        let global = self.global_atlas.as_ref().cloned();
        let entries = if let Some(global) = global {
            // 全局 atlas 路径：加锁查询；miss 时释放锁、光栅化、再加锁分配+缓存
            let mut guard = global.lock().unwrap();
            match guard.lookup_run(run_hash, bold, italic) {
                Some(e) => {
                    self.stats.atlas_hits += 1;
                    e
                }
                None => {
                    self.stats.atlas_misses += 1;
                    // 释放锁后光栅化所有 glyph（无需持锁）
                    drop(guard);
                    let mut rasters = Vec::with_capacity(shape_glyphs.len());
                    for sg in &shape_glyphs {
                        match self.rasterize_shape_glyph(sg.glyph_id, face_id, false) {
                            Some(r) => rasters.push(r),
                            None => continue,
                        }
                    }
                    // 重新加锁：分配槽位并缓存 run
                    let mut guard = global.lock().unwrap();
                    let mut collected = Vec::with_capacity(rasters.len());
                    for raster in &rasters {
                        if let Some(entry) = guard.allocate_dynamic(
                            &raster.data,
                            raster.width,
                            raster.height,
                            raster.left_bearing,
                            raster.top_bearing,
                            raster.is_color,
                        ) {
                            collected.push(entry);
                        }
                    }
                    // 仅当全部 glyph 都成功分配时缓存 run，避免下次命中残缺 entries
                    if collected.len() == shape_glyphs.len() && !collected.is_empty() {
                        guard.insert_run(run_hash, bold, italic, collected.clone());
                    }
                    collected
                }
            }
        } else {
            // per-instance atlas 路径
            match self.atlas.lookup_run(run_hash, bold, italic) {
                Some(e) => {
                    self.stats.atlas_hits += 1;
                    e
                }
                None => {
                    self.stats.atlas_misses += 1;
                    // Miss: 对每个 glyph 光栅化 + 分配槽位
                    let mut collected = Vec::with_capacity(shape_glyphs.len());
                    for sg in &shape_glyphs {
                        let raster = match self.rasterize_shape_glyph(sg.glyph_id, face_id, false) {
                            Some(r) => r,
                            None => continue,
                        };
                        if let Some(entry) = self.atlas.allocate_dynamic(
                            &raster.data,
                            raster.width,
                            raster.height,
                            raster.left_bearing,
                            raster.top_bearing,
                            raster.is_color,
                        ) {
                            collected.push(entry);
                        }
                    }
                    // 仅当全部 glyph 都成功分配时缓存 run，避免下次命中残缺 entries
                    if collected.len() == shape_glyphs.len() && !collected.is_empty() {
                        self.atlas
                            .insert_run(run_hash, bold, italic, collected.clone());
                    }
                    collected
                }
            }
        };

        // 4. 按 cluster 映射回 cell 并合成
        let metrics = self.config.metrics;
        let fg = if run_cells[0].flags.contains(CellFlags::REVERSE) {
            run_cells[0].bg
        } else {
            run_cells[0].fg
        };
        let dim = run_cells[0].flags.contains(CellFlags::DIM);

        // 合成阶段需从对应 atlas 采样：global 路径持锁从 global 采样，
        // per-instance 路径从 self.atlas 采样。
        // global_arc 必须独立绑定以延长 Arc 生命周期，使 MutexGuard 有效。
        let global_arc = self.global_atlas.as_ref().cloned();
        let global_guard = global_arc.as_ref().map(|arc| arc.lock().unwrap());
        let atlas: &TextureAtlas = match &global_guard {
            Some(g) => g,
            None => &self.atlas,
        };

        for (sg, entry) in shape_glyphs.iter().zip(entries.iter()) {
            // cluster 是 run_str 的字节偏移，找到所属 cell
            let cell_idx = match cluster_to_cell_index(sg.cluster, &run.byte_offsets) {
                Some(idx) => idx,
                None => continue,
            };
            if cell_idx >= run_cells.len() as u32 {
                continue;
            }
            // 计算 cell 的 x 坐标：累加前面所有 cell 的 width
            let mut cell_x = x_start;
            for c in &run_cells[..cell_idx as usize] {
                cell_x += (c.width as u32) * metrics.cell_width;
            }
            composite_glyph_into(&mut self.canvas, atlas, metrics, cell_x, y, *entry, fg, dim);
        }
    }

    /// 按 span 列表渲染帧
    ///
    /// 每个 [`DirtySpan`] 携带行号、列范围与该行完整 cells。
    /// 整行脏（`col_end == cells.len()` 或 `col_end == cols`）调用 [`render_row`]；
    /// 部分行脏调用 [`render_row_segment`] 仅重绘脏列区间。
    pub fn render_frame(&mut self, spans: &[DirtySpan]) -> RenderResult {
        let mut result = RenderResult {
            dirty_rects: Vec::with_capacity(spans.len()),
            cursor: None,
        };
        for span in spans {
            let row = span.row as u32;
            let cells = &span.cells;
            // 判定整行 vs 部分：col_start == 0 且 col_end >= cells.len() 视为整行
            if span.col_start == 0 && span.col_end >= cells.len() {
                let rect = self.render_row(row, cells);
                result.dirty_rects.push(rect);
            } else {
                let rect = self.render_row_segment(row, span.col_start, span.col_end, cells);
                result.dirty_rects.push(rect);
            }
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
        // is_color 由 font_tree 根据 Emoji Unicode 区段内部判定并覆盖，
        // 调用方传入的 is_color_hint=false 仅为占位（未来可由 cell flags 提示），
        // 实际光栅化用的是返回的 glyph_info.is_color。
        let glyph_info = match self.font_tree.lookup_glyph(ch, cell.width, false) {
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

        let bold = cell.flags.contains(CellFlags::BOLD);
        let italic = cell.flags.contains(CellFlags::ITALIC);

        // 优先从全局 atlas 查询（若已挂载）。
        // 克隆 Arc 释放对 self 的借用，以便后续调用 self.rasterize_glyph 等方法。
        if let Some(global) = self.global_atlas.as_ref().cloned() {
            // 加锁查询；miss 时释放锁、光栅化、再加锁插入（避免持锁光栅化）。
            let entry = {
                let mut guard = global.lock().unwrap();
                match guard.lookup_dynamic(ch, bold, italic) {
                    Some(e) => {
                        self.stats.atlas_hits += 1;
                        Some(*e)
                    }
                    None => {
                        self.stats.atlas_misses += 1;
                        // 释放锁后光栅化，再重新加锁插入到 global atlas
                        drop(guard);
                        match self.rasterize_glyph(ch, glyph_info.is_color) {
                            Some(pixels) => {
                                let mut guard = global.lock().unwrap();
                                guard.insert_dynamic(
                                    ch,
                                    bold,
                                    italic,
                                    &pixels.data,
                                    pixels.width,
                                    pixels.height,
                                    pixels.left_bearing,
                                    pixels.top_bearing,
                                    pixels.is_color,
                                )
                            }
                            None => None,
                        }
                    }
                }
            };
            let entry = match entry {
                Some(e) => e,
                None => return,
            };
            // 合成：从 global atlas 采样（持锁期间完成合成）
            let fg = if cell.flags.contains(CellFlags::REVERSE) {
                cell.bg
            } else {
                cell.fg
            };
            let dim = cell.flags.contains(CellFlags::DIM);
            let guard = global.lock().unwrap();
            composite_glyph_into(
                &mut self.canvas,
                &guard,
                self.config.metrics,
                x,
                y,
                entry,
                fg,
                dim,
            );
            return;
        }

        // 回退：per-instance atlas
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
    ///
    /// 根据 `is_color` 选择不同的 swash 源链：
    /// - 彩色路径：`ColorOutline` -> `ColorBitmap` -> `Outline`（fallback）
    ///   彩色源命中时输出 RGBA（4 bytes/pixel），未命中回退到 Outline 输出 alpha
    /// - 普通路径：`Outline` 输出 alpha（1 byte/pixel）
    ///
    /// 由于图集统一使用 RGBA（4 bytes/pixel）存储，alpha 掩码会被展开为
    /// `(a, a, a, a)`，使得 `sample_alpha` 仍能正确取到 A 通道。
    fn rasterize_glyph(&mut self, ch: char, is_color: bool) -> Option<RasterizedGlyph> {
        // 通过单字符查表获得 glyph_id 与 face_id，再委托给 rasterize_shape_glyph。
        // 此处 width=1 仅用于查表，实际渲染宽度由调用方（cell.width）决定。
        let glyph_info = self.font_tree.lookup_glyph(ch, 1, is_color)?;
        let ids = self.font_tree.all_ids();
        let face_id = *ids.get(glyph_info.face_index)?;
        self.rasterize_shape_glyph(glyph_info.glyph_id, face_id, is_color)
    }

    /// 按 glyph_id 与 face_id 光栅化（连字路径用）
    ///
    /// 与 `rasterize_glyph` 共享同一光栅化管线，但跳过单字符查表，
    /// 直接使用 `shape_run` 产出的 (glyph_id, face_id)。这样连字字形无需
    /// 关联到某个具体 char 即可光栅化。
    fn rasterize_shape_glyph(
        &mut self,
        glyph_id: swash::GlyphId,
        face_id: fontdb::ID,
        is_color: bool,
    ) -> Option<RasterizedGlyph> {
        let metrics = self.config.metrics;
        let ppem = metrics.font_size;

        // 获取字体数据
        let data = self
            .font_tree
            .database()
            .with_face_data(face_id, |data, _index| data.to_vec())?;

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
        use swash::scale::image::Content;
        use swash::scale::{Render, Source, StrikeWith};
        use swash::zeno::Format;

        // 彩色字形优先尝试 COLR/CPAL 彩色轮廓与 sbix/CBDT 彩色位图，
        // 都未命中时回退到普通轮廓（输出 alpha 掩码）。
        // swash 0.1.15 + zeno 0.2.3 的 Format 仅支持 Alpha / Subpixel / CustomSubpixel，
        // 不存在 Format::Bgra / SubpixelRgb；彩色源 (ColorOutline/ColorBitmap) 始终
        // 输出 RGBA（4 bytes/pixel），与 self.format 无关，故这里继续用 Alpha
        // 即可——只有 fallback 到 Outline 时格式才生效，此时输出 1 byte alpha。
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

        let image = render.render(&mut scaler, glyph_id)?;

        self.stats.rasterizations += 1;

        let placement = image.placement;
        // 彩色源命中时 content == Color，data 已是 RGBA；否则 content == Mask，
        // data 为 1 byte/pixel alpha，需展开为 4 bytes/pixel 以适配 RGBA 图集。
        let is_color_result = image.content == Content::Color;
        let data = if is_color_result {
            image.data
        } else {
            // 将 alpha 掩码 (1 byte/pixel) 展开为 RGBA (4 bytes/pixel)：
            // 写入 (a, a, a, a)，使 sample_alpha 取 A 通道、sample_rgba 取灰度。
            let mut rgba = Vec::with_capacity(image.data.len() * 4);
            for &a in &image.data {
                rgba.push(a);
                rgba.push(a);
                rgba.push(a);
                rgba.push(a);
            }
            rgba
        };

        Some(RasterizedGlyph {
            data,
            width: placement.width,
            height: placement.height,
            left_bearing: placement.left,
            top_bearing: placement.top,
            is_color: is_color_result,
        })
    }

    /// 合成字形到画布
    ///
    /// 根据 `entry.is_color` 走两条路径：
    /// - **彩色路径**：直接从图集 `sample_rgba` 取 RGBA 原色，写入画布。
    ///   不与前景色混合（emoji 自带颜色），但 `dim` 时整体亮度减半。
    /// - **普通路径**：从图集 `sample_alpha` 取 alpha 掩码，与前景色混合后写入。
    ///
    /// 当字形部分超出画布左/上边界时（如 `left_bearing = -1` 的 'A'），
    /// 会裁剪到可见区域而非整体跳过——否则 ASCII 与 emoji 都无法在首列/首行渲染。
    fn composite_glyph(&mut self, x: u32, y: u32, entry: AtlasEntry, color: Color, dim: bool) {
        composite_glyph_into(
            &mut self.canvas,
            &self.atlas,
            self.config.metrics,
            x,
            y,
            entry,
            color,
            dim,
        )
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

    /// 渲染 IME 预编辑文本（composition）
    ///
    /// 在 cursor 行的 cursor 列之后绘制带下划线的预编辑文本。
    /// 不改变 cell 布局，仅叠加像素层（背景已由 render_row 填充）。
    /// 文本按字符逐个绘制：每个字符占 `cell_width` 像素宽，下划线在底部。
    pub fn render_preedit(&mut self, cursor: &CursorMeta, text: &str) {
        if text.is_empty() {
            return;
        }
        let metrics = self.config.metrics;
        let base_x = cursor.x as u32 * metrics.cell_width;
        let y = cursor.y as u32 * metrics.cell_height;
        let max_width = self.canvas.width();
        let fg = self.config.default_fg;
        let mut x = base_x;
        for ch in text.chars() {
            // 估算字符显示宽度（CJK / 全角 emoji 算 2，其余 1）
            let cw = char_width_for_render(ch);
            let cell_w = cw as u32 * metrics.cell_width;
            if x + cell_w > max_width {
                break;
            }
            // 用 lookup_glyph + composite_glyph 路径绘制单字符（不走连字）
            // 直接复用 render_cell_text 风格，但需要一个独立的 helper 避免与 cell 耦合
            self.render_preedit_char(x, y, ch, fg);
            // 下划线（在 cell 底部）
            let uy = y + metrics.cell_height - 1;
            self.canvas
                .fill_rect(x, uy, cell_w, 1, fg.r, fg.g, fg.b, fg.a);
            x += cell_w;
        }
    }

    /// 渲染单个预编辑字符（不依赖 cell，直接光栅化 + 合成）
    fn render_preedit_char(&mut self, x: u32, y: u32, ch: char, color: Color) {
        // 查字形（用宽度提示，is_color 由 font_tree 内部判定）
        let cw = char_width_for_render(ch);
        let glyph_info = match self.font_tree.lookup_glyph(ch, cw, false) {
            Some(info) => info,
            None => return,
        };
        if glyph_info.glyph_id == 0 {
            // 缺字：画方块占位
            let metrics = self.config.metrics;
            self.canvas.fill_rect(
                x,
                y,
                metrics.cell_width,
                metrics.cell_height,
                color.r,
                color.g,
                color.b,
                color.a / 2,
            );
            return;
        }
        // 查全局 atlas / per-instance atlas
        let bold = false;
        let italic = false;
        if let Some(global) = self.global_atlas.as_ref().cloned() {
            let entry = {
                let mut guard = global.lock().unwrap();
                guard.lookup_dynamic(ch, bold, italic).copied()
            };
            if let Some(entry) = entry {
                self.stats.atlas_hits += 1;
                self.composite_glyph(x, y, entry, color, false);
                return;
            }
        }
        let entry = self.atlas.lookup_dynamic(ch, bold, italic).copied();
        if let Some(entry) = entry {
            self.stats.atlas_hits += 1;
            self.composite_glyph(x, y, entry, color, false);
            return;
        }
        // miss: 光栅化并插入
        self.stats.atlas_misses += 1;
        if let Some(pixels) = self.rasterize_glyph(ch, glyph_info.is_color) {
            // 优先插入 global atlas（若挂载），否则 per-instance
            let entry = if let Some(global) = self.global_atlas.as_ref().cloned() {
                let mut guard = global.lock().unwrap();
                guard.insert_dynamic(
                    ch,
                    bold,
                    italic,
                    &pixels.data,
                    pixels.width,
                    pixels.height,
                    pixels.left_bearing,
                    pixels.top_bearing,
                    pixels.is_color,
                )
            } else {
                self.atlas.insert_dynamic(
                    ch,
                    bold,
                    italic,
                    &pixels.data,
                    pixels.width,
                    pixels.height,
                    pixels.left_bearing,
                    pixels.top_bearing,
                    pixels.is_color,
                )
            };
            if let Some(e) = entry {
                self.composite_glyph(x, y, e, color, false);
            }
        }
    }

    /// 清空画布
    pub fn clear(&mut self) {
        let bg = self.config.default_bg;
        self.canvas.clear(bg.r, bg.g, bg.b, bg.a);
    }

    /// 渲染图像放置（SubTask 9.5）
    ///
    /// 将 [`ImagePlacement`] 的 RGBA 像素数据直接 blit 到画布对应像素区域。
    ///
    /// 起始像素坐标 = `(col * cell_width, row * cell_height)`。
    /// 超出画布边界的像素被裁剪。透明像素（alpha = 0）跳过；
    /// 不透明像素（alpha = 255）直接 `put_pixel`；半透明像素走 `blend_pixel`。
    ///
    /// 宿主层应在每帧遍历 `manager.images().placements()` 调用本方法。
    pub fn render_image(&mut self, placement: &ImagePlacement) {
        let metrics = self.config.metrics;
        let base_x = placement.col as u32 * metrics.cell_width;
        let base_y = placement.row as u32 * metrics.cell_height;
        let canvas_w = self.canvas.width();
        let canvas_h = self.canvas.height();
        let stride = placement.width as usize * 4;

        for py in 0..placement.height {
            let dy = base_y + py;
            if dy >= canvas_h {
                break;
            }
            let row_offset = py as usize * stride;
            if row_offset + stride > placement.rgba.len() {
                break;
            }
            for px in 0..placement.width {
                let dx = base_x + px;
                if dx >= canvas_w {
                    break;
                }
                let idx = row_offset + px as usize * 4;
                if idx + 4 > placement.rgba.len() {
                    break;
                }
                let r = placement.rgba[idx];
                let g = placement.rgba[idx + 1];
                let b = placement.rgba[idx + 2];
                let a = placement.rgba[idx + 3];
                match a {
                    0 => continue,
                    255 => self.canvas.put_pixel(dx, dy, r, g, b, a),
                    _ => self.canvas.blend_pixel(dx, dy, r, g, b, a),
                }
            }
        }

        self.stats.composites += 1;
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

/// Run 收集结果
///
/// `collect_run` 的输出，描述一个可整形的连续 cell 段。
struct RunInfo {
    /// Run 在 `cells` 中的结束索引（exclusive，起始索引由调用方持有）
    end: usize,
    /// 拼接后的 run_str（每 cell 取首字符）
    run_str: String,
    /// 每个 cell 的首字符在 `run_str` 中的字节偏移
    byte_offsets: Vec<u32>,
    /// 主字体 ID（`None` 表示当前 cell 不满足连字前置条件）
    face_id: Option<fontdb::ID>,
}

/// 用 glyph_id 序列计算 run_hash
///
/// 仅哈希 glyph_id 序列，不包含 cluster/offset 等位置信息——
/// 这些位置由 cell 布局决定，不影响光栅化结果。
fn hash_glyph_ids(glyphs: &[ShapeGlyph]) -> u64 {
    let mut hasher = DefaultHasher::new();
    for g in glyphs {
        g.glyph_id.hash(&mut hasher);
    }
    hasher.finish()
}

/// 将 cluster 字节偏移映射到 run 内的 cell 索引
///
/// `byte_offsets[i]` 是第 i 个 cell 的首字符在 `run_str` 中的字节偏移。
/// 找到最大的 `i` 使得 `byte_offsets[i] <= cluster`，即为该 cluster 所属 cell。
fn cluster_to_cell_index(cluster: u32, byte_offsets: &[u32]) -> Option<u32> {
    if byte_offsets.is_empty() {
        return None;
    }
    // 二分查找最后一个 byte_offsets[i] <= cluster
    let mut lo = 0usize;
    let mut hi = byte_offsets.len();
    while lo + 1 < hi {
        let mid = (lo + hi) / 2;
        if byte_offsets[mid] <= cluster {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    // lo 是候选；但若 byte_offsets[0] > cluster，说明 cluster 在 run 之前（不应发生）
    if byte_offsets[lo] <= cluster {
        Some(lo as u32)
    } else {
        None
    }
}

/// 合成字形到画布（自由函数版本）
///
/// 与 [`Renderer::composite_glyph`] 共享同一份合成逻辑，但允许调用方
/// 显式指定采样源 `atlas`——用于全局图集路径：`render_cell_text` /
/// `render_run_text` 在 global atlas 命中后，持锁从 global atlas 采样
/// 合成到 `canvas`。
///
/// - **彩色路径**：直接从图集 `sample_rgba` 取 RGBA 原色，写入画布。
///   不与前景色混合（emoji 自带颜色），但 `dim` 时整体亮度减半。
/// - **普通路径**：从图集 `sample_alpha` 取 alpha 掩码，与前景色混合后写入。
///
/// 当字形部分超出画布左/上边界时（如 `left_bearing = -1` 的 'A'），
/// 会裁剪到可见区域而非整体跳过——否则 ASCII 与 emoji 都无法在首列/首行渲染。
#[allow(clippy::too_many_arguments)]
fn composite_glyph_into(
    canvas: &mut Canvas,
    atlas: &TextureAtlas,
    metrics: RenderMetrics,
    x: u32,
    y: u32,
    entry: AtlasEntry,
    color: Color,
    dim: bool,
) {
    let baseline = metrics.baseline;

    // 计算目标位置（画布坐标）
    let dest_x_signed = x as i32 + entry.left_bearing;
    let dest_y_signed = y as i32 + baseline as i32 - entry.top_bearing;

    // 部分裁剪：若 dest < 0，跳过左侧/上侧的像素，仅绘制可见部分
    let src_x_off = dest_x_signed.max(0) - dest_x_signed; // >=0 部分
    let src_y_off = dest_y_signed.max(0) - dest_y_signed;
    // 转为无符号后的画布目标位置
    let dest_x = dest_x_signed.max(0) as u32;
    let dest_y = dest_y_signed.max(0) as u32;

    if dest_x >= canvas.width() || dest_y >= canvas.height() {
        return;
    }
    // 整个字形都在左/上方之外
    if src_x_off as u32 >= entry.width || src_y_off as u32 >= entry.height {
        return;
    }

    // 可见区域的宽高（同时受画布右/下边界与字形尺寸约束）
    let max_w = (canvas.width() - dest_x).min(entry.width - src_x_off as u32);
    let max_h = (canvas.height() - dest_y).min(entry.height - src_y_off as u32);

    // 采样时从图集 entry 的存储位置 (entry.x, entry.y) 加偏移读取
    let atlas_x = entry.x + src_x_off as u32;
    let atlas_y = entry.y + src_y_off as u32;

    if entry.is_color {
        // 彩色字形：直接取 RGBA 原色，不与前景色混合。
        // dim 时整体亮度减半（与普通路径一致）。
        let color_factor = if dim { 0.5 } else { 1.0 };
        for py in 0..max_h {
            for px in 0..max_w {
                let (r, g, b, a) = atlas.sample_rgba(atlas_x + px, atlas_y + py);
                if a == 0 {
                    continue;
                }
                let r = (r as f32 * color_factor) as u8;
                let g = (g as f32 * color_factor) as u8;
                let b = (b as f32 * color_factor) as u8;
                canvas.blend_pixel(dest_x + px, dest_y + py, r, g, b, a);
            }
        }
    } else {
        // 普通字形：alpha 掩码与前景色混合
        let color_factor = if dim { 0.5 } else { 1.0 };
        let r = (color.r as f32 * color_factor) as u8;
        let g = (color.g as f32 * color_factor) as u8;
        let b = (color.b as f32 * color_factor) as u8;

        for py in 0..max_h {
            for px in 0..max_w {
                let alpha = atlas.sample_alpha(atlas_x + px, atlas_y + py);
                if alpha > 0 {
                    canvas.blend_pixel(dest_x + px, dest_y + py, r, g, b, alpha);
                }
            }
        }
    }
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
        let mut renderer = Renderer::new(RendererConfig::default());
        let cell_height = renderer.metrics().cell_height;

        // 准备 10 行空白 Cell
        let rows: Vec<Vec<RustXtermCell>> =
            (0..10).map(|_| vec![RustXtermCell::blank(); 5]).collect();

        // 仅渲染第 2、5、8 行（整行）
        let spans: Vec<DirtySpan> = [2usize, 5, 8]
            .iter()
            .map(|&r| DirtySpan {
                row: r,
                col_start: 0,
                col_end: rows[r].len(),
                cells: rows[r].clone(),
            })
            .collect();
        let result = renderer.render_frame(&spans);

        assert_eq!(
            result.dirty_rects.len(),
            spans.len(),
            "dirty_rects 数量应等于 span 数"
        );
        for (i, &row) in [2u32, 5, 8].iter().enumerate() {
            let rect = result.dirty_rects[i];
            assert_eq!(
                rect.1, // y
                row * cell_height,
                "row {row} 的脏矩形 y 应为 {row} * cell_height"
            );
        }
    }

    #[test]
    fn test_render_frame_partial_span() {
        let mut renderer = Renderer::new(RendererConfig::default());
        let cell_width = renderer.metrics().cell_width;

        // 一行 20 列空白
        let cells: Vec<RustXtermCell> = (0..20).map(|_| RustXtermCell::blank()).collect();

        // 仅渲染 col 5..15
        let span = DirtySpan {
            row: 3,
            col_start: 5,
            col_end: 15,
            cells: cells.clone(),
        };
        let result = renderer.render_frame(std::slice::from_ref(&span));

        assert_eq!(result.dirty_rects.len(), 1);
        let rect = result.dirty_rects[0];
        // x = col_start * cell_width
        assert_eq!(rect.0, 5 * cell_width);
        // width = (col_end - col_start) * cell_width
        assert_eq!(rect.2, 10 * cell_width);
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

    /// SubTask 1.4: 验证彩色 Emoji 光栅化输出 RGBA 数据
    ///
    /// 流程：
    /// 1. lookup U+1F600（😀）—— 由 font_tree 的 emoji 区段判定，is_color 应为 true
    /// 2. rasterize_glyph 应返回 RGBA（4 bytes/pixel）数据
    ///
    /// 在无 emoji 字体的 CI 环境下（lookup 返回 glyph_id==0 或 rasterize 落到
    /// Outline fallback），本测试会 skip 而非失败。
    #[test]
    fn test_color_emoji_rasterize_rgba() {
        let mut renderer = Renderer::new(RendererConfig::default());

        // 先 lookup 检查 emoji 字体可用性 + is_color 标志
        let info = match renderer.font_tree_mut().lookup_glyph('\u{1F600}', 2, false) {
            Some(info) => info,
            None => return, // skip：无主字体
        };
        if info.glyph_id == 0 {
            // 系统无 emoji 字体覆盖 U+1F600，跳过
            return;
        }
        // U+1F600 在 emoji 区段，font_tree 应强制 is_color=true
        assert!(
            info.is_color,
            "U+1F600 在 emoji 区段，lookup_glyph 应返回 is_color=true"
        );

        // rasterize：彩色源命中时返回 RGBA
        let pixels = match renderer.rasterize_glyph('\u{1F600}', info.is_color) {
            Some(p) => p,
            None => return, // skip：光栅化失败
        };
        // 若字体只有 outline 版本（无 COLR/CBDT），fallback 到 alpha 路径——跳过
        if !pixels.is_color {
            return;
        }
        // 彩色字形数据应为 RGBA（4 bytes/pixel）
        let expected_len = (pixels.width as usize) * (pixels.height as usize) * 4;
        assert_eq!(
            pixels.data.len(),
            expected_len,
            "彩色字形数据应为 RGBA（4 bytes/pixel），实际 {} 字节，预期 {}",
            pixels.data.len(),
            expected_len
        );
        // 应至少有一个非零像素（emoji 不是全透明）
        assert!(
            pixels.data.iter().any(|&b| b != 0),
            "彩色 emoji 光栅化后应至少有一个非零像素"
        );
    }

    /// SubTask 1.4: 验证彩色 Emoji 合成后画布像素为 emoji 原色（非前景色白色）
    ///
    /// 用 Color::WHITE 作为前景色渲染 emoji，若走彩色路径，画布上应出现
    /// 非纯白的彩色像素（emoji 自带的黄/红等颜色）。
    /// 若系统无彩色 emoji 字体，本测试 skip。
    #[test]
    fn test_color_emoji_composite() {
        let mut renderer = Renderer::new(RendererConfig::default());

        // 先确认系统有彩色 emoji 字体（否则跳过）
        let has_color_emoji = {
            let info = renderer.font_tree_mut().lookup_glyph('\u{1F600}', 2, false);
            match info {
                Some(info) if info.glyph_id > 0 && info.is_color => {
                    // 进一步确认 rasterize 真的输出 Color content
                    renderer
                        .rasterize_glyph('\u{1F600}', true)
                        .map(|p| p.is_color)
                        .unwrap_or(false)
                }
                _ => false,
            }
        };
        if !has_color_emoji {
            return; // skip：无彩色 emoji 字体
        }

        // 用白色前景渲染 emoji 单元格
        let cell = RustXtermCell {
            text: '\u{1F600}'.to_string(),
            width: 2,
            fg: Color::WHITE,
            bg: Color::BLACK,
            flags: CellFlags(0),
            hyperlink: None,
        };
        renderer.render_row(0, &[cell]);

        // 画布上应存在"真彩色"像素（emoji 原色，R/G/B 通道有显著差异）。
        // 关键区分：
        // - 正确路径（RGBA 直写）：emoji 黄色面部像素如 (255, 200, 0)，
        //   R/G/B 通道值差异显著（非灰度）。
        // - 错误路径（alpha × 白色前景）：emoji alpha 掩码与白色混合后
        //   产生灰度像素 (n, n, n)，R == G == B。
        // 用 max-min > 阈值的方式严格区分两条路径，确保测试在颜色路径
        // 被破坏时（如 `if false && entry.is_color`）会真正失败。
        let canvas = renderer.canvas();
        let mut found_color = false;
        for py in 0..canvas.height() {
            for px in 0..canvas.width() {
                let (r, g, b, _) = canvas.get_pixel(px, py);
                let max_ch = r.max(g).max(b);
                let min_ch = r.min(g).min(b);
                // 阈值 16：容忍 NotoColorEmoji 内部少量中间色调，
                // 同时排除灰度像素（max-min == 0）
                if max_ch.saturating_sub(min_ch) > 16 {
                    found_color = true;
                    break;
                }
            }
            if found_color {
                break;
            }
        }
        assert!(
            found_color,
            "彩色 emoji 合成后应有真彩色像素（R/G/B 通道差异 > 16），\
             说明走了 RGBA 原色路径而非 alpha × 白色前景的灰度路径"
        );
    }

    /// SubTask 1.4: 验证 ASCII 字符仍走 alpha×前景色路径
    ///
    /// 渲染 'A'：lookup_glyph 应返回 is_color=false，rasterize 应输出
    /// 非彩色数据（alpha 掩码展开为 RGBA 后写入图集）。
    #[test]
    fn test_ascii_still_alpha() {
        let mut renderer = Renderer::new(RendererConfig::default());

        let info = match renderer.font_tree_mut().lookup_glyph('A', 1, false) {
            Some(info) => info,
            None => return, // skip：无主字体
        };
        if info.glyph_id == 0 {
            return; // skip：主字体不覆盖 'A'（极少见）
        }
        // ASCII 不在 emoji 区段，is_color 应为 false
        assert!(
            !info.is_color,
            "ASCII 'A' 不在 emoji 区段，is_color 应为 false"
        );

        // rasterize 应返回非彩色数据（走 Outline + Alpha 路径）
        let pixels = renderer
            .rasterize_glyph('A', info.is_color)
            .expect("rasterize 'A' 应成功");
        assert!(
            !pixels.is_color,
            "ASCII 'A' 光栅化后 is_color 应为 false（走 alpha 路径）"
        );

        // 渲染到画布并验证：白色前景的 'A' 应在画布上产生白色像素
        let cell = RustXtermCell {
            text: "A".to_string(),
            width: 1,
            fg: Color::WHITE,
            bg: Color::BLACK,
            flags: CellFlags(0),
            hyperlink: None,
        };
        renderer.render_row(0, &[cell]);

        let canvas = renderer.canvas();
        let metrics = renderer.metrics();
        let mut found_white = false;
        for py in 0..metrics.cell_height {
            for px in 0..metrics.cell_width {
                let (r, g, b, _) = canvas.get_pixel(px, py);
                if r == 255 && g == 255 && b == 255 {
                    found_white = true;
                    break;
                }
            }
            if found_white {
                break;
            }
        }
        assert!(
            found_white,
            "ASCII 'A' 用白色前景渲染后，画布应有白色像素（前景色混合路径）"
        );
    }

    /// SubTask 2.5: 连字禁用路径
    ///
    /// `enable_ligatures=false` 时 `render_row` 应走单字符路径（`render_cell_text`），
    /// 不调用 `shape_run` / `run_cache`。断言：渲染不 panic，且对 ASCII 文本
    /// 仍输出非零像素。
    #[test]
    fn test_ligature_disabled() {
        let config = RendererConfig {
            enable_ligatures: false,
            ..Default::default()
        };
        let mut renderer = Renderer::new(config);

        let cells: Vec<RustXtermCell> = "!="
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
        // 不 panic 即通过；同时验证画布至少有非零像素
        renderer.render_row(0, &cells);

        let canvas = renderer.canvas();
        let metrics = renderer.metrics();
        let mut found_non_zero = false;
        for py in 0..metrics.cell_height {
            for px in 0..(2 * metrics.cell_width) {
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
            "禁用连字时 ASCII 文本仍应在画布上产生非零像素"
        );
    }

    /// SubTask 2.5: 连字启用路径
    ///
    /// `enable_ligatures=true` 时 `render_row` 应走 run 整形路径。
    /// 断言：渲染不 panic，且对 ASCII 文本仍输出非零像素。
    /// 在无主字体的环境应 skip。
    #[test]
    fn test_ligature_enabled() {
        let mut renderer = Renderer::new(RendererConfig::default());
        // 先确认主字体可用且覆盖 ASCII，否则 skip
        let info = match renderer.font_tree_mut().lookup_glyph('!', 1, false) {
            Some(info) => info,
            None => return,
        };
        if info.glyph_id == 0 || info.face_index != 0 {
            return; // skip：主字体不覆盖 '!'，连字路径不会触发
        }

        let cells: Vec<RustXtermCell> = "!==>"
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
        // 不 panic 即通过
        renderer.render_row(0, &cells);

        // 验证画布有非零像素（连字路径也应在画布上画出字形）
        let canvas = renderer.canvas();
        let metrics = renderer.metrics();
        let mut found_non_zero = false;
        for py in 0..metrics.cell_height {
            for px in 0..(4 * metrics.cell_width) {
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
            "启用连字时 ASCII 文本应在画布上产生非零像素"
        );
    }

    /// SubTask 2.5: 连字 run_cache 命中
    ///
    /// 同一行渲染两次，第二次应命中 `run_cache`。验证不 panic 且
    /// 渲染结果一致（画布非零）。
    #[test]
    fn test_ligature_run_cache_hit() {
        let mut renderer = Renderer::new(RendererConfig::default());
        let info = match renderer.font_tree_mut().lookup_glyph('a', 1, false) {
            Some(info) => info,
            None => return,
        };
        if info.glyph_id == 0 || info.face_index != 0 {
            return; // skip
        }

        let cells: Vec<RustXtermCell> = "abc"
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
        // 第一次：miss，触发光栅化 + insert_run
        renderer.render_row(0, &cells);
        // 第二次：应命中 run_cache，不重做光栅化
        renderer.render_row(0, &cells);
        // 第三次再次确认稳定
        renderer.render_row(0, &cells);

        let canvas = renderer.canvas();
        let metrics = renderer.metrics();
        let mut found_non_zero = false;
        for py in 0..metrics.cell_height {
            for px in 0..(3 * metrics.cell_width) {
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
        assert!(found_non_zero, "run_cache 命中后画布仍应有非零像素");
        // 命中次数应 ≥ 1（第二次渲染命中 run_cache）
        assert!(
            renderer.stats().atlas_hits >= 1,
            "第二次渲染应命中 run_cache，atlas_hits={}",
            renderer.stats().atlas_hits
        );
    }

    /// SubTask 2.5: cluster → cell 映射
    #[test]
    fn test_cluster_to_cell_index_mapping() {
        // 模拟 run_str = "abc"，3 个 ASCII cell，byte_offsets = [0, 1, 2]
        let byte_offsets = [0u32, 1, 2];
        // cluster 0 -> cell 0
        assert_eq!(cluster_to_cell_index(0, &byte_offsets), Some(0));
        // cluster 1 -> cell 1
        assert_eq!(cluster_to_cell_index(1, &byte_offsets), Some(1));
        // cluster 2 -> cell 2
        assert_eq!(cluster_to_cell_index(2, &byte_offsets), Some(2));
        // cluster 超出范围（不应发生）-> 仍映射到最后一个 cell
        assert_eq!(cluster_to_cell_index(3, &byte_offsets), Some(2));

        // 空列表
        assert_eq!(cluster_to_cell_index(0, &[]), None);
    }

    /// SubTask 2.5: 多字节字符的 cluster 映射
    ///
    /// 如 run_str = "àb"（'à' 是 2 字节 UTF-8），byte_offsets = [0, 2]
    #[test]
    fn test_cluster_to_cell_index_multibyte() {
        let s = "àb";
        let mut offsets = Vec::new();
        let mut acc = 0u32;
        for ch in s.chars() {
            offsets.push(acc);
            acc += ch.len_utf8() as u32;
        }
        // offsets = [0, 2]
        assert_eq!(&offsets, &[0u32, 2]);
        // cluster 0 (à 的起始) -> cell 0
        assert_eq!(cluster_to_cell_index(0, &offsets), Some(0));
        // cluster 1 (à 的中间字节，不应发生) -> cell 0（最近的）
        assert_eq!(cluster_to_cell_index(1, &offsets), Some(0));
        // cluster 2 (b 的起始) -> cell 1
        assert_eq!(cluster_to_cell_index(2, &offsets), Some(1));
    }

    /// SubTask 9.5/9.6：render_image 把 RGBA 块 blit 到 canvas 对应像素区域
    #[test]
    fn test_render_image_blit() {
        use rust_xterm_core::ImagePlacement;
        let mut renderer = Renderer::new(RendererConfig::default());
        let metrics = renderer.metrics();
        renderer.clear();

        // 构造 2×3 像素的纯红 RGBA 图像，放置在 cell (row=1, col=2)
        let w: u32 = 2;
        let h: u32 = 3;
        let rgba: Vec<u8> = (0..(w * h)).flat_map(|_| [255u8, 0, 0, 255]).collect();
        let placement = ImagePlacement {
            rgba,
            width: w,
            height: h,
            row: 1,
            col: 2,
        };

        renderer.render_image(&placement);

        let canvas = renderer.canvas();
        let base_x = 2 * metrics.cell_width;
        let base_y = metrics.cell_height;
        // 所有 2×3 像素应为红色
        for py in 0..h {
            for px in 0..w {
                let (r, g, b, a) = canvas.get_pixel(base_x + px, base_y + py);
                assert_eq!(r, 255, "px={px} py={py} R 应为 255");
                assert_eq!(g, 0, "px={px} py={py} G 应为 0");
                assert_eq!(b, 0, "px={px} py={py} B 应为 0");
                assert_eq!(a, 255, "px={px} py={py} A 应为 255");
            }
        }
        // 图像左上角前一像素不应被改写（仍为 clear 后的背景色 BLACK）
        let (r, _, _, _) = canvas.get_pixel(base_x.saturating_sub(1), base_y);
        assert_eq!(r, 0, "图像区域外像素不应被改写");
    }

    /// SubTask 9.5：透明像素（alpha=0）跳过，不写入画布
    #[test]
    fn test_render_image_transparent_skip() {
        use rust_xterm_core::ImagePlacement;
        let mut renderer = Renderer::new(RendererConfig::default());
        renderer.clear();

        // 1×1 全透明像素
        let placement = ImagePlacement {
            rgba: vec![255, 0, 0, 0],
            width: 1,
            height: 1,
            row: 0,
            col: 0,
        };
        renderer.render_image(&placement);

        // 透明像素不写入，画布仍为 clear 后的黑色
        let (r, g, b, _) = renderer.canvas().get_pixel(0, 0);
        assert_eq!((r, g, b), (0, 0, 0), "透明像素不应写入画布");
    }

    /// SubTask 9.5：超出画布边界的像素被裁剪，不 panic
    #[test]
    fn test_render_image_clip_boundary() {
        use rust_xterm_core::ImagePlacement;
        let mut renderer = Renderer::new(RendererConfig::default());
        let canvas_w = renderer.canvas().width();
        let canvas_h = renderer.canvas().height();

        // 构造一个远超画布尺寸的图像，起始在画布外
        let rgba: Vec<u8> = (0..(4 * 4)).flat_map(|_| [255u8, 255, 0, 255]).collect();
        let placement = ImagePlacement {
            rgba,
            width: 4,
            height: 4,
            row: (canvas_h / renderer.metrics().cell_height + 10) as usize,
            col: (canvas_w / renderer.metrics().cell_width + 10) as usize,
        };
        // 不 panic 即通过
        renderer.render_image(&placement);
    }

    /// SubTask 8.4: 两个 Renderer 通过 `with_global_atlas` 挂载同一全局 atlas
    ///
    /// 验证 `OnceLock` 单例语义：相同进程内多次 `with_global_atlas` 调用
    /// 返回的 `Arc<Mutex<TextureAtlas>>` 指向同一底层分配（`Arc::ptr_eq`）。
    ///
    /// 注意：全局单例是进程级的，测试间共享。本测试仅验证 `Arc::ptr_eq`，
    /// 不依赖 global atlas 的具体内容（避免受其它测试插入字形影响）。
    #[test]
    fn test_global_atlas_shared() {
        use std::sync::Arc;
        let config = RendererConfig::default();
        let r1 = Renderer::with_global_atlas(config.clone());
        let r2 = Renderer::with_global_atlas(config);
        // 两者应共享同一全局 atlas
        assert!(Arc::ptr_eq(
            r1.global_atlas.as_ref().unwrap(),
            r2.global_atlas.as_ref().unwrap()
        ));
    }

    /// SubTask 8.4: 全局 atlas 实际共享命中
    ///
    /// renderer A 渲染 'X'（miss -> 光栅化 -> 插入 global atlas）后，
    /// renderer B 渲染 'X' 应命中 global atlas（atlas_hits 增加）。
    /// 在无主字体的环境应 skip。
    #[test]
    fn test_global_atlas_cross_instance_hit() {
        // 先确认主字体可用且覆盖 'X'，否则 skip
        let mut probe = Renderer::new(RendererConfig::default());
        let info = match probe.font_tree_mut().lookup_glyph('X', 1, false) {
            Some(info) => info,
            None => return,
        };
        if info.glyph_id == 0 {
            return; // skip：主字体不覆盖 'X'
        }
        drop(probe);

        let config = RendererConfig::default();
        let mut r1 = Renderer::with_global_atlas(config.clone());
        let mut r2 = Renderer::with_global_atlas(config);

        let cell = RustXtermCell {
            text: "X".to_string(),
            width: 1,
            fg: Color::WHITE,
            bg: Color::BLACK,
            flags: CellFlags(0),
            hyperlink: None,
        };
        // r1 渲染 'X'：global miss -> 光栅化 -> 插入 global
        r1.render_row(0, std::slice::from_ref(&cell));
        let r1_stats = r1.stats();
        assert!(
            r1_stats.atlas_misses >= 1,
            "r1 首次渲染应 miss global atlas"
        );

        // r2 渲染 'X'：应命中 r1 插入的 global atlas 条目
        r2.render_row(0, std::slice::from_ref(&cell));
        let r2_stats = r2.stats();
        assert!(
            r2_stats.atlas_hits >= 1,
            "r2 渲染 'X' 应命中 r1 插入到 global atlas 的条目，atlas_hits={}",
            r2_stats.atlas_hits
        );
    }
}
