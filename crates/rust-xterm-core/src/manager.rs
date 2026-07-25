//! TerminalManager：终极结构体
//!
//! 所有子系统的所有者，通过 `&mut self` 保证线程安全。
//!
//! ## 数据流
//!
//! ### 输入流
//!
//! ```text
//! SSH/PTY 收到数据
//!   → manager.write(raw_bytes)
//!   → codec.decode(raw_bytes) → UTF-8 String
//!   → core.advance_bytes(utf8_str)
//!   → WezTerm 内部解析 ANSI，更新 Grid
//!   → damage.mark_dirty(changed_rows)
//!   → 返回（不渲染）
//! ```
//!
//! ### 渲染流
//!
//! ```text
//! Slint 定时器触发
//!   → manager.poll_frame(now)
//!   → 懒检查：damage.is_empty() && !blink_due → None
//!   → 若有变更：进入渲染管线
//!   → 返回 FrameUpdate { dirty_rects, cursor_meta, cells }
//! ```

use crate::addon::{Addon, AddonContext};
use crate::buffer::{Buffer, BufferNamespace, BufferType, Marker};
use crate::codec_gate::{Codec, CodecGate};
use crate::damage::{DamageTracker, DirtyRect};
use crate::events::{EventBus, EventSubscription, TerminalEvent};
use crate::state::RuntimeState;
use crate::theme::WindowsTerminalTheme;
use crate::wezterm_core::{ScreenSnapshot, WezTermCore};
use crate::{Color, CursorMeta, TerminalSize};
use std::time::Instant;

/// 帧更新数据
///
/// 包含一次渲染所需的全部信息。
#[derive(Debug, Clone)]
pub struct FrameUpdate {
    /// 脏矩形列表
    pub dirty_rects: Vec<DirtyRect>,
    /// 光标元信息
    pub cursor: CursorMeta,
    /// 屏幕快照（仅包含脏区行的数据）
    pub dirty_cells: Vec<DirtyRow>,
    /// 当前 seqno
    pub seqno: u64,
}

/// 脏行数据
#[derive(Debug, Clone)]
pub struct DirtyRow {
    /// 行索引
    pub y: usize,
    /// 该行的 Cell 数据
    pub cells: Vec<crate::RustXtermCell>,
}

/// 终极结构体：所有子系统的所有者
///
/// 通过 `&mut self` 保证线程安全，无需任何锁。
/// 宿主层应在单线程事件循环中调用所有方法。
pub struct TerminalManager {
    /// 1. 状态机核心：封装 WezTerm
    core: WezTermCore,
    /// 2. 编解码闸门：处理 GBK/UTF-8 边界
    codec: CodecGate,
    /// 3. 脏区追踪器：记录逻辑变更
    damage: DamageTracker,
    /// 4. 运行时状态
    state: RuntimeState,
    /// 默认前景色
    default_fg: Color,
    /// 默认背景色
    default_bg: Color,
    /// 5. 事件总线（xterm.js 风格）
    events: EventBus,
    /// 6. Buffer 命名空间（xterm.js 风格）
    buffers: BufferNamespace,
    /// 7. Addon 列表
    addons: Vec<Box<dyn Addon>>,
    /// 8. Marker ID 计数器
    next_marker_id: u32,
    /// 上次记录的 title（用于检测变更）
    last_title: String,
}

impl TerminalManager {
    /// 创建新的终端管理器
    ///
    /// - `size`：初始终端尺寸
    /// - `codec`：编码类型
    pub fn new(size: TerminalSize, codec: Codec) -> Self {
        Self {
            core: WezTermCore::new(size, Default::default()),
            codec: CodecGate::new(codec),
            damage: DamageTracker::new(size.rows, size.cols),
            state: RuntimeState::new(),
            default_fg: Color::WHITE,
            default_bg: Color::BLACK,
            events: EventBus::new(),
            buffers: BufferNamespace::new(size),
            addons: Vec::new(),
            next_marker_id: 0,
            last_title: String::new(),
        }
    }

    /// 创建 UTF-8 模式的终端管理器
    pub fn utf8(size: TerminalSize) -> Self {
        Self::new(size, Codec::Utf8)
    }

    /// 创建 GBK 模式的终端管理器
    pub fn gbk(size: TerminalSize) -> Self {
        Self::new(size, Codec::Gbk)
    }

    // ========================================================================
    // 输入流 API
    // ========================================================================

    /// 写入原始字节流
    ///
    /// 这是 SSH/PTY 数据的入口。数据经过编码闸门转换为 UTF-8，
    /// 然后喂入 WezTerm 状态机。变更的行会被标记为脏。
    ///
    /// 调用后立即返回，不触发渲染。
    pub fn write(&mut self, raw_bytes: &[u8]) {
        // 1. 编码闸门：解码为 UTF-8
        let utf8_str = self.codec.decode(raw_bytes);

        if utf8_str.is_empty() {
            return;
        }

        // 2. 记录变更前的 seqno 和光标位置
        let before_seqno = self.core.current_seqno();
        let before_cursor = self.core.cursor_meta();

        // 3. 喂入状态机
        self.core.advance_bytes(&utf8_str);

        // 4. 标记脏区
        let changed_rows = self.core.changed_rows_since(before_seqno);
        for row in changed_rows {
            self.damage.mark_dirty(row);
        }

        // 5. 触发事件（xterm.js 风格）
        self.emit_state_events(before_cursor);
    }

    /// 检查并触发状态变更事件
    fn emit_state_events(&mut self, before_cursor: CursorMeta) {
        // Title 变更
        let current_title = self.core.title();
        if current_title != self.last_title {
            self.last_title = current_title.clone();
            self.events.emit(&TerminalEvent::TitleChange(current_title));
        }

        // 光标移动
        let current_cursor = self.core.cursor_meta();
        if current_cursor.x != before_cursor.x
            || current_cursor.y != before_cursor.y
            || current_cursor.visible != before_cursor.visible
        {
            self.events.emit(&TerminalEvent::CursorMove(current_cursor));
        }
    }

    /// 写入用户输入（TX 方向）
    ///
    /// 将用户输入编码为目标编码，返回应发送给 PTY 的字节流。
    /// 同时也喂入状态机以更新本地回显。
    pub fn write_input(&mut self, input: &str) -> Vec<u8> {
        // 编码为 PTY 编码
        let encoded = self.codec.encode(input);
        // 同时喂入状态机（本地回显）
        self.core.advance_bytes(input);
        encoded
    }

    /// 调整终端尺寸
    pub fn resize(&mut self, size: TerminalSize) {
        let old_size = self.core.size();
        self.core.resize(size);
        self.damage.resize(size.rows, size.cols);
        self.buffers.resize(size);
        if old_size != size {
            self.events.emit(&TerminalEvent::Resize(size));
        }
    }

    // ========================================================================
    // 渲染流 API
    // ========================================================================

    /// 轮询帧更新
    ///
    /// 懒检查：若无脏区且光标闪烁未到期，返回 `None`。
    /// 否则提取脏区并返回 `FrameUpdate`。
    ///
    /// - `now`：当前时间戳
    pub fn poll_frame(&mut self, now: Instant) -> Option<FrameUpdate> {
        // 懒检查（在 advance_blink 之前检查，否则 last_blink 会被更新导致永远 false）
        let has_damage = !self.damage.is_empty();
        let blink_due = self.state.blink_due(now);

        if !has_damage && !blink_due {
            return None;
        }

        // 推进光标闪烁（仅在需要渲染时才推进）
        self.state.advance_blink(now);

        // 提取脏矩形
        let dirty_rects = self.damage.drain_rects();

        // 获取屏幕快照
        let snapshot = self.core.screen_snapshot();
        let seqno = snapshot.seqno;

        // 提取脏行数据
        let mut dirty_cells = Vec::new();
        for rect in &dirty_rects {
            for y in rect.y..(rect.y + rect.height) {
                if let Some(row) = snapshot.rows.get(y) {
                    dirty_cells.push(DirtyRow {
                        y,
                        cells: row.clone(),
                    });
                }
            }
        }

        // 获取光标元信息
        let mut cursor = self.core.cursor_meta();
        // 应用闪烁
        if !self.state.cursor_visible() {
            cursor.visible = false;
        }

        // 记录渲染
        self.state.mark_rendered(now, seqno);

        Some(FrameUpdate {
            dirty_rects,
            cursor,
            dirty_cells,
            seqno,
        })
    }

    /// 强制全量渲染
    ///
    /// 标记所有行为脏，下次 `poll_frame` 将返回完整屏幕。
    pub fn invalidate(&mut self) {
        self.damage.mark_all_dirty();
    }

    // ========================================================================
    // 查询 API
    // ========================================================================

    /// 获取当前尺寸
    pub fn size(&self) -> TerminalSize {
        self.core.size()
    }

    /// 获取当前编码
    pub fn codec(&self) -> Codec {
        self.codec.codec()
    }

    /// 切换编码
    ///
    /// 切换后重置编解码器状态。
    pub fn set_codec(&mut self, codec: Codec) {
        self.codec.set_codec(codec);
    }

    /// 获取编码统计
    pub fn codec_stats(&self) -> crate::codec_gate::CodecStats {
        self.codec.stats()
    }

    /// 获取光标元信息
    pub fn cursor(&self) -> CursorMeta {
        self.core.cursor_meta()
    }

    /// 获取完整屏幕快照
    pub fn screen_snapshot(&self) -> ScreenSnapshot {
        self.core.screen_snapshot()
    }

    /// 获取带滚动偏移的屏幕快照（`0` = 实时可视窗口，`>0` = 回溯历史）
    pub fn snapshot_scrolled(&self, scroll_offset: usize) -> ScreenSnapshot {
        self.core.snapshot_scrolled(scroll_offset)
    }

    /// 可向上回溯的最大行数
    pub fn max_scrollback(&self) -> usize {
        self.core.max_scrollback()
    }

    /// 取出终端产生的输出字节（鼠标报告、CSI/OSC 响应等）
    ///
    /// 宿主层应在每次 tick 后调用，将响应转发给 PTY，以使鼠标模式、
    /// 光标位置查询（CSI 6n）、颜色查询等终端响应正确工作。
    pub fn drain_output(&self) -> Vec<u8> {
        self.core.drain_output()
    }

    /// 是否有任何鼠标跟踪模式被启用
    pub fn is_mouse_grabbed(&self) -> bool {
        self.core.is_mouse_grabbed()
    }

    /// 提交一个鼠标事件
    ///
    /// 坐标 `x`/`y` 为当前可视窗口的 0-based 列/行。
    /// 若应用启用了鼠标跟踪，WezTerm 会自动编码报告并写入捕获缓冲，
    /// 下次 [`Self::drain_output`] 即可取出转发给 PTY。
    pub fn mouse_event(
        &mut self,
        x: usize,
        y: usize,
        action: crate::mouse::MouseAction,
        button: crate::mouse::MouseButton,
        mods: crate::mouse::KeyMods,
    ) {
        self.core.mouse_event(x, y, action, button, mods);
    }

    /// 获取默认前景色
    pub fn default_fg(&self) -> Color {
        self.default_fg
    }

    /// 获取默认背景色
    pub fn default_bg(&self) -> Color {
        self.default_bg
    }

    /// 设置默认前景色
    pub fn set_default_fg(&mut self, color: Color) {
        self.default_fg = color;
        self.invalidate();
    }

    /// 设置默认背景色
    pub fn set_default_bg(&mut self, color: Color) {
        self.default_bg = color;
        self.invalidate();
    }

    /// 设置光标闪烁
    pub fn set_cursor_blinking(&mut self, enabled: bool) {
        self.state.set_cursor_blinking(enabled);
    }

    /// 获取对 WezTermCore 的不可变引用（高级 API）
    pub fn core(&self) -> &WezTermCore {
        &self.core
    }

    /// 获取对 WezTermCore 的可变引用（高级 API）
    pub fn core_mut(&mut self) -> &mut WezTermCore {
        &mut self.core
    }

    /// 获取对 CodecGate 的可变引用（高级 API）
    pub fn codec_mut(&mut self) -> &mut CodecGate {
        &mut self.codec
    }

    /// 获取对 DamageTracker 的不可变引用（高级 API）
    pub fn damage(&self) -> &DamageTracker {
        &self.damage
    }

    // ========================================================================
    // xterm.js 风格 API
    // ========================================================================

    /// 注册事件回调（类似 xterm.js 的 `terminal.onData(cb)`）
    ///
    /// 返回 `EventSubscription`，drop 时自动取消订阅。
    pub fn on<F>(&self, callback: F) -> EventSubscription
    where
        F: Fn(&TerminalEvent) + Send + Sync + 'static,
    {
        self.events.on(callback)
    }

    /// 获取事件总线引用
    pub fn events(&self) -> &EventBus {
        &self.events
    }

    /// 获取窗口标题（OSC 0/2）
    pub fn title(&self) -> String {
        self.core.title()
    }

    /// 获取图标名称（OSC 1）
    pub fn icon_name(&self) -> String {
        self.core.icon_name()
    }

    /// 是否处于备用屏幕
    pub fn is_alt_screen_active(&self) -> bool {
        self.core.is_alt_screen_active()
    }

    /// 创建 Marker（类似 xterm.js 的 `buffer.addMarker(line)`）
    pub fn add_marker(&mut self, line: i32) -> Marker {
        let marker = Marker::new(self.next_marker_id, line);
        self.next_marker_id += 1;
        self.buffers.add_marker(line);
        marker
    }

    /// 获取所有 Marker
    pub fn markers(&self) -> &[Marker] {
        self.buffers.markers()
    }

    /// 移除 Marker
    pub fn remove_marker(&mut self, id: u32) -> bool {
        self.buffers.remove_marker(id)
    }

    /// 获取当前 Buffer 视图（类似 xterm.js 的 `terminal.buffer`）
    pub fn buffer(&self) -> Buffer {
        let cursor = self.core.cursor_meta();
        let snapshot = self.core.screen_snapshot();
        let kind = if self.is_alt_screen_active() {
            BufferType::Alternate
        } else {
            BufferType::Normal
        };
        Buffer {
            kind,
            cursor_y: cursor.y,
            cursor_x: cursor.x,
            base_y: 0,
            height: snapshot.size.rows,
            width: snapshot.size.cols,
            lines: snapshot.rows,
        }
    }

    /// 加载 Addon（类似 xterm.js 的 `terminal.loadAddon(addon)`）
    pub fn load_addon<A: Addon + 'static>(&mut self, mut addon: A) {
        let mut ctx = AddonContext { manager: self };
        addon.activate(&mut ctx);
        self.addons.push(Box::new(addon));
    }

    /// 应用 Windows Terminal 主题
    pub fn apply_theme(&mut self, theme: &WindowsTerminalTheme) {
        let fg = theme.foreground;
        let bg = theme.background;
        self.default_fg = Color::rgba(
            (fg.0 * 255.0) as u8,
            (fg.1 * 255.0) as u8,
            (fg.2 * 255.0) as u8,
            (fg.3 * 255.0) as u8,
        );
        self.default_bg = Color::rgba(
            (bg.0 * 255.0) as u8,
            (bg.1 * 255.0) as u8,
            (bg.2 * 255.0) as u8,
            (bg.3 * 255.0) as u8,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_write_and_poll() {
        let mut mgr = TerminalManager::utf8(TerminalSize::new(24, 80));

        // 写入一些文本
        mgr.write(b"Hello, rust-xterm!");

        // 应该有脏区
        assert!(!mgr.damage().is_empty());

        // 轮询帧
        let frame = mgr.poll_frame(Instant::now());
        assert!(frame.is_some());

        let frame = frame.unwrap();
        assert!(!frame.dirty_rects.is_empty());
        assert!(!frame.dirty_cells.is_empty());
    }

    #[test]
    fn test_no_damage_returns_none() {
        let mut mgr = TerminalManager::utf8(TerminalSize::new(24, 80));

        // 没有写入任何数据，也没有脏区
        let frame = mgr.poll_frame(Instant::now());
        assert!(frame.is_none());
    }

    #[test]
    fn test_resize_invalidates() {
        let mut mgr = TerminalManager::utf8(TerminalSize::new(24, 80));
        mgr.write(b"test");
        let _ = mgr.poll_frame(Instant::now());

        // resize 后应该有脏区
        mgr.resize(TerminalSize::new(30, 100));
        assert!(!mgr.damage().is_empty());
    }

    #[test]
    fn test_gbk_mode() {
        let mut mgr = TerminalManager::gbk(TerminalSize::new(24, 80));

        // "你好" 的 GBK 编码
        let gbk_bytes = [0xC4, 0xE3, 0xBA, 0xC3];
        mgr.write(&gbk_bytes);

        let frame = mgr.poll_frame(Instant::now());
        assert!(frame.is_some());

        // 验证屏幕上包含中文
        let snapshot = mgr.screen_snapshot();
        let full_text: String = snapshot
            .rows
            .iter()
            .flat_map(|row| row.iter().map(|c| c.text.as_str()))
            .collect();
        assert!(full_text.contains("你好"));
    }
}
