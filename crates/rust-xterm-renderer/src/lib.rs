//! # rust-xterm-renderer: 渲染后端
//!
//! 基于 swash 文本整形与光栅化引擎，提供：
//! - [`TextureAtlas`]：纹理图集（静态区 + LRU 动态区）
//! - [`FontTree`]：字体树（主字体 + 系统回退链）
//! - [`Renderer`]：渲染引擎（光栅化 + 合成 + 脏区输出）
//!
//! ## 设计原则
//!
//! - **固定内存**：图集使用预分配的 `Box<[u8]>`，运行时零分配
//! - **LRU 淘汰**：动态区使用 LRU 策略，防止图集溢出
//! - **绝不缓存颜色**：图集只存储 Alpha 掩码，颜色在合成阶段混合
//! - **Emoji 特殊路径**：检测 Unicode Range -> 走 ColorGlyph 路径 -> 直接写入 RGBA

#![forbid(unsafe_code)]
#![warn(missing_docs)]

// ============================================================================
// 模块声明
// ============================================================================

/// 纹理图集
pub mod atlas;

/// 字体树
pub mod font_tree;

/// 渲染引擎
pub mod renderer;

/// 像素缓冲区
pub mod canvas;

/// 全局共享纹理图集（跨 Renderer 实例 LRU）
pub mod global_atlas;

// ============================================================================
// 公共重导出
// ============================================================================

pub use atlas::{AtlasEntry, AtlasStats, TextureAtlas};
pub use canvas::{Canvas, PixelFormat};
pub use font_tree::{FontFace, FontTree, GlyphInfo, ShapeGlyph};
pub use global_atlas::{global_atlas, GlobalAtlas};
pub use renderer::{RenderMetrics, RenderResult, Renderer, RendererConfig};
