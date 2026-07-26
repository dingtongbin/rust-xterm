//! 全局共享纹理图集
//!
//! 提供 [`GlobalAtlas`]，使用 `OnceLock<Arc<Mutex<TextureAtlas>>>` 实现
//! 跨 [`Renderer`](crate::renderer::Renderer) 实例共享同一份纹理图集，
//! 减少多终端实例的内存占用与重复光栅化。
//!
//! `OnceLock` / `Arc` / `Mutex` 均为安全抽象，无需 `unsafe`，
//! 与 `#![forbid(unsafe_code)]` 兼容。

use std::sync::{Arc, Mutex, OnceLock};

use crate::atlas::TextureAtlas;
use crate::renderer::RendererConfig;

/// 全局共享纹理图集容器
///
/// 使用 `OnceLock<Arc<Mutex<TextureAtlas>>>` 实现跨 Renderer 实例共享。
/// 多个 Renderer 可挂载同一全局 atlas，减少多终端实例的内存占用。
///
/// 保持 `#![forbid(unsafe_code)]`：OnceLock / Arc / Mutex 均为安全抽象。
pub struct GlobalAtlas {
    inner: OnceLock<Arc<Mutex<TextureAtlas>>>,
}

impl GlobalAtlas {
    /// 创建新的全局图集容器（尚未初始化）
    pub const fn new() -> Self {
        Self {
            inner: OnceLock::new(),
        }
    }

    /// 获取或初始化全局图集
    ///
    /// 首次调用用 `config` 创建 TextureAtlas，后续调用返回已有实例。
    /// 多个 Renderer 通过相同 `config` 挂载时，会得到同一份 `Arc`。
    pub fn get_or_init(&self, config: RendererConfig) -> Arc<Mutex<TextureAtlas>> {
        self.inner
            .get_or_init(|| {
                let atlas = TextureAtlas::new(
                    config.atlas_width,
                    config.atlas_height,
                    4, // RGBA，与 Renderer::new 一致
                    1024,
                );
                Arc::new(Mutex::new(atlas))
            })
            .clone()
    }

    /// 尝试获取已初始化的全局图集
    ///
    /// 若尚未调用过 `get_or_init`，返回 `None`。
    pub fn try_get(&self) -> Option<Arc<Mutex<TextureAtlas>>> {
        self.inner.get().cloned()
    }
}

impl Default for GlobalAtlas {
    fn default() -> Self {
        Self::new()
    }
}

/// 进程级全局图集单例
static GLOBAL_ATLAS: GlobalAtlas = GlobalAtlas::new();

/// 获取进程级全局图集单例
pub fn global_atlas() -> &'static GlobalAtlas {
    &GLOBAL_ATLAS
}
