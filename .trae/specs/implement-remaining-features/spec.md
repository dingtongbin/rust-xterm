# 实现剩余未完成特性 Spec

## Why

`FEATURES.md` 汇总显示 rust-xterm 整体达成率约 70%，仍有 5 项未实现 + 5 项部分实现（连字 / 彩色 Emoji / Unicode 14.0 全平面 / 图像协议 / IME 预编辑 / 全局字形缓存 / 智能选词 / 矩形块选 / 选区一致性 / 子行脏区）。这些是终端产品级体验的硬缺口，用户要求**必须实现**，允许提升依赖版本与引入必要依赖（不超过 Rust 1.88 MSRV）。

## What Changes

### 渲染层（rust-xterm-renderer）
- **彩色 Emoji**：`TextureAtlas` 改为 RGBA（`bytes_per_pixel = 4`）；`composite_glyph` 增加 `is_color` 分支，直接写 atlas 的 RGBA 像素而非"前景色 × alpha"；`rasterize_glyph` 切到 `Format::SubpixelRgb` 或多 Source（ColorOutline + ColorBitmap + Outline）。
- **连字（Ligature）**：引入 `swash::shape` 做 GSUB/GPOS 整形；`Renderer::render_row` 改为按"整形 run"绘制（同属性连续 Cell 一次 shape），atlas 缓存键扩展为 `(run_hash, bold, italic)`；`FontTree` 暴露 `shape_run(text, face_id) -> Vec<ShapeGlyph>`。
- **全局字形缓存**：新增 `GlobalAtlas`（`OnceLock<Arc<Mutex<TextureAtlas>>>`），`Renderer` 可选挂载全局 atlas 共享字形，per-instance atlas 仅作 fallback；保持 `#![forbid(unsafe_code)]`（`Mutex` + `Arc` 是安全抽象）。

### 核心层（rust-xterm-core）
- **IME 预编辑**：`TerminalManager` 增加 `composition: Option<String>` + `set_preedit(&str)` / `commit_text(&str)` / `clear_preedit()`；`poll_frame` 在有 composition 时把预编辑文本作为临时脏行（cursor 行带下划线渲染）；`write_input` 包装为可被 `commit_text` 复用。
- **Unicode 14.0 grapheme 聚簇**：引入 `unicode-segmentation` 依赖；`RustXtermCell.text` 仍为 `String`，但 `buffer::selection_text` / `select_word` 改为按 grapheme cluster 边界扩展（ZWJ 序列、国旗对、肤色修饰符聚为一个 cell）；`select_word` 优先识别 URL/路径/IP。
- **智能选词规则**：`select_word` 增强——点击位置若位于 URL（`http://` / `https://` / `ftp://` / `file://` / `ssh://` 前缀）或 Unix 路径（`/` 起始含字母数字`/_-.`）或 IPv4/IPv6 之内，扩展覆盖整个 token；否则回退现有字符类别边界。**手写启发式，不引入 regex/url 依赖**。
- **矩形块选模式**：`handle_selection_mouse` 读取 `mods.alt`，Alt+左键拖拽时设 `rectangular: true`；双击/三击不受 Alt 影响。
- **选区一致性**：`resize` / `scrollback_scroll` 时清除选区并 emit `SelectionChange`；`set_selection` 入参坐标做 clamp（不超过当前 `size`）。
- **子行/列级脏区**：`DamageTracker` 内部增加 `col_dirty: BTreeSet<(usize, Range<usize>)>`（行→列区间集合）；`DirtyRow` 扩展为 `DirtySpan { row, col_start, col_end }`；`render_frame` 按 span 切片合成。

### 图像协议（新模块 rust-xterm-core/src/image.rs）
- **Sixel**：新增 `sixel` 模块，手写 Sixel 解析器（DCS `8;...q` ... `ST`），解析 RLE 压缩 + 调色板，输出 RGBA 像素 + 尺寸；存入 `TerminalManager.images: Vec<Placement>`，关联到 cursor 行/列；`poll_frame` 把图像作为脏区标记；`Renderer` 增加 `render_image` 把 RGBA 块直接 blit 到 canvas。
- **iTerm2 Inline Image**：新增 `iterm2` 模块，解析 OSC 1337 `File=inline=1;...:base64`；引入 `image` crate（PNG/JPEG 解码，必要依赖）；解码后存入 `images`。
- **图像状态管理**：图像随文本滚动（记录 logical row），reflow 时清除（避免错位），选区不覆盖图像像素。

### 依赖变更
- **新增**（必要）：
  - `unicode-segmentation = "1.12"`（grapheme cluster 边界，MSRV 1.61）
  - `image = "0.25"`（iTerm2 inline image PNG/JPEG 解码，MSRV 1.70；features = `["png","jpeg"]` 仅启用需要的解码器以减小体积）
- **不引入**：`regex` / `url`（智能选词用启发式）、`base64`（手写或用 `image` 内部）
- **保持现状**：`swash = "=0.1.15"`（已支持 shape + color）、`fontdb`、`lru`、`encoding_rs`、`tattoy-wezterm-term`
- **MSRV**：所有新增依赖 ≤ Rust 1.88，workspace `rust-version` 保持 `1.88`

## Impact

- **Affected specs**：`FEATURES.md` 第二章（渲染）/第三章（协议）/第四章（图像）/第六章（选区）/第八章（性能）/第九章（输入）全部项由"未实现/部分实现"升为"已实现"
- **Affected code**：
  - `crates/rust-xterm-renderer/src/{atlas.rs,renderer.rs,font_tree.rs,canvas.rs}`
  - `crates/rust-xterm-core/src/{manager.rs,mouse.rs,buffer.rs,selection.rs,events.rs,damage.rs,lib.rs}`
  - `crates/rust-xterm-core/src/image.rs`（新增）、`crates/rust-xterm-core/src/sixel.rs`（新增）、`crates/rust-xterm-core/src/iterm2.rs`（新增）
  - `crates/rust-xterm-core/Cargo.toml`（+ unicode-segmentation）
  - `crates/rust-xterm-renderer/Cargo.toml`（+ image）
  - `Cargo.toml` workspace.dependencies（+ unicode-segmentation / image）
- **Breaking**：
  - `TextureAtlas::new` 的 `bytes_per_pixel` 默认从 1 改为 4（调用方 `Renderer::new` 自动适配，但外部直接构造者需更新）
  - `DirtyRow` 结构可能改名/扩展为 `DirtySpan`（影响 `FrameUpdate` 消费方）

## ADDED Requirements

### Requirement: 彩色 Emoji 渲染
系统 SHALL 在 `TextureAtlas` 以 RGBA 格式存储字形像素，对 `is_color = true` 的字形在合成阶段直接写入原始 RGBA 而非"前景色 × alpha"。

#### Scenario: Emoji 单字符渲染
- **WHEN** 终端写入 U+1F600（😀）
- **THEN** `FontTree::lookup_glyph` 标记 `is_color = true`
- **AND** `rasterize_glyph` 使用 ColorOutline/ColorBitmap source 输出 RGBA
- **AND** `composite_glyph` 检测 `entry.is_color`，从 atlas `sample_rgba` 取像素直接写 canvas

#### Scenario: 普通 ASCII 不受影响
- **WHEN** 渲染 'A'
- **THEN** `is_color = false`，走原有 alpha × 前景色路径

### Requirement: 连字渲染
系统 SHALL 使用 `swash::shape` 对同属性（fg/bg/flags）连续 Cell 做 GSUB/GPOS 整形，按整形后的 glyph run 绘制。

#### Scenario: Fira Code 连字
- **WHEN** 主字体为 Fira Code 且写入 `!=`
- **THEN** shape 输出单个 ligature glyph，atlas 按 run 缓存
- **AND** 渲染宽度仍按 cell.width（WezTerm 权威），不改变布局

### Requirement: 智能选词
系统 SHALL 在 `select_word` 时优先检测点击位置是否位于 URL / Unix 路径 / IPv4 / IPv6 内，若是则扩展覆盖整个 token，否则回退字符类别边界。

#### Scenario: 选中 URL
- **WHEN** 行内容为 `see https://example.com/path now` 且双击 "example"
- **THEN** 选区覆盖 `https://example.com/path`

#### Scenario: 普通 word
- **WHEN** 行内容为 `hello world` 且双击 "hello"
- **THEN** 选区覆盖 `hello`（回退字符类别）

### Requirement: 矩形块选
系统 SHALL 在 Alt+左键拖拽时生成 `rectangular = true` 的选区。

#### Scenario: Alt+drag
- **WHEN** 按住 Alt 并左键拖拽
- **THEN** `selection.rectangular = true`，`selection_text` 按列截取

### Requirement: IME 预编辑
系统 SHALL 维护 `composition: Option<String>`，`set_preedit(text)` 设置后下一帧 cursor 行渲染带下划线的预编辑文本，`commit_text` 把文本写入 PTY 并清空 composition。

#### Scenario: 中文输入
- **WHEN** 用户通过 IME 输入 "你好"
- **THEN** `set_preedit("你")` 触发脏帧，cursor 行显示 "你" 带下划线
- **AND** `commit_text("你好")` 把 "你好" 写入 PTY，composition 清空

### Requirement: 图像协议
系统 SHALL 解析 Sixel（DCS `8;...q`）与 iTerm2 Inline Image（OSC 1337 File=），把图像 RGBA 与逻辑位置存入 `images`，渲染时 blit 到 canvas。

#### Scenario: Sixel 图像
- **WHEN** PTY 写入 `\x1bP1;0;0q...` Sixel 序列
- **THEN** 解析为 RGBA + 尺寸，存入 images，下一帧渲染到 cursor 位置

### Requirement: 全局字形缓存
系统 SHALL 提供 `GlobalAtlas`（`OnceLock<Arc<Mutex<TextureAtlas>>>`），多个 `Renderer` 实例可共享字形缓存，减少多终端实例内存。

### Requirement: 子行/列级脏区
系统 SHALL 在 `DamageTracker` 按 `(row, col_range)` 追踪脏区，`render_frame` 仅重绘脏列区间。

### Requirement: Unicode grapheme 聚簇
系统 SHALL 使用 `unicode-segmentation` 按 grapheme cluster 边界扩展选区，确保 ZWJ 序列 / 国旗对 / 肤色修饰符不被拆分。

## MODIFIED Requirements

### Requirement: 选区一致性
选区在 `resize` / `scrollback_scroll` 时 SHALL 被清除并 emit `SelectionChange`；`set_selection` 入参坐标 SHALL 被 clamp 到当前 `size`。

### Requirement: 字体回退
`FontTree` SHALL 在系统字体扫描时使用 `fontdb` 的 `load_system_fonts` 全量扫描，移除硬编码 Noto/DejaVu 列表的强依赖（保留作为优先查询）。

## REMOVED Requirements

无移除。
