//! DamageTracker：脏区追踪器
//!
//! 记录哪些行发生了变更，并将连续的脏行合并为矩形，
//! 输出给渲染器。
//!
//! ## 数据结构
//!
//! 使用 `Vec<bool>` 作为行级脏标记（比 `BitVec` 更简单，
//! 且在行数 < 1000 时性能相当）。每次 `advance_bytes` 后，
//! 上层调用 `mark_dirty(row)` 标记变更行。
//!
//! 子行/列级脏区通过 `col_spans: BTreeMap<usize, Vec<(usize, usize)>>`
//! 维护：行号 -> 已合并、互不重叠的列区间列表。整行脏（`mark_dirty`）
//! 优先生效，会清除该行的 `col_spans`。
//!
//! ## 合并策略
//!
//! 将连续的脏行合并为一个 `DirtyRect`（覆盖完整宽度）。
//! 这是最简单且高效的策略，因为终端变更通常是行级的。
//!
//! ## 生命周期
//!
//! 1. `mark_dirty(row)` — 标记某行为脏（整行）
//! 2. `mark_dirty_range(start, end)` — 标记行范围为脏
//! 3. `mark_span_dirty(row, cs, ce)` — 标记子行/列区间为脏
//! 4. `mark_cell_dirty(row, col)` — 标记单 cell 为脏
//! 5. `mark_all_dirty()` — 标记全部为脏（如 resize 后）
//! 6. `drain_rects()` — 提取并清空所有脏矩形
//! 7. `drain_spans()` — 提取所有脏 span（行级 + 列级）
//! 8. `is_empty()` — 检查是否有脏区

use std::collections::BTreeMap;

/// 脏矩形
///
/// 表示一个需要重绘的矩形区域。
/// 对于行级变更，x=0, width=cols（覆盖完整宽度）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirtyRect {
    /// 起始列（0-based）
    pub x: usize,
    /// 起始行（0-based）
    pub y: usize,
    /// 宽度（列数）
    pub width: usize,
    /// 高度（行数）
    pub height: usize,
}

impl DirtyRect {
    /// 创建新的脏矩形
    pub const fn new(x: usize, y: usize, width: usize, height: usize) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// 创建覆盖完整宽度的行级脏矩形
    pub const fn full_width(y: usize, cols: usize, height: usize) -> Self {
        Self::new(0, y, cols, height)
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// 面积
    pub fn area(&self) -> usize {
        self.width * self.height
    }
}

/// 脏区追踪器
///
/// 维护行级脏标记，支持合并连续脏行为矩形。
pub struct DamageTracker {
    /// 行级脏标记
    dirty_rows: Vec<bool>,
    /// 子行/列级脏区：行号 -> 列区间列表（已合并、互不重叠）
    col_spans: BTreeMap<usize, Vec<(usize, usize)>>,
    /// 列数（用于生成完整宽度矩形）
    cols: usize,
    /// 行数
    rows: usize,
    /// 是否有未提取的脏区
    has_damage: bool,
}

impl DamageTracker {
    /// 创建新的脏区追踪器
    pub fn new(rows: usize, cols: usize) -> Self {
        Self {
            dirty_rows: vec![false; rows],
            col_spans: BTreeMap::new(),
            cols,
            rows,
            has_damage: false,
        }
    }

    /// 调整尺寸
    ///
    /// resize 后所有内容都可能错位，因此标记全部为脏。
    pub fn resize(&mut self, rows: usize, cols: usize) {
        self.dirty_rows = vec![false; rows];
        self.col_spans.clear();
        self.rows = rows;
        self.cols = cols;
        // resize 后标记全部为脏
        self.mark_all_dirty();
    }

    /// 标记某行为脏
    ///
    /// 整行脏会覆盖该行已存在的子行/列级 span。
    pub fn mark_dirty(&mut self, row: usize) {
        if row < self.dirty_rows.len() && !self.dirty_rows[row] {
            self.dirty_rows[row] = true;
            // 整行脏覆盖子行/列级 span
            self.col_spans.remove(&row);
            self.has_damage = true;
        }
    }

    /// 标记行范围为脏
    pub fn mark_dirty_range(&mut self, start: usize, end: usize) {
        for row in start..end.min(self.dirty_rows.len()) {
            self.mark_dirty(row);
        }
    }

    /// 标记全部为脏
    pub fn mark_all_dirty(&mut self) {
        for slot in &mut self.dirty_rows {
            *slot = true;
        }
        self.col_spans.clear();
        self.has_damage = true;
    }

    /// 标记单 cell 为脏（等价于 mark_span_dirty(row, col, col+1)）
    pub fn mark_cell_dirty(&mut self, row: usize, col: usize) {
        self.mark_span_dirty(row, col, col + 1);
    }

    /// 标记行列区间为脏
    ///
    /// 坐标 0-based，半开区间 `[col_start, col_end)`。若该行已被 `mark_dirty` 标记
    /// 为整行脏，则本调用为 no-op（整行已覆盖）。否则将区间合并到 `col_spans[row]`。
    pub fn mark_span_dirty(&mut self, row: usize, col_start: usize, col_end: usize) {
        if row >= self.rows {
            return;
        }
        // 整行脏已覆盖，跳过
        if row < self.dirty_rows.len() && self.dirty_rows[row] {
            return;
        }
        let cs = col_start.min(self.cols);
        let ce = col_end.min(self.cols);
        if cs >= ce {
            return;
        }
        let spans = self.col_spans.entry(row).or_default();
        spans.push((cs, ce));
        // 合并重叠/相邻区间
        spans.sort_by_key(|&(s, _)| s);
        let mut merged: Vec<(usize, usize)> = Vec::with_capacity(spans.len());
        for &(s, e) in spans.iter() {
            if let Some(last) = merged.last_mut() {
                if s <= last.1 {
                    last.1 = last.1.max(e);
                    continue;
                }
            }
            merged.push((s, e));
        }
        *spans = merged;
        self.has_damage = true;
    }

    /// 提取所有脏 span（行级 + 列级），返回 `(row, col_start, col_end)` 列表
    ///
    /// - 整行脏（来自 `mark_dirty`）：emit `(row, 0, cols)`
    /// - 列级脏（来自 `mark_span_dirty`）：emit 每个合并后的 `(row, cs, ce)`
    ///
    /// 清空 `dirty_rows` 与 `col_spans`，重置 `has_damage`。
    pub fn drain_spans(&mut self) -> Vec<(usize, usize, usize)> {
        let mut out = Vec::new();
        // 行级脏
        for (i, &d) in self.dirty_rows.iter().enumerate() {
            if d {
                out.push((i, 0, self.cols));
            }
        }
        // 列级脏
        for (row, spans) in self.col_spans.iter() {
            for &(s, e) in spans.iter() {
                out.push((*row, s, e));
            }
        }
        // 清空
        for slot in &mut self.dirty_rows {
            *slot = false;
        }
        self.col_spans.clear();
        self.has_damage = false;
        out
    }

    /// 检查是否有脏区
    pub fn is_empty(&self) -> bool {
        !self.has_damage
    }

    /// 提取并清空所有脏矩形
    ///
    /// 将连续的脏行合并为矩形，然后清空所有标记。
    pub fn drain_rects(&mut self) -> Vec<DirtyRect> {
        if !self.has_damage {
            return Vec::new();
        }

        let mut rects = Vec::new();
        let mut i = 0;

        while i < self.dirty_rows.len() {
            if self.dirty_rows[i] {
                // 找到连续脏行的起始
                let start = i;
                while i < self.dirty_rows.len() && self.dirty_rows[i] {
                    i += 1;
                }
                let height = i - start;
                rects.push(DirtyRect::full_width(start, self.cols, height));
            } else {
                i += 1;
            }
        }

        // 清空标记
        for slot in &mut self.dirty_rows {
            *slot = false;
        }
        self.col_spans.clear();
        self.has_damage = false;

        rects
    }

    /// 获取脏行数量（不提取）
    pub fn dirty_row_count(&self) -> usize {
        self.dirty_rows.iter().filter(|&&d| d).count()
    }

    /// 获取当前尺寸
    pub fn size(&self) -> (usize, usize) {
        (self.rows, self.cols)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mark_and_drain() {
        let mut tracker = DamageTracker::new(24, 80);
        assert!(tracker.is_empty());

        tracker.mark_dirty(5);
        tracker.mark_dirty(6);
        tracker.mark_dirty(7);
        assert!(!tracker.is_empty());

        let rects = tracker.drain_rects();
        assert_eq!(rects.len(), 1);
        assert_eq!(rects[0], DirtyRect::full_width(5, 80, 3));

        assert!(tracker.is_empty());
    }

    #[test]
    fn test_non_contiguous() {
        let mut tracker = DamageTracker::new(24, 80);
        tracker.mark_dirty(2);
        tracker.mark_dirty(3);
        tracker.mark_dirty(10);
        tracker.mark_dirty(11);
        tracker.mark_dirty(12);

        let rects = tracker.drain_rects();
        assert_eq!(rects.len(), 2);
        assert_eq!(rects[0], DirtyRect::full_width(2, 80, 2));
        assert_eq!(rects[1], DirtyRect::full_width(10, 80, 3));
    }

    #[test]
    fn test_mark_all() {
        let mut tracker = DamageTracker::new(10, 80);
        tracker.mark_all_dirty();

        let rects = tracker.drain_rects();
        assert_eq!(rects.len(), 1);
        assert_eq!(rects[0], DirtyRect::full_width(0, 80, 10));
    }

    #[test]
    fn test_resize_marks_all_dirty() {
        let mut tracker = DamageTracker::new(24, 80);
        tracker.resize(30, 100);

        assert!(!tracker.is_empty());
        let rects = tracker.drain_rects();
        assert_eq!(rects.len(), 1);
        assert_eq!(rects[0], DirtyRect::full_width(0, 100, 30));
    }

    #[test]
    fn test_mark_span_dirty_basic() {
        let mut tracker = DamageTracker::new(24, 80);
        tracker.mark_span_dirty(5, 10, 20);
        let spans = tracker.drain_spans();
        assert_eq!(spans, vec![(5, 10, 20)]);
    }

    #[test]
    fn test_mark_span_merge_adjacent() {
        let mut tracker = DamageTracker::new(24, 80);
        tracker.mark_span_dirty(5, 10, 15);
        tracker.mark_span_dirty(5, 14, 20); // overlap
        tracker.mark_span_dirty(5, 30, 35);
        let spans = tracker.drain_spans();
        assert_eq!(spans, vec![(5, 10, 20), (5, 30, 35)]);
    }

    #[test]
    fn test_mark_dirty_overrides_spans() {
        let mut tracker = DamageTracker::new(24, 80);
        tracker.mark_span_dirty(5, 10, 20);
        tracker.mark_dirty(5); // 整行覆盖，应清空 spans
        tracker.mark_span_dirty(5, 30, 40); // 整行已脏，应 no-op
        let spans = tracker.drain_spans();
        assert_eq!(spans, vec![(5, 0, 80)]);
    }

    #[test]
    fn test_mark_cell_dirty() {
        let mut tracker = DamageTracker::new(24, 80);
        tracker.mark_cell_dirty(3, 7);
        let spans = tracker.drain_spans();
        assert_eq!(spans, vec![(3, 7, 8)]);
    }

    #[test]
    fn test_drain_spans_mixed() {
        let mut tracker = DamageTracker::new(24, 80);
        tracker.mark_dirty(2);
        tracker.mark_span_dirty(5, 10, 20);
        let mut spans = tracker.drain_spans();
        spans.sort();
        assert_eq!(spans, vec![(2, 0, 80), (5, 10, 20)]);
    }
}
