# Tasks

- [x] Task 1: 修复 resolve_color Default 硬编码 bug
  - [x] SubTask 1.1: 修改 `wezterm_core.rs` 的 `resolve_color` 签名，增加 `default_fg`/`default_bg` 参数，`Default` 分支改为使用传入值
  - [x] SubTask 1.2: 修改 `convert_cell` 签名透传 default 色，更新 `snapshot_scrolled`/`screen_snapshot`/`row_cells` 调用点从 manager 取默认色传入
  - [x] SubTask 1.3: 新增测试 `test_default_color_respects_theme`：set_default_fg 后写默认色文本，断言 cell.fg 为设定色

- [x] Task 2: 修复 TextureAtlas 回绕鬼影 bug
  - [x] SubTask 2.1: 在 `atlas.rs` 的 `find_dynamic_slot` 回绕路径中，清理 `dynamic_cache` 中被覆盖区域的旧条目
  - [x] SubTask 2.2: 新增测试 `test_wraparound_clears_stale_entries`：填满动态区触发回绕后，旧 entry 查找返回 None

- [x] Task 3: 清理 BufferNamespace 影子状态
  - [x] SubTask 3.1: 从 `buffer.rs` 的 `BufferNamespace` 移除 `normal`/`alternate`/`active` 字段，保留 `markers` + `next_marker_id`
  - [x] SubTask 3.2: 更新 `manager.rs` 中 `buffers.resize` 调用（改为 no-op 或移除），更新 `buffer()` 保持实时重建逻辑
  - [x] SubTask 3.3: 更新 `api_lock.rs` 测试中对 `BufferNamespace` 字段的访问（若有）

- [x] Task 4: Marker 滚动追踪
  - [x] SubTask 4.1: 在 `BufferNamespace` 增加 `scrollback_offset: usize` 字段，`Marker` 的有效行号 = `marker.line - scrollback_offset`
  - [x] SubTask 4.2: 在 `manager.rs` 的 `write()` 后检测 scrollback 增长（`core` 行数变化），更新 `scrollback_offset`，移除 `line < 0` 的 marker
  - [x] SubTask 4.3: 新增测试 `test_marker_tracks_scroll`：写满一屏后 add_marker，继续写入触发滚动，断言 marker 有效行号递减

- [x] Task 5: 接线 Parser 到 write 数据流
  - [x] SubTask 5.1: 在 `TerminalManager` 增加 `parser: Parser` 字段
  - [x] SubTask 5.2: 在 `write()` 中对输入字节做轻量 OSC 扫描（识别 `ESC ]` ... `BEL`/`ST`），dispatch 到 `parser.dispatch_osc`
  - [x] SubTask 5.3: 新增测试 `test_osc_handler_invoked`：注册 OSC 52 handler，写 OSC 52 序列，断言 handler 被调用

- [x] Task 6: emit 缺失事件
  - [x] SubTask 6.1: 在 `emit_state_events` 中检测 WezTerm bell 状态，emit `Bell` 事件
  - [x] SubTask 6.2: 在 `emit_state_events` 中检测 `icon_name()` 变更，emit `IconNameChange`
  - [x] SubTask 6.3: 通过 Task 5 的 OSC handler 接线 OSC 52，emit `ClipboardRequest(payload)`
  - [x] SubTask 6.4: 新增测试 `test_bell_event`：写 `\x07` 断言 Bell 事件触发

- [x] Task 7: 暴露 bracketed paste 查询
  - [x] SubTask 7.1: 在 `wezterm_core.rs` 增加 `is_bracketed_paste_enabled()` 委托 WezTerm
  - [x] SubTask 7.2: 在 `manager.rs` 暴露 `pub fn is_bracketed_paste_enabled(&self) -> bool`
  - [x] SubTask 7.3: 新增测试 `test_bracketed_paste_query`：写 DECSET 2004 序列后断言查询返回 true

- [x] Task 8: RustXtermCell 增加 hyperlink 字段
  - [x] SubTask 8.1: 在 `cell.rs` 的 `RustXtermCell` 增加 `pub hyperlink: Option<String>`，更新 `blank()` 设为 None
  - [x] SubTask 8.2: 在 `wezterm_core.rs` 的 `convert_cell` 中从 `attrs.hyperlink()` 提取
  - [x] SubTask 8.3: 更新 `api_lock.rs` 等直接构造 RustXtermCell 的测试

- [x] Task 9: FontTree 内存有界化
  - [x] SubTask 9.1: 将 `font_tree.rs` 的 `glyph_cache: HashMap<char, GlyphInfo>` 改为 `LruCache<char, GlyphInfo>`（复用 `lru` crate，硬上限 8192）
  - [x] SubTask 9.2: 将 `font_data_cache: HashMap<ID, Vec<u8>>` 改为 `HashMap<ID, Arc<[u8]>>`，`get_font_data` 返回 `Arc<[u8]>` 克隆
  - [x] SubTask 9.3: 更新 `lookup_glyph`/`lookup_in_face` 使用 LRU API（`get`/`put`）
  - [x] SubTask 9.4: 新增测试 `test_glyph_cache_bounded`：插入超容量字符后断言旧条目被淘汰

- [x] Task 10: 移除 font_tree 硬编码宽度判定
  - [x] SubTask 10.1: 从 `font_tree.rs` 删除 `is_emoji`/`is_wide_char` 函数
  - [x] SubTask 10.2: `lookup_in_face` 的 `is_color`/`advance` 改为参数传入
  - [x] SubTask 10.3: 更新 `renderer.rs` 的 `render_cell_text` 不再依赖 font_tree 的宽度判定
  - [x] SubTask 10.4: 删除 `font_tree.rs` 测试中的 `test_is_emoji`/`test_is_wide_char`

- [x] Task 11: 渲染层 dirty_rects 接线
  - [x] SubTask 11.1: 在 `renderer.rs` 新增 `render_frame(&mut self, dirty_rows: &[u32], cells: &[&[RustXtermCell]]) -> RenderResult` 入口
  - [x] SubTask 11.2: 内部循环调 `render_row`，将每行矩形 push 进 `result.dirty_rects`
  - [x] SubTask 11.3: 新增测试 `test_render_frame_dirty_rects`：仅渲染指定行，断言 dirty_rects 仅含这些行

- [x] Task 12: 缺字画 notdef 方块
  - [x] SubTask 12.1: 在 `font_tree.rs` 的 `lookup_glyph` 全 miss 时返回主字体的 `.notdef` glyph_id
  - [x] SubTask 12.2: `renderer.rs` 的 `render_cell_text` 在 lookup 返回 notdef 时画方块
  - [x] SubTask 12.3: 新增测试 `test_missing_glyph_renders_box`：渲染一个无字体覆盖的字符，断言非空像素

# Task Dependencies
- [Task 6] 的 SubTask 6.3 依赖 [Task 5] — 已满足
- [Task 8] 依赖 [Task 5] — 已满足
- [Task 4] 依赖 [Task 3] — 已满足
- 所有任务已全部完成
