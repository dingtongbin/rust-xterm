//! 图像数据模型
//!
//! 提供 [`ImagePlacement`] 描述一幅已放置到终端 grid 上的 RGBA 图像，
//! 以及 [`ImageStore`] 管理所有已放置图像。
//!
//! 由 Sixel 解析器（[`crate::sixel`]) 与 iTerm2 inline image 协议复用。

/// 图像放置信息
#[derive(Debug, Clone)]
pub struct ImagePlacement {
    /// RGBA 像素数据（4 字节/像素，行优先，无 padding）
    pub rgba: Vec<u8>,
    /// 宽度（像素）
    pub width: u32,
    /// 高度（像素）
    pub height: u32,
    /// 起始行（cell 行，0-based）
    pub row: usize,
    /// 起始列（cell 列，0-based）
    pub col: usize,
}

/// 图像存储
#[derive(Debug, Default)]
pub struct ImageStore {
    /// 已放置的图像列表
    pub placements: Vec<ImagePlacement>,
}

impl ImageStore {
    /// 创建空的图像存储
    pub fn new() -> Self {
        Self::default()
    }

    /// 追加一个图像放置
    pub fn add(&mut self, p: ImagePlacement) {
        self.placements.push(p);
    }

    /// 清空所有图像
    pub fn clear(&mut self) {
        self.placements.clear();
    }

    /// 获取覆盖指定行列的图像（按起始行列匹配）
    pub fn at(&self, row: usize, col: usize) -> Option<&ImagePlacement> {
        self.placements
            .iter()
            .find(|p| p.row == row && p.col == col)
    }

    /// 当前存储的图像数量
    pub fn len(&self) -> usize {
        self.placements.len()
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.placements.is_empty()
    }

    /// 获取所有图像的不可变切片
    pub fn placements(&self) -> &[ImagePlacement] {
        &self.placements
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placement(row: usize, col: usize, w: u32, h: u32) -> ImagePlacement {
        ImagePlacement {
            rgba: vec![0u8; (w * h * 4) as usize],
            width: w,
            height: h,
            row,
            col,
        }
    }

    #[test]
    fn test_store_add_and_at() {
        let mut store = ImageStore::new();
        assert!(store.is_empty());
        store.add(placement(0, 0, 2, 6));
        store.add(placement(3, 4, 4, 6));
        assert_eq!(store.len(), 2);
        assert!(store.at(0, 0).is_some());
        assert!(store.at(3, 4).is_some());
        assert!(store.at(1, 1).is_none());
    }

    #[test]
    fn test_store_clear() {
        let mut store = ImageStore::new();
        store.add(placement(0, 0, 1, 1));
        assert!(!store.is_empty());
        store.clear();
        assert!(store.is_empty());
    }
}
