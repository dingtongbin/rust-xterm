//! 字体树
//!
//! 加载主字体 + 系统回退链。
//!
//! ## 职责
//!
//! - 使用 `fontdb` 扫描系统字体
//! - 使用 `swash` 解析字体数据
//! - 提供 `char -> GlyphId` 映射
//! - 处理 Emoji 彩色渲染特殊路径
//!
//! ## 回退策略
//!
//! 1. 尝试主字体
//! 2. 若主字体不包含该字符，遍历回退链
//! 3. 若所有字体都不包含，使用 `.notdef` 字形（通常是方块）

use fontdb::{Database, Family, Query, Style, Weight, ID};
use lru::LruCache;
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::Arc;
use swash::scale::ScaleContext;
use swash::{Charmap, FontRef, GlyphId};

/// 字形缓存硬上限
///
/// 防止渲染大量不同字符时 `glyph_cache` 无界增长。
/// 8192 足以覆盖绝大多数终端会话中出现的字符集。
const GLYPH_CACHE_CAPACITY: usize = 8192;

/// 字体面信息
#[derive(Debug, Clone)]
pub struct FontFace {
    /// fontdb 中的 ID
    pub id: ID,
    /// 字体家族名
    pub family: String,
    /// 是否支持彩色字形
    pub has_color: bool,
}

/// 字形信息
#[derive(Debug, Clone, Copy)]
pub struct GlyphInfo {
    /// 字形 ID
    pub glyph_id: GlyphId,
    /// 字体面在回退链中的索引
    pub face_index: usize,
    /// 是否为彩色字形
    pub is_color: bool,
    /// 字形前进宽度（字体单位）
    pub advance: f32,
}

/// 字体树
///
/// 管理主字体 + 系统回退链，提供字符到字形的映射。
pub struct FontTree {
    /// fontdb 数据库
    db: Database,
    /// 主字体 ID
    primary_id: Option<ID>,
    /// 回退字体 ID 列表
    fallback_ids: Vec<ID>,
    /// 字形缓存：char -> GlyphInfo（LRU 有界，硬上限 GLYPH_CACHE_CAPACITY）
    glyph_cache: LruCache<char, GlyphInfo>,
    /// swash 缩放上下文
    scale_context: ScaleContext,
    /// 字体数据缓存（避免重复解析）
    ///
    /// 使用 `Arc<[u8]>` 共享字体文件字节，避免每个调用方都 clone 整份字体数据。
    /// `Arc` clone 是廉价的引用计数 clone，不复制底层字节。
    font_data_cache: HashMap<ID, Arc<[u8]>>,
}

impl FontTree {
    /// 创建新的字体树，自动加载系统字体
    pub fn new() -> Self {
        let mut db = Database::new();
        db.load_system_fonts();

        // 尝试查找一个默认的等宽字体作为主字体
        let primary_id = Self::find_monospace(&db);

        // 收集回退字体（CJK + Emoji）
        let mut fallback_ids = Vec::new();
        for family in &[
            "Noto Sans SC",
            "Noto Sans CJK SC",
            "Noto Color Emoji",
            "DejaVu Sans",
            "Liberation Mono",
        ] {
            let id = db.query(&Query {
                families: &[Family::Name(family)],
                weight: Weight::NORMAL,
                style: Style::Normal,
                ..Default::default()
            });
            if let Some(id) = id {
                if !fallback_ids.contains(&id) {
                    fallback_ids.push(id);
                }
            }
        }

        Self {
            db,
            primary_id,
            fallback_ids,
            glyph_cache: LruCache::new(NonZeroUsize::new(GLYPH_CACHE_CAPACITY).unwrap()),
            scale_context: ScaleContext::new(),
            font_data_cache: HashMap::new(),
        }
    }

    /// 查找系统等宽字体
    fn find_monospace(db: &Database) -> Option<ID> {
        for family in &[
            "DejaVu Sans Mono",
            "Liberation Mono",
            "Noto Sans Mono",
            "Courier New",
            "monospace",
        ] {
            let id = db.query(&Query {
                families: &[Family::Name(family)],
                weight: Weight::NORMAL,
                style: Style::Normal,
                ..Default::default()
            });
            if id.is_some() {
                return id;
            }
        }

        // 如果没找到，返回第一个可用字体
        db.faces().next().map(|face| face.id)
    }

    /// 获取主字体 ID
    pub fn primary_id(&self) -> Option<ID> {
        self.primary_id
    }

    /// 获取回退字体 ID 列表
    pub fn fallback_ids(&self) -> &[ID] {
        &self.fallback_ids
    }

    /// 获取所有字体 ID（主字体 + 回退）
    pub fn all_ids(&self) -> Vec<ID> {
        let mut ids = Vec::new();
        if let Some(primary) = self.primary_id {
            ids.push(primary);
        }
        ids.extend_from_slice(&self.fallback_ids);
        ids
    }

    /// 查找字符对应的字形
    ///
    /// 遍历主字体 + 回退链，返回第一个包含该字符的字体中的字形 ID。
    ///
    /// # 参数
    ///
    /// - `ch`：待查找的字符
    /// - `width`：字符显示宽度（来自 `cell.width`，WezTerm 权威宽度表），
    ///   用于填充 `GlyphInfo.advance`
    /// - `is_color`：是否按彩色字形处理（由调用方根据 cell flags 等提示判定）
    ///
    /// # 返回
    ///
    /// - 若某字体包含该字符，返回对应字形的 `GlyphInfo`
    /// - 若所有字体都 miss，但有主字体，返回主字体的 `.notdef` 字形
    ///   （`glyph_id == 0`），由渲染层画方块
    /// - 若没有主字体，返回 `None`
    pub fn lookup_glyph(&mut self, ch: char, width: usize, is_color: bool) -> Option<GlyphInfo> {
        // 先查缓存（LruCache::get 需要 &mut self 以更新 LRU 顺序）
        if let Some(info) = self.glyph_cache.get(&ch) {
            return Some(*info);
        }

        let ids = self.all_ids();
        for (face_index, &id) in ids.iter().enumerate() {
            if let Some(info) = self.lookup_in_face(id, face_index, ch, width, is_color) {
                self.glyph_cache.put(ch, info);
                return Some(info);
            }
        }

        // 所有字体都 miss：返回主字体的 .notdef 字形（glyph_id == 0），
        // 由渲染层画方块。如果没有主字体，则返回 None。
        self.primary_id?;
        let info = GlyphInfo {
            glyph_id: 0,   // .notdef
            face_index: 0, // 主字体在 all_ids() 中的索引
            is_color: false,
            advance: width as f32,
        };
        self.glyph_cache.put(ch, info);
        Some(info)
    }

    /// 在指定字体面中查找字符
    ///
    /// `width` 与 `is_color` 由调用方传入，避免在字体树中硬编码 Unicode 宽度判定。
    fn lookup_in_face(
        &mut self,
        id: ID,
        face_index: usize,
        ch: char,
        width: usize,
        is_color: bool,
    ) -> Option<GlyphInfo> {
        // 获取字体数据
        let data = self.get_font_data(id)?;

        // 解析字体
        let font = FontRef::from_index(&data, 0)?;

        // 创建字符映射
        let charmap = Charmap::from_font(&font);

        // 查找字形 ID
        let glyph_id = charmap.map(ch);

        // glyph_id 为 0 表示字符不存在
        if glyph_id == 0 {
            return None;
        }

        // 字符显示宽度由调用方（基于 WezTerm 权威结果）传入
        let advance = width as f32;

        Some(GlyphInfo {
            glyph_id,
            face_index,
            is_color,
            advance,
        })
    }

    /// 获取字体数据（带缓存）
    ///
    /// 返回 `Arc<[u8]>` clone，是廉价的引用计数 clone，不复制底层字体字节。
    fn get_font_data(&mut self, id: ID) -> Option<Arc<[u8]>> {
        if let Some(data) = self.font_data_cache.get(&id) {
            // Arc clone：仅增加引用计数，不复制字节
            return Some(data.clone());
        }

        let data: Vec<u8> = self.db.with_face_data(id, |data, _index| data.to_vec())?;
        // Vec<u8> -> Arc<[u8]>：一次性分配引用计数，后续 clone 零拷贝
        let data: Arc<[u8]> = data.into();
        self.font_data_cache.insert(id, data.clone());
        Some(data)
    }

    /// 获取 swash 缩放上下文
    pub fn scale_context(&mut self) -> &mut ScaleContext {
        &mut self.scale_context
    }

    /// 获取可变的缩放上下文（scale_context 的别名）
    pub fn scale_context_mut(&mut self) -> &mut ScaleContext {
        &mut self.scale_context
    }

    /// 获取 fontdb 数据库引用
    pub fn database(&self) -> &Database {
        &self.db
    }

    /// 获取所有字体面信息
    pub fn faces(&self) -> impl Iterator<Item = FontFace> + '_ {
        self.db.faces().map(|face| FontFace {
            id: face.id,
            family: face
                .families
                .first()
                .map(|(name, _)| name.clone())
                .unwrap_or_default(),
            has_color: false,
        })
    }
}

impl Default for FontTree {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_font_tree_creation() {
        let tree = FontTree::new();
        // 系统应该至少有一个字体
        assert!(tree.primary_id.is_some() || tree.faces().count() > 0);
    }

    #[test]
    fn test_lookup_ascii() {
        let mut tree = FontTree::new();
        let info = tree.lookup_glyph('A', 1, false);
        // 应该能找到 ASCII 字符
        if let Some(info) = info {
            assert!(info.glyph_id > 0);
        }
    }

    #[test]
    fn test_lookup_cjk() {
        let mut tree = FontTree::new();
        // '你' 是 CJK 宽字符，width=2 来自 WezTerm 权威宽度表
        let info = tree.lookup_glyph('你', 2, false);
        // 系统若有 CJK 字体（如 Noto Sans SC），glyph_id > 0；
        // 否则 Task 12 的 .notdef 回退保证返回 Some(glyph_id == 0) 而非 None。
        if tree.primary_id.is_some() {
            assert!(info.is_some(), "有主字体时，缺字应返回 .notdef 而非 None");
        }
    }

    #[test]
    fn test_glyph_cache_bounded() {
        // 验证 glyph_cache 有硬上限 GLYPH_CACHE_CAPACITY (8192)
        // 插入超过上限数量的不同字符后，缓存大小不应超过上限
        let mut tree = FontTree::new();
        // 使用 BMP PUA 区字符，避免与 ASCII 等常用字符冲突
        // 0xE000..=0xF8FF 是 BMP PUA（6400 个），剩余从补充区取
        for i in 0..(GLYPH_CACHE_CAPACITY + 1000) as u32 {
            // 跳过代理区 0xD800..=0xDFFF
            let code = if i < 0x1900 {
                0xE000 + i
            } else {
                0x10000 + (i - 0x1900)
            };
            if let Some(ch) = char::from_u32(code) {
                let _ = tree.lookup_glyph(ch, 1, false);
            }
        }
        assert!(
            tree.glyph_cache.len() <= GLYPH_CACHE_CAPACITY,
            "glyph_cache 应有硬上限 {}，实际 {}",
            GLYPH_CACHE_CAPACITY,
            tree.glyph_cache.len()
        );
    }
}
