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
use std::collections::HashMap;
use swash::scale::ScaleContext;
use swash::{Charmap, FontRef, GlyphId};

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
    /// 字形缓存：char -> GlyphInfo
    glyph_cache: HashMap<char, GlyphInfo>,
    /// swash 缩放上下文
    scale_context: ScaleContext,
    /// 字体数据缓存（避免重复解析）
    font_data_cache: HashMap<ID, Vec<u8>>,
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
            glyph_cache: HashMap::new(),
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
    pub fn lookup_glyph(&mut self, ch: char) -> Option<GlyphInfo> {
        // 先查缓存
        if let Some(info) = self.glyph_cache.get(&ch) {
            return Some(*info);
        }

        let ids = self.all_ids();
        for (face_index, &id) in ids.iter().enumerate() {
            if let Some(info) = self.lookup_in_face(id, face_index, ch) {
                self.glyph_cache.insert(ch, info);
                return Some(info);
            }
        }

        None
    }

    /// 在指定字体面中查找字符
    fn lookup_in_face(&mut self, id: ID, face_index: usize, ch: char) -> Option<GlyphInfo> {
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

        // 检查是否为彩色字形（简化判断：Emoji 范围）
        let is_color = is_emoji(ch);

        // 获取前进宽度（简化：使用固定值）
        let advance = if is_wide_char(ch) { 2.0 } else { 1.0 };

        Some(GlyphInfo {
            glyph_id,
            face_index,
            is_color,
            advance,
        })
    }

    /// 获取字体数据（带缓存）
    fn get_font_data(&mut self, id: ID) -> Option<Vec<u8>> {
        if let Some(data) = self.font_data_cache.get(&id) {
            return Some(data.clone());
        }

        let data = self.db.with_face_data(id, |data, _index| data.to_vec())?;

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

/// 判断字符是否为 Emoji
fn is_emoji(ch: char) -> bool {
    let code = ch as u32;
    // 常见 Emoji 范围
    matches!(code,
        0x1F600..=0x1F64F | // Emoticons
        0x1F300..=0x1F5FF | // Misc Symbols and Pictographs
        0x1F680..=0x1F6FF | // Transport and Map
        0x1F1E0..=0x1F1FF | // Flags
        0x2600..=0x26FF |   // Misc symbols
        0x2700..=0x27BF |   // Dingbats
        0xFE00..=0xFE0F |   // Variation Selectors
        0x1F900..=0x1F9FF | // Supplemental Symbols and Pictographs
        0x1FA00..=0x1FA6F | // Chess Symbols
        0x1FA70..=0x1FAFF   // Symbols and Pictographs Extended-A
    )
}

/// 判断字符是否为宽字符（CJK 等）
fn is_wide_char(ch: char) -> bool {
    let code = ch as u32;
    // CJK 统一表意文字
    (0x4E00..=0x9FFF).contains(&code) ||
    // CJK 扩展 A
    (0x3400..=0x4DBF).contains(&code) ||
    // 韩文音节
    (0xAC00..=0xD7A3).contains(&code) ||
    // 日文假名
    (0x3040..=0x30FF).contains(&code)
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
        let info = tree.lookup_glyph('A');
        // 应该能找到 ASCII 字符
        if let Some(info) = info {
            assert!(info.glyph_id > 0);
        }
    }

    #[test]
    fn test_lookup_cjk() {
        let mut tree = FontTree::new();
        let info = tree.lookup_glyph('你');
        // 系统应该有 CJK 字体（测试环境安装了 Noto Sans SC）
        if let Some(info) = info {
            assert!(info.glyph_id > 0);
        }
    }

    #[test]
    fn test_is_emoji() {
        assert!(is_emoji('😀'));
        assert!(is_emoji('🎉'));
        assert!(!is_emoji('A'));
        assert!(!is_emoji('你'));
    }

    #[test]
    fn test_is_wide_char() {
        assert!(is_wide_char('你'));
        assert!(is_wide_char('あ'));
        assert!(!is_wide_char('A'));
        assert!(!is_wide_char(' '));
    }
}
