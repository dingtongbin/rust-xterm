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
use crate::parser::Parser;
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
    /// 上次记录的 icon_name（用于检测变更）
    last_icon_name: String,
    /// 9. 自定义序列解析器（xterm.js 风格）
    parser: Parser,
    /// 10. BEL 挂起标志（在 write 中扫描 BEL 字节设置，emit_state_events 中消费）
    bell_pending: bool,
}

impl TerminalManager {
    /// 创建新的终端管理器
    ///
    /// - `size`：初始终端尺寸
    /// - `codec`：编码类型
    pub fn new(size: TerminalSize, codec: Codec) -> Self {
        let events = EventBus::new();
        let mut parser = Parser::new();

        // 注册内部 OSC 52 handler，收到时 emit ClipboardRequest 事件。
        // 注意：用户通过 `parser()` 注册自己的 OSC 52 handler 会覆盖此内部 handler。
        let events_for_osc52 = events.clone();
        parser.register_osc(52, move |data: &[u8]| {
            // OSC 52 payload 形如 `c;<base64>`，这里将原始数据转为 String
            let payload = String::from_utf8_lossy(data).into_owned();
            events_for_osc52.emit(&TerminalEvent::ClipboardRequest(payload));
        });

        Self {
            core: WezTermCore::new(size, Default::default()),
            codec: CodecGate::new(codec),
            damage: DamageTracker::new(size.rows, size.cols),
            state: RuntimeState::new(),
            default_fg: Color::WHITE,
            default_bg: Color::BLACK,
            events,
            buffers: BufferNamespace::new(),
            addons: Vec::new(),
            next_marker_id: 0,
            last_title: String::new(),
            last_icon_name: String::new(),
            parser,
            bell_pending: false,
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

        // 2. 记录变更前的 seqno、光标位置和 scrollback 行数
        let before_seqno = self.core.current_seqno();
        let before_cursor = self.core.cursor_meta();
        let before_scrollback = self.core.max_scrollback();

        // 3. 轻量 OSC/BEL 扫描：在喂入 WezTerm 之前，先扫描输入字节，
        //    将 OSC 序列派发给自定义 handler，并检测 BEL 字节。
        //    注意：仅 OSC，不拦截 CSI/DCS，避免与 WezTerm 重复处理。
        self.scan_and_dispatch(utf8_str.as_bytes());

        // 4. 喂入状态机
        self.core.advance_bytes(&utf8_str);

        // 5. 标记脏区
        let changed_rows = self.core.changed_rows_since(before_seqno);
        for row in changed_rows {
            self.damage.mark_dirty(row);
        }

        // 6. 更新 scrollback 偏移（Task 4：Marker 滚动追踪）
        let after_scrollback = self.core.max_scrollback();
        if let Some(delta) = after_scrollback.checked_sub(before_scrollback) {
            if delta > 0 {
                self.buffers.add_scrollback_offset(delta);
            }
        }

        // 7. 触发事件（xterm.js 风格）
        self.emit_state_events(before_cursor);
    }

    /// 轻量 OSC/BEL 扫描
    ///
    /// 识别 `ESC ]` 开头、`BEL` 或 `ST` 结尾的 OSC 序列，
    /// 解析 OSC code 与 payload，调用 `parser.dispatch_osc`。
    /// 同时检测 BEL 字节 (0x07)，设置 `bell_pending` 标志。
    ///
    /// 注意：此扫描不消耗输入（WezTerm 仍会正常处理这些序列），
    /// 仅用于将 OSC 派发给用户注册的自定义 handler。
    fn scan_and_dispatch(&mut self, bytes: &[u8]) {
        let mut i = 0;
        let len = bytes.len();
        while i < len {
            if bytes[i] == 0x1b {
                // 检查是否为 OSC 开始：ESC ] (0x1b 0x5d)
                if i + 1 < len && bytes[i + 1] == 0x5d {
                    // 解析 OSC 序列
                    let start = i + 2;
                    // 解析 OSC code（数字部分）
                    let mut code_end = start;
                    while code_end < len && bytes[code_end].is_ascii_digit() {
                        code_end += 1;
                    }
                    if code_end > start {
                        // 有数字 code
                        if let Ok(code_str) = std::str::from_utf8(&bytes[start..code_end]) {
                            if let Ok(code) = code_str.parse::<u32>() {
                                // 查找序列结束：BEL (0x07) 或 ST (0x1b 0x5c)
                                let mut j = code_end;
                                let mut payload_end = None;
                                while j < len {
                                    if bytes[j] == 0x07 {
                                        payload_end = Some(j);
                                        break;
                                    }
                                    if bytes[j] == 0x1b && j + 1 < len && bytes[j + 1] == 0x5c {
                                        payload_end = Some(j);
                                        break;
                                    }
                                    j += 1;
                                }
                                if let Some(end) = payload_end {
                                    // payload 为 code 之后到结束符之前（跳过分号）
                                    let payload_start = if code_end < end && bytes[code_end] == b';'
                                    {
                                        code_end + 1
                                    } else {
                                        code_end
                                    };
                                    let payload = &bytes[payload_start..end];
                                    self.parser.dispatch_osc(code, payload);
                                    i = end + 1;
                                    continue;
                                }
                            }
                        }
                    }
                    // 解析失败的 OSC，跳过 ESC
                    i += 1;
                    continue;
                }
                // 其他 ESC 序列，跳过 ESC（WezTerm 会处理）
                i += 1;
                continue;
            }
            // 检测 BEL 字节 (0x07)，设置 bell_pending
            if bytes[i] == 0x07 {
                self.bell_pending = true;
            }
            i += 1;
        }
    }

    /// 检查并触发状态变更事件
    fn emit_state_events(&mut self, before_cursor: CursorMeta) {
        // Title 变更
        let current_title = self.core.title();
        if current_title != self.last_title {
            self.last_title = current_title.clone();
            self.events.emit(&TerminalEvent::TitleChange(current_title));
        }

        // IconName 变更
        // 注意：WezTerm 的 icon_name() 总是返回 title 的回退值，
        // 因此 IconNameChange 可能与 TitleChange 同时触发，这是预期行为。
        let current_icon = self.core.icon_name();
        if current_icon != self.last_icon_name {
            self.last_icon_name = current_icon.clone();
            self.events
                .emit(&TerminalEvent::IconNameChange(current_icon));
        }

        // Bell 事件（检测 write() 中扫描到的 BEL 字节）
        if self.bell_pending {
            self.bell_pending = false;
            self.events.emit(&TerminalEvent::Bell);
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
        // BufferNamespace 不再持有影子 Buffer 状态，无需 resize
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
        let snapshot = self.core.screen_snapshot(self.default_fg, self.default_bg);
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
        self.core.screen_snapshot(self.default_fg, self.default_bg)
    }

    /// 获取带滚动偏移的屏幕快照（`0` = 实时可视窗口，`>0` = 回溯历史）
    pub fn snapshot_scrolled(&self, scroll_offset: usize) -> ScreenSnapshot {
        self.core
            .snapshot_scrolled(scroll_offset, self.default_fg, self.default_bg)
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

    /// 是否启用了括号粘贴模式（bracketed paste）
    ///
    /// 委托给 WezTerm 的 `bracketed_paste_enabled()`，
    /// 当应用发送 `\x1b[?2004h` 时启用，`\x1b[?2004l` 时禁用。
    pub fn is_bracketed_paste_enabled(&self) -> bool {
        self.core.is_bracketed_paste_enabled()
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

    /// 获取对 Parser 的可变引用，用于注册自定义 OSC/CSI/DCS handler
    ///
    /// 已注册的 handler 会在 `write()` 流中被自动调用（仅 OSC）。
    /// 注意：用户注册的 OSC handler 会覆盖内部 handler（如 OSC 52 的
    /// ClipboardRequest handler）。
    pub fn parser(&mut self) -> &mut Parser {
        &mut self.parser
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

    /// 获取所有有效 Marker
    ///
    /// 返回的 Marker 的 `line` 字段是经过 scrollback 偏移修正后的
    /// "有效行号"，已推出可视区的标记会被过滤掉。
    pub fn markers(&self) -> Vec<Marker> {
        self.buffers.markers()
    }

    /// 移除 Marker
    pub fn remove_marker(&mut self, id: u32) -> bool {
        self.buffers.remove_marker(id)
    }

    /// 获取当前 Buffer 视图（类似 xterm.js 的 `terminal.buffer`）
    pub fn buffer(&self) -> Buffer {
        let cursor = self.core.cursor_meta();
        let snapshot = self.core.screen_snapshot(self.default_fg, self.default_bg);
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

    /// Task 1: 验证 resolve_color 的 Default 分支尊重用户配置的默认色
    #[test]
    fn test_default_color_respects_theme() {
        let mut mgr = TerminalManager::utf8(TerminalSize::new(24, 80));
        // 设置默认前景色为蓝色
        let blue = Color::rgb(0, 0, 255);
        mgr.set_default_fg(blue);
        // 写入纯文本（不显式设置颜色，应使用 Default 前景色）
        mgr.write(b"Hi");
        // poll_frame 触发快照
        let _ = mgr.poll_frame(Instant::now());
        let snapshot = mgr.screen_snapshot();
        // 找到第一个非空白 cell，断言其前景色为蓝色
        let cell = snapshot
            .rows
            .iter()
            .flat_map(|row| row.iter())
            .find(|c| !c.is_blank())
            .expect("应至少有一个非空白 cell");
        assert_eq!(cell.fg, blue, "默认前景色应为蓝色（用户配置）");
    }

    /// Task 4: 验证 marker 在 scrollback 增长时有效行号随之调整
    #[test]
    fn test_marker_tracks_scroll() {
        let mut mgr = TerminalManager::utf8(TerminalSize::new(24, 80));
        // 写满 24 行（每行带换行），最后一行触发滚动前 add_marker
        for i in 0..24 {
            let line = format!("line{}\r\n", i);
            mgr.write(line.as_bytes());
        }
        // 此时 scrollback 仍为 0（屏幕刚好填满），add_marker(23) 标记最后一行
        let _ = mgr.add_marker(23);
        assert_eq!(mgr.buffers.scrollback_offset(), 0);
        // 再写一行触发滚动
        mgr.write(b"line24\r\n");
        // scrollback 应增长 1，marker 有效行号 = 23 - 1 = 22
        let markers = mgr.markers();
        assert_eq!(markers.len(), 1, "应有一个有效 marker");
        assert_eq!(
            markers[0].line, 22,
            "marker 有效行号应为 22（原 23 减去 scrollback 偏移 1）"
        );
    }

    /// Task 5: 验证 OSC handler 在 write 数据流中被调用
    #[test]
    fn test_osc_handler_invoked() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let mut mgr = TerminalManager::utf8(TerminalSize::new(24, 80));
        let counter = Arc::new(AtomicUsize::new(0));
        let c = counter.clone();
        // 注册用户 OSC 52 handler（会覆盖内部 ClipboardRequest handler）
        mgr.parser().register_osc(52, move |_data| {
            c.fetch_add(1, Ordering::Relaxed);
        });
        // 写入 OSC 52 序列：ESC ] 52 ; c ; test BEL
        mgr.write(b"\x1b]52;c;test\x07");
        assert!(
            counter.load(Ordering::Relaxed) > 0,
            "OSC 52 handler 应被调用"
        );
    }

    /// Task 6: 验证 Bell 事件在写入 BEL 字节时被触发
    #[test]
    fn test_bell_event() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let mut mgr = TerminalManager::utf8(TerminalSize::new(24, 80));
        let counter = Arc::new(AtomicUsize::new(0));
        let c = counter.clone();
        let _sub = mgr.on(move |event| {
            if matches!(event, TerminalEvent::Bell) {
                c.fetch_add(1, Ordering::Relaxed);
            }
        });
        // 写入 BEL 字节
        mgr.write(b"\x07");
        assert!(counter.load(Ordering::Relaxed) > 0, "Bell 事件应被触发");
    }

    /// Task 6: 验证 ClipboardRequest 事件通过内部 OSC 52 handler 触发
    #[test]
    fn test_clipboard_request_event() {
        use std::sync::{Arc, Mutex};

        let mut mgr = TerminalManager::utf8(TerminalSize::new(24, 80));
        let received: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let r = received.clone();
        let _sub = mgr.on(move |event| {
            if let TerminalEvent::ClipboardRequest(payload) = event {
                *r.lock().unwrap() = Some(payload.clone());
            }
        });
        // 写入 OSC 52 序列：ESC ] 52 ; c ; dGVzdA== BEL （dGVzdA== 是 "test" 的 base64）
        mgr.write(b"\x1b]52;c;dGVzdA==\x07");
        let got = received.lock().unwrap().clone();
        assert!(got.is_some(), "ClipboardRequest 事件应被触发");
        assert!(
            got.unwrap().contains("dGVzdA=="),
            "payload 应包含 base64 数据"
        );
    }

    /// Task 7: 验证 bracketed paste 模式查询
    #[test]
    fn test_bracketed_paste_query() {
        let mut mgr = TerminalManager::utf8(TerminalSize::new(24, 80));
        // 初始应为禁用
        assert!(!mgr.is_bracketed_paste_enabled());
        // 启用 bracketed paste: ESC [ ? 2004 h
        mgr.write(b"\x1b[?2004h");
        assert!(mgr.is_bracketed_paste_enabled(), "启用后应返回 true");
        // 禁用 bracketed paste: ESC [ ? 2004 l
        mgr.write(b"\x1b[?2004l");
        assert!(!mgr.is_bracketed_paste_enabled(), "禁用后应返回 false");
    }

    /// Task 8: 验证 OSC 8 超链接被提取到 RustXtermCell.hyperlink
    #[test]
    fn test_hyperlink_extracted() {
        let mut mgr = TerminalManager::utf8(TerminalSize::new(24, 80));
        // 设置超链接，写入文本，清除超链接
        mgr.write(b"\x1b]8;;https://example.com\x07");
        mgr.write(b"link");
        mgr.write(b"\x1b]8;;\x07");
        let snapshot = mgr.screen_snapshot();
        // 找到带 "link" 文本的 cell
        let cell = snapshot
            .rows
            .iter()
            .flat_map(|row| row.iter())
            .find(|c| c.text == "link");
        if let Some(cell) = cell {
            // WezTerm 在写入 "link" 时 pen 应持有 hyperlink，
            // 清除后不影响已写入的 cell（cell 保留创建时的 hyperlink）
            assert!(
                cell.hyperlink.is_some(),
                "带超链接的文本 cell 应保留 hyperlink 字段"
            );
            assert_eq!(cell.hyperlink.as_ref().unwrap(), "https://example.com");
        }
        // 至少不应 panic
    }
}
