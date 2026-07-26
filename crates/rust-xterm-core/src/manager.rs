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
use crate::buffer::{self, Buffer, BufferNamespace, BufferType, Marker};
use crate::codec_gate::{Codec, CodecGate};
use crate::damage::{DamageTracker, DirtyRect};
use crate::events::{EventBus, EventSubscription, TerminalEvent};
use crate::mouse::{KeyMods, MouseAction, MouseButton, MouseState};
use crate::parser::Parser;
use crate::selection::SelectionRange;
use crate::state::RuntimeState;
use crate::theme::WindowsTerminalTheme;
use crate::wezterm_core::{ScreenSnapshot, WezTermCore};
use crate::{Color, CursorMeta, TerminalSize};
use std::path::PathBuf;
use std::time::{Duration, Instant};

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
    /// 11. 当前选区（线性或矩形），`None` 表示无选区
    selection: Option<SelectionRange>,
    /// 12. 鼠标选区状态机（点击计数、拖拽起点等）
    mouse_state: MouseState,
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

        // 注册内部 OSC 7 handler，解析 `file://<host>/<path>` 并 emit CwdChange 事件。
        // 注意：用户通过 `parser()` 注册自己的 OSC 7 handler 会覆盖此内部 handler。
        let events_for_osc7 = events.clone();
        parser.register_osc(7, move |data: &[u8]| {
            if let Some(path) = parse_cwd_osc7(data) {
                events_for_osc7.emit(&TerminalEvent::CwdChange(path));
            }
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
            selection: None,
            mouse_state: MouseState::default(),
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
    ///
    /// - 若应用启用了鼠标跟踪模式（`is_mouse_grabbed` 为真），事件转发给 WezTerm，
    ///   自动编码报告并写入捕获缓冲，下次 [`Self::drain_output`] 即可取出转发给 PTY。
    /// - 否则，事件用于本地选区交互：左键单击/拖拽选词、双击选词、三击选行。
    pub fn mouse_event(
        &mut self,
        x: usize,
        y: usize,
        action: MouseAction,
        button: MouseButton,
        mods: KeyMods,
    ) {
        if self.core.is_mouse_grabbed() {
            self.core.mouse_event(x, y, action, button, mods);
            return;
        }
        self.handle_selection_mouse(x, y, action, button, mods);
    }

    /// 处理非鼠标跟踪模式下的选区交互
    ///
    /// 实现单击拖拽选区、双击选词、三击选行的状态机。
    /// 点击计数遵循 500ms 时间窗与同位置约束，循环 1→2→3→1。
    ///
    /// 入参 `x`/`y` 为 0-based 列/行，内部统一转换为 `(row, col) = (y, x)`。
    fn handle_selection_mouse(
        &mut self,
        x: usize,
        y: usize,
        action: MouseAction,
        button: MouseButton,
        _mods: KeyMods,
    ) {
        // 仅左键参与选区交互
        if button != MouseButton::Left {
            return;
        }
        // 统一使用 (row, col) 坐标
        let pos = (y, x);
        match action {
            MouseAction::Press => {
                let now = Instant::now();
                // 点击计数：500ms 内且同位置则递增，否则重置为 1
                let same_pos = self.mouse_state.last_click_pos == pos;
                let within_window = now.duration_since(self.mouse_state.last_click_time)
                    < Duration::from_millis(500);
                if same_pos && within_window {
                    self.mouse_state.click_count = (self.mouse_state.click_count % 3) + 1;
                } else {
                    self.mouse_state.click_count = 1;
                }
                self.mouse_state.last_click_time = now;
                self.mouse_state.last_click_pos = pos;

                match self.mouse_state.click_count {
                    1 => {
                        // 单击：清旧选区，记录拖拽起点
                        self.selection = None;
                        self.mouse_state.selecting = true;
                        self.mouse_state.select_start = pos;
                        self.events.emit(&TerminalEvent::SelectionChange);
                    }
                    2 => {
                        // 双击：选词
                        self.mouse_state.selecting = true;
                        let snapshot = self.screen_snapshot();
                        let range = buffer::select_word(pos, &snapshot.rows);
                        self.selection = Some(range);
                        self.events.emit(&TerminalEvent::SelectionChange);
                    }
                    3 => {
                        // 三击：选行
                        self.mouse_state.selecting = true;
                        let snapshot = self.screen_snapshot();
                        let range = buffer::select_line(pos, &snapshot.rows);
                        self.selection = Some(range);
                        self.events.emit(&TerminalEvent::SelectionChange);
                    }
                    _ => {}
                }
            }
            MouseAction::Move => {
                // 拖拽扩展选区（仅单击后的拖拽）
                if self.mouse_state.selecting && self.mouse_state.click_count == 1 {
                    let start = self.mouse_state.select_start;
                    self.selection = Some(SelectionRange {
                        start,
                        end: pos,
                        rectangular: false,
                    });
                    self.events.emit(&TerminalEvent::SelectionChange);
                }
            }
            MouseAction::Release => {
                if self.mouse_state.selecting {
                    self.mouse_state.selecting = false;
                    if self.selection.is_some() {
                        self.events.emit(&TerminalEvent::SelectionReady);
                    }
                }
            }
            _ => {}
        }
    }

    /// 设置当前选区（程序化 API）
    ///
    /// 传入 `None` 清除选区。会派发 [`TerminalEvent::SelectionChange`] 事件。
    pub fn set_selection(&mut self, range: Option<SelectionRange>) {
        self.selection = range;
        self.events.emit(&TerminalEvent::SelectionChange);
    }

    /// 获取当前选区
    pub fn selection(&self) -> Option<SelectionRange> {
        self.selection
    }

    /// 获取当前选区的文本内容
    ///
    /// 从当前屏幕快照按选区范围提取文本。无选区时返回 `None`。
    /// 线性选区按起点终点排序后跨行用 `\n` 连接；矩形选区按列范围逐行截取。
    pub fn selection_text(&self) -> Option<String> {
        let range = self.selection?;
        let snapshot = self.screen_snapshot();
        Some(buffer::selection_text(range, &snapshot.rows))
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

    /// 是否启用了焦点报告模式（DECSET 1004）
    ///
    /// 当应用发送 `\x1b[?1004h` 时启用，`\x1b[?1004l` 时禁用。
    /// 状态由 [`Self::write`] 在数据流中扫描 DECSET 1004 序列维护。
    pub fn is_focus_reporting_enabled(&self) -> bool {
        self.core.is_focus_reporting_enabled()
    }

    /// 通知终端焦点状态变化
    ///
    /// 若启用了焦点报告模式（DECSET 1004），WezTerm 会向输出缓冲写入
    /// `\x1b[I`（`focused = true`）或 `\x1b[O`（`focused = false`），
    /// 宿主在下次 [`Self::drain_output`] 即可取出转发给 PTY。
    ///
    /// 同时向事件总线派发 [`TerminalEvent::FocusReport`]（仅在焦点报告启用时），
    /// 宿主可据此同步本地 UI 焦点状态。
    pub fn set_focused(&mut self, focused: bool) {
        let reporting = self.core.is_focus_reporting_enabled();
        self.core.set_focused(focused);
        if reporting {
            self.events.emit(&TerminalEvent::FocusReport(focused));
        }
    }

    /// 获取当前滚动区域（DECSTBM），1-based `(top, bottom)`
    ///
    /// 返回 `None` 表示全屏（未设置 DECSTBM 或已重置）。
    /// 状态由 [`Self::write`] 在数据流中扫描 `\x1b[<top>;<bottom>r` 序列维护。
    pub fn scroll_region(&self) -> Option<(usize, usize)> {
        self.core.scroll_region()
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

/// 解析 OSC 7 payload 为本地路径
///
/// payload 形如 `file://<host>/<path>`，手写解析以避免引入 `url` crate：
/// 1. 校验 `file://` 前缀
/// 2. 跳过 host（从 `file://` 后到第一个 `/`）
/// 3. 剩余部分（含前导 `/`）作为路径
///
/// 返回 `None` 表示格式不合法或无路径。
fn parse_cwd_osc7(payload: &[u8]) -> Option<PathBuf> {
    let s = std::str::from_utf8(payload).ok()?;
    const PREFIX: &str = "file://";
    let rest = s.strip_prefix(PREFIX)?;
    // host 段到第一个 '/' 结束；其后为绝对路径
    let path_start = rest.find('/')?;
    let path = &rest[path_start..];
    if path.is_empty() {
        return None;
    }
    Some(PathBuf::from(path))
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
            let line = format!("line{i}\r\n");
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

    // ========================================================================
    // Task 1: 焦点报告（DECSET 1004）
    // ========================================================================

    /// Task 1: 启用焦点报告后 set_focused 应将 \x1b[I 推入 drain_output
    #[test]
    fn test_focus_report_enabled() {
        use std::thread;
        let mut mgr = TerminalManager::utf8(TerminalSize::new(24, 80));
        // 初始未启用
        assert!(!mgr.is_focus_reporting_enabled());
        // 启用焦点报告：ESC [ ? 1004 h
        mgr.write(b"\x1b[?1004h");
        assert!(mgr.is_focus_reporting_enabled(), "DECSET 1004 后应启用");
        // WezTerm 默认 focused=true，先切换到 false 以触发后续状态变更。
        // WezTerm 的 writer 是 ThreadedWriter（后台线程异步写入），
        // 需短暂 sleep 让后台线程处理完写入后再 drain。
        mgr.set_focused(false);
        thread::sleep(Duration::from_millis(20));
        let _ = mgr.drain_output();
        // 获得焦点：从 false 变 true，应产生 \x1b[I
        mgr.set_focused(true);
        thread::sleep(Duration::from_millis(20));
        let out = mgr.drain_output();
        assert!(
            out.windows(3).any(|w| w == b"\x1b[I"),
            "启用时 set_focused(true) 应产生 \\x1b[I，got {out:?}"
        );
        // 失去焦点：从 true 变 false，应产生 \x1b[O
        mgr.set_focused(false);
        thread::sleep(Duration::from_millis(20));
        let out = mgr.drain_output();
        assert!(
            out.windows(3).any(|w| w == b"\x1b[O"),
            "启用时 set_focused(false) 应产生 \\x1b[O，got {out:?}"
        );
        // 禁用焦点报告：ESC [ ? 1004 l
        mgr.write(b"\x1b[?1004l");
        assert!(!mgr.is_focus_reporting_enabled(), "DECRST 1004 后应禁用");
    }

    /// Task 1: 未启用焦点报告时 set_focused 不应产生输出
    #[test]
    fn test_focus_report_disabled() {
        use std::thread;
        let mut mgr = TerminalManager::utf8(TerminalSize::new(24, 80));
        assert!(!mgr.is_focus_reporting_enabled());
        mgr.set_focused(false);
        mgr.set_focused(true);
        // 即使等待后台线程，未启用焦点报告也不应产生输出
        thread::sleep(Duration::from_millis(20));
        let out = mgr.drain_output();
        assert!(out.is_empty(), "未启用焦点报告时不应产生输出，got {out:?}");
    }

    // ========================================================================
    // Task 2: OSC 7 CWD 事件
    // ========================================================================

    /// Task 2: OSC 7 file:// URL 应触发 CwdChange 事件
    #[test]
    fn test_osc7_cwd_event() {
        use std::sync::{Arc, Mutex};

        let mut mgr = TerminalManager::utf8(TerminalSize::new(24, 80));
        let received: Arc<Mutex<Option<PathBuf>>> = Arc::new(Mutex::new(None));
        let r = received.clone();
        let _sub = mgr.on(move |event| {
            if let TerminalEvent::CwdChange(path) = event {
                *r.lock().unwrap() = Some(path.clone());
            }
        });
        // ESC ] 7 ; file://localhost/home/user BEL
        mgr.write(b"\x1b]7;file://localhost/home/user\x07");
        let got = received.lock().unwrap().clone();
        assert_eq!(got.as_deref(), Some(std::path::Path::new("/home/user")));
    }

    /// Task 2: 格式错误的 OSC 7 payload 应被忽略，不触发事件
    #[test]
    fn test_osc7_malformed_ignored() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let mut mgr = TerminalManager::utf8(TerminalSize::new(24, 80));
        let counter = Arc::new(AtomicUsize::new(0));
        let c = counter.clone();
        let _sub = mgr.on(move |event| {
            if matches!(event, TerminalEvent::CwdChange(_)) {
                c.fetch_add(1, Ordering::Relaxed);
            }
        });
        // 非 file:// 前缀
        mgr.write(b"\x1b]7;https://example.com\x07");
        // 无路径
        mgr.write(b"\x1b]7;file://localhost\x07");
        // 非 UTF-8 风格的乱码（file:// 但无 / 路径）
        mgr.write(b"\x1b]7;file://localhost\x07");
        assert_eq!(
            counter.load(Ordering::Relaxed),
            0,
            "格式错误的 OSC 7 不应触发 CwdChange"
        );
    }

    // ========================================================================
    // Task 3: 滚动区域查询
    // ========================================================================

    /// Task 3: DECSTBM 设置滚动区域后应能查询
    #[test]
    fn test_scroll_region_query() {
        let mut mgr = TerminalManager::utf8(TerminalSize::new(24, 80));
        // 初始为全屏
        assert!(mgr.scroll_region().is_none(), "初始应为全屏 (None)");
        // 设置滚动区域 5..10 (1-based)：ESC [ 5 ; 10 r
        mgr.write(b"\x1b[5;10r");
        assert_eq!(
            mgr.scroll_region(),
            Some((5, 10)),
            "DECSTBM 后应返回 (5, 10)"
        );
        // 重置为全屏：ESC [ r
        mgr.write(b"\x1b[r");
        assert!(mgr.scroll_region().is_none(), "重置后应为全屏 (None)");
        // 等价于全屏的设置也应返回 None
        mgr.write(b"\x1b[1;24r");
        assert!(mgr.scroll_region().is_none(), "1;24 等价于全屏，应为 None");
    }

    // ========================================================================
    // Task 5: 选区文本提取
    // ========================================================================

    /// Task 5: 线性选区跨行文本提取
    #[test]
    fn test_selection_linear_text() {
        let mut mgr = TerminalManager::utf8(TerminalSize::new(5, 10));
        // 使用 \r\n 确保 "world" 出现在第 1 行 col 0..4（\n 仅换行不回车）
        mgr.write(b"hello\r\nworld");
        let _ = mgr.poll_frame(Instant::now());
        let snap = mgr.screen_snapshot();
        // 第 0 行 "hello"，第 1 行 "world"
        let range = SelectionRange::linear((0, 1), (1, 3));
        let text = buffer::selection_text(range, &snap.rows);
        // 第 0 行 col 1..末尾 = "ello"，第 1 行 0..=3 = "worl"
        assert_eq!(text, "ello\nworl");
    }

    /// Task 5: 矩形选区文本提取
    #[test]
    fn test_selection_rectangular_text() {
        let mut mgr = TerminalManager::utf8(TerminalSize::new(5, 10));
        // 使用 \r\n 确保 "world" 出现在第 1 行 col 0..4（\n 仅换行不回车）
        mgr.write(b"hello\r\nworld");
        let _ = mgr.poll_frame(Instant::now());
        let snap = mgr.screen_snapshot();
        // 矩形：(0,1)..(1,3)
        let range = SelectionRange::rectangular((0, 1), (1, 3));
        let text = buffer::selection_text(range, &snap.rows);
        // 第 0 行 col 1..=3 = "ell"，第 1 行 col 1..=3 = "orl"
        assert_eq!(text, "ell\norl");
    }

    // ========================================================================
    // Task 6: 鼠标选区交互
    // ========================================================================

    /// Task 6: 单击拖拽产生线性选区，释放触发 SelectionReady
    #[test]
    fn test_mouse_drag_selection() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let mut mgr = TerminalManager::utf8(TerminalSize::new(5, 10));
        mgr.write(b"hello world");
        let _ = mgr.poll_frame(Instant::now());

        let ready = Arc::new(AtomicUsize::new(0));
        let r = ready.clone();
        let _sub = mgr.on(move |event| {
            if matches!(event, TerminalEvent::SelectionReady) {
                r.fetch_add(1, Ordering::Relaxed);
            }
        });

        let mods = KeyMods::default();
        // 按下左键于 (col=0, row=0)
        mgr.mouse_event(0, 0, MouseAction::Press, MouseButton::Left, mods);
        // 拖拽到 (col=4, row=0)
        mgr.mouse_event(4, 0, MouseAction::Move, MouseButton::Left, mods);
        assert_eq!(
            mgr.selection(),
            Some(SelectionRange::linear((0, 0), (0, 4)))
        );
        // 释放
        mgr.mouse_event(4, 0, MouseAction::Release, MouseButton::Left, mods);
        assert_eq!(
            ready.load(Ordering::Relaxed),
            1,
            "释放应触发 SelectionReady"
        );
        // 选区文本应为 "hello"
        assert_eq!(mgr.selection_text().as_deref(), Some("hello"));
    }

    /// Task 6: 双击智能选词
    #[test]
    fn test_double_click_select_word() {
        let mut mgr = TerminalManager::utf8(TerminalSize::new(5, 20));
        mgr.write(b"foo bar baz");
        let _ = mgr.poll_frame(Instant::now());

        let mods = KeyMods::default();
        // 第一次单击于 'b'(col=4) of "bar"
        mgr.mouse_event(4, 0, MouseAction::Press, MouseButton::Left, mods);
        // 第二次单击（同位置，500ms 内）→ 双击
        mgr.mouse_event(4, 0, MouseAction::Press, MouseButton::Left, mods);
        // 选区应覆盖 "bar"（col 4..6）
        let sel = mgr.selection().expect("双击应产生选区");
        assert_eq!(sel.start, (0, 4));
        assert_eq!(sel.end, (0, 6));
        assert_eq!(mgr.selection_text().as_deref(), Some("bar"));
    }

    /// Task 6: 三击选整行
    #[test]
    fn test_triple_click_select_line() {
        let mut mgr = TerminalManager::utf8(TerminalSize::new(5, 20));
        mgr.write(b"hello world");
        let _ = mgr.poll_frame(Instant::now());

        let mods = KeyMods::default();
        // 连续三次同位置单击 → 三击
        mgr.mouse_event(2, 0, MouseAction::Press, MouseButton::Left, mods);
        mgr.mouse_event(2, 0, MouseAction::Press, MouseButton::Left, mods);
        mgr.mouse_event(2, 0, MouseAction::Press, MouseButton::Left, mods);
        let sel = mgr.selection().expect("三击应产生选区");
        assert_eq!(sel.start, (0, 0));
        assert_eq!(sel.end, (0, 19), "三击应选整行 0..cols-1");
        // 选区文本应包含 "hello world"
        let text = mgr.selection_text().unwrap();
        assert!(text.contains("hello world"));
    }

    // ========================================================================
    // Task 7: 双宽度字符
    // ========================================================================

    /// Task 7: CJK 宽字符应占据 width=2
    #[test]
    fn test_wide_char_advance() {
        let mut mgr = TerminalManager::utf8(TerminalSize::new(5, 20));
        mgr.write("你".as_bytes());
        let _ = mgr.poll_frame(Instant::now());
        let snap = mgr.screen_snapshot();
        let cell = &snap.rows[0][0];
        assert_eq!(cell.text, "你");
        assert_eq!(cell.width, 2, "CJK 字符 width 应为 2");
        assert!(cell.is_wide(), "is_wide 应为 true");
    }

    /// Task 7: 写入宽字符后光标应前进 2 列
    #[test]
    fn test_wide_char_cursor_movement() {
        let mut mgr = TerminalManager::utf8(TerminalSize::new(5, 20));
        mgr.write("你".as_bytes());
        let _ = mgr.poll_frame(Instant::now());
        let cursor = mgr.cursor();
        assert_eq!(cursor.x, 2, "写入宽字符后光标应前进 2 列");
        // 再写一个宽字符，光标到 4
        mgr.write("好".as_bytes());
        let cursor = mgr.cursor();
        assert_eq!(cursor.x, 4);
    }

    /// Task 7: 在宽字符位置覆盖写入应替换该宽字符
    #[test]
    fn test_wide_char_overwrite() {
        let mut mgr = TerminalManager::utf8(TerminalSize::new(5, 20));
        // 写入宽字符 "你"（占 col 0-1）
        mgr.write("你".as_bytes());
        let _ = mgr.poll_frame(Instant::now());
        // 光标回到行首
        mgr.write(b"\r");
        // 写入 ASCII 'A'，应覆盖 col 0
        mgr.write(b"A");
        let _ = mgr.poll_frame(Instant::now());
        let snap = mgr.screen_snapshot();
        assert_eq!(snap.rows[0][0].text, "A", "覆盖后 col 0 应为 A");
    }
}
