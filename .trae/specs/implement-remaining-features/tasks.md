# Tasks

- [x] Task 1: 彩色 Emoji（Color Glyph）渲染
  - [x] SubTask 1.1: `crates/rust-xterm-renderer/src/atlas.rs` — `TextureAtlas::new` 默认 `bytes_per_pixel = 4`（RGBA），`write_pixels` / `sample_alpha` / `sample_rgba` 已支持 4 字节路径（验证无回归）；新增 `insert_dynamic` 在 `is_color = true` 时写入 RGBA 4 字节
  - [x] SubTask 1.2: `crates/rust-xterm-renderer/src/renderer.rs` — `Renderer::new` 调用 `TextureAtlas::new(.., 4, ..)`；`rasterize_glyph` 在 `is_color = true` 时切到 `Format::SubpixelRgb` 或多 Source（ColorOutline(0) + ColorBitmap(BestFit) + Outline），输出 RGBA；`composite_glyph` 检测 `entry.is_color`，从 `sample_rgba` 取像素直接 `canvas.blend_pixel`，不走"前景色 × alpha"
  - [x] SubTask 1.3: `font_tree.rs` — `lookup_glyph` 对 Emoji 区段（U+1F300–1FAFF、U+2600–27BF）自动设 `is_color = true`（基于 `has_color` 字体面属性，而非硬编码）
  - [x] SubTask 1.4: 新增测试 `test_color_emoji_rasterize_rgba`：lookup U+1F600 断言 `is_color = true`、rasterize 返回 RGBA 数据；`test_color_emoji_composite`：合成后 canvas 像素为 emoji 原色（非前景色）
  - [x] SubTask 1.5: 回归测试 `test_ascii_still_alpha`：渲染 'A' 断言仍走 alpha × 前景色路径

- [x] Task 2: 连字（Ligature）渲染
  - [x] SubTask 2.1: `crates/rust-xterm-renderer/src/font_tree.rs` — 新增 `shape_run(text: &str, face_id: ID) -> Vec<ShapeGlyph>`，内部用 `swash::shape::Shape::new(..).add_str(text).shape()`；`ShapeGlyph { glyph_id, x_offset, y_offset, advance, cluster }`
  - [x] SubTask 2.2: `atlas.rs` — 缓存键扩展为 `(run_hash: u64, bold, italic)`，新增 `insert_run` / `lookup_run`（run_hash 用 FxHash 或 std::collections::DefaultHasher 对 shape 后的 glyph_id 序列哈希）
  - [x] SubTask 2.3: `renderer.rs` — `render_row` 改为按"同属性 Cell run"分组：遍历 Cell 收集连续同 (fg/bg/flags) 的 text 拼接为 run_str → `shape_run` → 按 cluster 映射回 cell 宽度绘制；保持 cell.width 布局权威（ligature glyph 居中或左对齐于 cell 宽度内）
  - [x] SubTask 2.4: `RendererConfig` 增加 `enable_ligatures: bool`（默认 true），false 时回退单字符路径
  - [x] SubTask 2.5: 新增测试 `test_ligature_shape_run`：shape "!=" 断言返回 1 个 glyph（Fira Code 系统装了的话，否则 skip）；`test_ligature_disabled`：`enable_ligatures=false` 走单字符路径

- [x] Task 3: 智能选词规则（URL/路径/IP）
  - [x] SubTask 3.1: `crates/rust-xterm-core/src/buffer.rs` — `select_word` 改为：先取点击位置所在 token（空白分隔），检测是否匹配 URL scheme（`http://` / `https://` / `ftp://` / `file://` / `ssh://` 前缀，大小写不敏感）/ Unix 路径（`/` 起始，含 `[A-Za-z0-9/_.\-]`）/ IPv4（`\d{1,3}(\.\d{1,3}){3}`）/ IPv6（含 `:` 的 hex），匹配则扩展覆盖整个 token，否则回退现有 `cell_class` 边界
  - [x] SubTask 3.2: 提取 `fn detect_token_bounds(pos, row_cells) -> Option<(usize, usize)>` 辅助函数（token = 连续非空白字符序列）
  - [x] SubTask 3.3: 新增测试 `test_select_word_url`：行 `see https://ex.com/p now` 双击 "ex" 断言选区为 `https://ex.com/p`；`test_select_word_unix_path`：`/usr/local/bin` 双击断言全选；`test_select_word_ipv4`：`192.168.1.1` 双击断言全选；`test_select_word_plain`：`hello world` 双击 "hello" 断言选 "hello"
  - [x] SubTask 3.4: 不引入 `regex` / `url` crate，纯手写启发式

- [x] Task 4: 矩形块选模式
  - [x] SubTask 4.1: `crates/rust-xterm-core/src/manager.rs` — `handle_selection_mouse` 把 `_mods` 改为 `mods`；Press(Left) 单击分支记录 `rectangular = mods.alt`；Move 拖拽分支用 `self.mouse_state.alt_held`（新增字段）决定 `rectangular`
  - [x] SubTask 4.2: `mouse.rs` — `MouseState` 增加 `alt_held: bool` 字段
  - [x] SubTask 4.3: 新增测试 `test_alt_drag_rectangular`：模拟 Alt+左键按下拖拽，断言 `selection.rectangular == true`；`test_no_alt_linear`：无 Alt 拖拽断言 `rectangular == false`

- [x] Task 5: 选区一致性校验
  - [x] SubTask 5.1: `crates/rust-xterm-core/src/manager.rs` — `resize` 方法在调用前后清除 `self.selection = None` 并 emit `SelectionChange`；新增 `scrollback_scroll(offset)` 方法同样清选区
  - [x] SubTask 5.2: `set_selection` 入参 `range` 的 `start`/`end` 坐标 clamp 到 `(0..rows, 0..cols)`
  - [x] SubTask 5.3: 新增测试 `test_resize_clears_selection`：设选区 → resize → 断言 selection == None 且 SelectionChange 被 emit；`test_set_selection_clamp`：传入超界坐标断言被 clamp

- [x] Task 6: IME 预编辑
  - [x] SubTask 6.1: `crates/rust-xterm-core/src/manager.rs` — 增加 `composition: Option<String>` 字段；`set_preedit(text: &str)` 设 composition 并标记 cursor 行为脏；`commit_text(text: &str)` 调 `write_input(text.as_bytes())` 后清 composition；`clear_preedit()` 清 composition 并标脏
  - [x] SubTask 6.2: `poll_frame` 在 `composition.is_some()` 时把 cursor 行加入脏行列表，并在 `FrameUpdate` 中通过新增字段 `preedit: Option<(String, usize)>`（text + cursor 行）传给渲染层
  - [x] SubTask 6.3: `crates/rust-xterm-renderer/src/renderer.rs` — `render_row` 对 cursor 行额外绘制 composition 文本（带下划线，位于 cursor 之后），不改变 cell 布局
  - [x] SubTask 6.4: `events.rs` 新增 `TerminalEvent::PreeditChange(String)` 变体
  - [x] SubTask 6.5: 新增测试 `test_set_preedit_marks_dirty`：set_preedit 后 poll_frame 返回含 cursor 行的 FrameUpdate；`test_commit_text_writes_pty`：commit_text 后 drain_output 含提交文本；`test_clear_preedit`

- [x] Task 7: Unicode 14.0 grapheme 聚簇
  - [x] SubTask 7.1: `crates/rust-xterm-core/Cargo.toml` 新增 `unicode-segmentation = "1.12"`（workspace.dependencies 也加）
  - [x] SubTask 7.2: `buffer.rs` — `selection_text` / `select_word` 在确定列边界时用 `UnicodeSegmentation::graphemes` 聚簇，避免拆分 ZWJ 序列 / 国旗对 / 肤色修饰符
  - [x] SubTask 7.3: `lib.rs` 重导出 `unicode_segmentation::UnicodeSegmentation`（或封装为内部 trait）
  - [x] SubTask 7.4: 新增测试 `test_grapheme_zwj_selection`：行含 "👨‍👩‍👧"（family ZWJ）断言双击选中整个序列；`test_grapheme_flag_selection`：国旗对 🇨🇳 断言选中两个 regional indicator

- [x] Task 8: 全局字形缓存
  - [x] SubTask 8.1: `crates/rust-xterm-renderer/src/global_atlas.rs`（新增）— `GlobalAtlas` 用 `OnceLock<Arc<Mutex<TextureAtlas>>>`，`get_or_init(config) -> Arc<Mutex<TextureAtlas>>`，`try_get() -> Option<Arc<Mutex<TextureAtlas>>>`
  - [x] SubTask 8.2: `renderer.rs` — `Renderer` 增加 `global_atlas: Option<Arc<Mutex<TextureAtlas>>>` 字段；`Renderer::with_global_atlas(config)` 构造时挂载全局 atlas；`render_cell_text` 优先从全局 atlas 查（lock 后 lookup），miss 则回退 per-instance
  - [x] SubTask 8.3: 保持 `#![forbid(unsafe_code)]`：`OnceLock` / `Arc` / `Mutex` 均为安全抽象
  - [x] SubTask 8.4: 新增测试 `test_global_atlas_shared`：两个 Renderer 共享同一 global atlas，renderer A 插入 'X' 后 renderer B 查 'X' 命中

- [x] Task 9: Sixel 图像协议
  - [x] SubTask 9.1: `crates/rust-xterm-core/src/image.rs`（新增）— 定义 `ImagePlacement { rgba: Vec<u8>, width: u32, height: u32, row: usize, col: usize }`；`ImageStore` 管理 `Vec<ImagePlacement>`
  - [x] SubTask 9.2: `crates/rust-xterm-core/src/sixel.rs`（新增）— 手写 Sixel 解析器 `parse_sixel(data: &[u8]) -> Option<ImagePlacement>`：解析 DCS 参数（`P1;P2;P3`）、color registers（`#register;co;hue;sat;lum` 或 `#register`）、RLE 重复（`!n!char`）、6 行一组的像素布局；输出 RGBA
  - [x] SubTask 9.3: `manager.rs` — `write` 在检测到 DCS `\x1bP` 开头时分流到 Sixel 解析器（与 OSC/CSI 分流类似），解析成功存入 `images`
  - [x] SubTask 9.4: `poll_frame` 把 image placement 覆盖的行列标为脏
  - [x] SubTask 9.5: `crates/rust-xterm-renderer/src/renderer.rs` — `render_image(placement)` 把 RGBA 块直接 blit 到 canvas 对应像素区域
  - [x] SubTask 9.6: 新增测试 `test_sixel_parse_simple`：解析一个 2x2 红色 Sixel 断言 RGBA 正确；`test_sixel_rle`：解析 `!5!~` RLE 重复断言 5 个像素

- [x] Task 10: iTerm2 Inline Image 协议
  - [x] SubTask 10.1: `crates/rust-xterm-renderer/Cargo.toml` 新增 `image = { version = "0.25", default-features = false, features = ["png","jpeg"] }`（workspace.dependencies 也加）；验证 MSRV ≤ 1.88
  - [x] SubTask 10.2: `crates/rust-xterm-core/src/iterm2.rs`（新增）— 解析 OSC 1337 `File=inline=1;width=...;height=...:base64data\x07`；base64 手写解码（不引入 base64 crate，~50 行）；用 `image::load_from_memory` 解码 PNG/JPEG 为 RGBA
  - [x] SubTask 10.3: `manager.rs` — 注册 OSC 1337 handler（与 OSC 7 类似），解析成功存入 `images`
  - [x] SubTask 10.4: 新增测试 `test_iterm2_png_decode`：构造一个 1x1 PNG base64 序列，断言解析为 RGBA

- [x] Task 11: 子行/列级脏区
  - [x] SubTask 11.1: `crates/rust-xterm-core/src/damage.rs` — `DamageTracker` 增加 `col_spans: BTreeMap<usize, Vec<(usize, usize)>>`（行→列区间列表）；`mark_cell_dirty(row, col)` / `mark_span_dirty(row, col_start, col_end)`
  - [x] SubTask 11.2: `FrameUpdate` 的 `DirtyRow` 扩展为 `DirtySpan { row: usize, col_start: usize, col_end: usize }`（向后兼容：全行 dirty 用 `col_end = cols`）
  - [x] SubTask 11.3: `renderer.rs` — `render_frame` 接收 `&[DirtySpan]`，按 span 切片 cells 后调用 `render_row_segment(row, col_start, col_end, cells_slice)`
  - [x] SubTask 11.4: 新增测试 `test_col_level_dirty`：仅标记 (5, 10..20) 脏，断言 render_frame 仅重绘该列区间

- [x] Task 12: 依赖隔离与 MSRV 验证
  - [x] SubTask 12.1: `cargo +1.88.0 build --all-targets` 通过（workspace + 三个 demo）
  - [x] SubTask 12.2: `cargo +1.88.0 test --all-targets` 通过，新增测试全绿
  - [x] SubTask 12.3: `cargo +1.88.0 clippy --all-targets -- -D warnings` 通过
  - [x] SubTask 12.4: `cargo +1.88.0 fmt --all -- --check` 通过
  - [x] SubTask 12.5: 三个 demo 的 `Cargo.lock` 不被新依赖污染（unicode-segmentation / image 仅在 workspace 传递依赖，demo 通过 path 引用自动获得，无需 demo 改 Cargo.toml）
  - [x] SubTask 12.6: `/workspace/Cargo.lock` 不含 GUI 框架（grep slint/iced/egui 无输出）

- [x] Task 13: 文档更新
  - [x] SubTask 13.1: `FEATURES.md` 把第二章（连字/彩色Emoji/Unicode）、第四章（Sixel/iTerm2）、第六章（智能选词/矩形块选/选区一致性）、第八章（全局字形缓存/子行脏区）、第九章（IME）所有项从"未实现/部分实现"改为"已实现"；更新汇总表
  - [x] SubTask 13.2: `PROGRESS.md` 增加本轮交付清单
  - [x] SubTask 13.3: `README.md` Features 列表新增 IME / Sixel / iTerm2 image / ligature / color emoji / global atlas / sub-row dirty

# Task Dependencies
- [Task 1 彩色 Emoji] 与 [Task 2 连字] 都改 renderer/atlas/font_tree，有冲突风险 → **顺序执行**（先 Task 1 再 Task 2）
- [Task 2 连字] 依赖 [Task 1]（atlas RGBA 路径已就绪）
- [Task 3 智能选词] 与 [Task 7 grapheme] 都改 buffer.rs select_word → [Task 7] 依赖 [Task 3]（先做 token 检测再做 grapheme 边界修正）
- [Task 4 矩形块选] 独立，可与 Task 3/5 并行
- [Task 5 选区一致性] 独立，可与 Task 4 并行
- [Task 6 IME] 改 manager + renderer，与 Task 1/2 有 renderer 冲突 → 在 Task 2 之后
- [Task 8 全局 atlas] 改 renderer/atlas，在 Task 1 之后
- [Task 9 Sixel] 与 [Task 10 iTerm2] 都新增 image 模块 → **顺序执行**（先 Task 9 再 Task 10 复用 image store）
- [Task 11 子行脏区] 改 damage + renderer，在 Task 2 之后
- [Task 12 验证] 依赖所有实现任务完成
- [Task 13 文档] 依赖 Task 12

**并行机会**：
- Task 3 + Task 4 + Task 5（core 层，互不冲突）
- Task 9 完成后 Task 10 可与 Task 11 并行（不同模块）
