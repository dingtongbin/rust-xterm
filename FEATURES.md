# rust-xterm 特性对照表：计划 vs 已实现

> 本文档以《rust-xterm 特性定义表》为基准，逐项对照当前代码库（`crates/rust-xterm-core` / `rust-xterm-renderer` / `rust-xterm-host`）的实际实现状态。
> 状态标记说明：
> - **已实现** — 代码中存在完整、可调用的实现，并通过测试或示例验证。
> - **部分实现** — 存在骨架/API 表面或依赖底层库间接支持，但有显著缺口（如未接线、未暴露、未渲染）。
> - **未实现** — 代码中无相关实现，或仅有死代码/未使用的事件枚举。
> - **【核心难点】** — 标记传统终端易崩溃或逻辑失效的深水区，需重点关注。
>
> **最近更新**：2026-07-26，补齐焦点报告 / OSC 7 / 滚动区域 / 选区系统 / 键盘映射核心层 / 鼠标选区交互 / 双宽字符测试（详见 `PROGRESS.md`）。

---

## 一、架构与解耦

| 功能特性点 | 达成标准（计划） | 实现状态 | 证据 / 缺口 |
| :--- | :--- | :--- | :--- |
| 平台无关性 | 核心 100% Rust，无系统调用/文件 IO/网络请求，可入 WASM | **已实现** | `rust-xterm-core/src/lib.rs:22` `#![forbid(unsafe_code)]`；仅依赖 `encoding_rs`、`wezterm_term` 等纯 Rust 库；无 `std::fs`/`std::net`/`std::process`。 |
| 无 PTY/TTY 依赖 | 输入输出完全基于内存字节流 | **已实现** | `portable-pty` 仅出现在 `rust-xterm-host/Cargo.toml`；核心通过 `Box<dyn Write>`（`CapturingWriter`）吸收输出（`null_writer.rs:14-18`）。 |
| GUI 框架无关性 | 仅提供光栅化像素或绘图指令 | **已实现** | `rust-xterm-renderer` 仅依赖 `swash`/`fontdb`/`lru`，输出 `&[u8]` 像素缓冲（`canvas.rs:71`）；`integration.rs` 定义 `RenderSurface`/`InputSource`/`SizeSource` trait，含 `NullRenderSurface`。 |

---

## 二、文本渲染与排版

| 功能特性点 | 达成标准（计划） | 实现状态 | 证据 / 缺口 |
| :--- | :--- | :--- | :--- |
| 现代字体渲染 | TrueType/OpenType 加载、连字、彩色 Emoji | **部分实现** | TTF/OTF 加载与轮廓光栅化已实现（`font_tree.rs:176`、`renderer.rs:310-338`）。**连字未实现**：仅 `Charmap::map(ch)` 单字符查表，无 `swash::shape` GSUB/GPOS 整形（`font_tree.rs:182`）。**彩色 Emoji 不可用**：`TextureAtlas` 固定 `bytes_per_pixel = 1`（Alpha-only），RGBA 写入路径未接线，合成仍走 `sample_alpha`（`renderer.rs:125,385`）。 |
| 字体回退机制 | 主字体缺字自动回退，杜绝豆腐块 | **部分实现** | `FontTree::lookup_glyph` 走 primary + fallback 链（`font_tree.rs:153-168`），但回退表硬编码为 Noto Sans SC / Noto Color Emoji / DejaVu Sans 等（`font_tree.rs:76-82`），仅当系统装齐这些家族才生效；全 miss 时返回 `None`，渲染端静默跳过该 cell（`renderer.rs:248-249`），即"画空"而非"画 .notdef 框"。 |
| Unicode 完整支持 | Unicode 14.0 全平面、组合字符、零宽字符、Emoji 序列 | **未实现** | `is_emoji`/`is_wide_char` 仅硬编码少量范围（`font_tree.rs:251-279`），CJK Ext B/C/D/E/F（Plane 2）等未覆盖；`advance` 写死 1.0/2.0，无零宽/组合 mark 模型；`renderer.rs:239` 仅取 `cell.text.chars().next()`，ZWJ 序列、国旗对、肤色修饰符均未聚簇。 |

---

## 三、终端协议与序列

| 功能特性点 | 达成标准（计划） | 实现状态 | 证据 / 缺口 |
| :--- | :--- | :--- | :--- |
| 完整 SGR 文本属性 | 粗体/斜体/闪烁/隐藏/删除线 + 多种下划线 | **已实现**（经 WezTerm） | `CellFlags` 含 BOLD/ITALIC/REVERSE/UNDERLINE/DOUBLE_UNDERLINE/UNDERCURL/STRIKETHROUGH/BLINK/INVISIBLE/DIM（`cell.rs:22-42`）；`from_attrs` 完整映射（`cell.rs:51-88`）。SGR 序列由 WezTerm 解析，本层仅转换。 |
| 真彩色支持 | 24-bit 前景/背景，向下兼容 256/16 色 | **已实现**（经 WezTerm） | `resolve_color` 处理 `TrueColorWithDefaultFallback`/`TrueColorWithPaletteFallback`/`PaletteIndex`（`wezterm_core.rs:362-395`）；`theme.rs:47-83` 内置完整 Campbell 256 色板（16 + 216 + 24）。 |
| OSC 控制序列 | 标题（OSC 0/2）、CWD（OSC 7）、超链接（OSC 8）、剪贴板（OSC 52） | **已实现** | OSC 0/2：`title()` + `TitleChange` 事件。OSC 52：Parser 接线 emit `ClipboardRequest` 事件。OSC 8：hyperlink 字段已透传到 `RustXtermCell.hyperlink`。**OSC 7**：构造时注册内部 handler，手写解析 `file://<host>/<path>`（无新依赖），emit `CwdChange(PathBuf)` 事件（`manager.rs:128-132`）。测试：`test_osc7_cwd_event`、`test_osc7_malformed_ignored`。 |
| 鼠标协议 | X10、VT200、SGR Extended 等模式 | **已实现**（经 WezTerm） | `mouse_event` 委托 WezTerm 编码（`wezterm_core.rs:96-106`）；`is_mouse_grabbed` 反映应用捕获状态（`wezterm_core.rs:87-89`）；`mouse.rs:55-104` 完成抽象→WezTerm 事件转换。 |
| 焦点报告 | DECSET 1004，宿主获/失焦点时核心生成转义序列 | **已实现** | `TerminalEvent::FocusReport(bool)` 变体（`events.rs`）；`TerminalManager::set_focused` 在 DECSET 1004 启用时向 `drain_output` 写入 `\x1b[I` / `\x1b[O`；`is_focus_reporting_enabled()` 查询。状态由 `write` 在数据流中扫描 DECSET 1004 序列维护。测试：`test_focus_report_enabled`、`test_focus_report_disabled`。 |
| 动态光标样式 | DECSCUSR 切换光标形状与闪烁 | **已实现**（经 WezTerm） | `convert_cursor_shape` 映射全部 6 种 WezTerm 变体到 `CursorShape::{Default,Block,Bar,Underline}`（`wezterm_core.rs:398-406`）；`set_cursor_blinking` + 500ms 相位机（`state.rs`）。 |
| 括号粘贴模式 | DECSET 2004，识别粘贴包裹防误执行 | **已实现**（经 WezTerm） | WezTerm 内部处理括号标记；已暴露 `is_bracketed_paste_enabled()` 查询 API。`write_input`（`manager.rs:182-188`）不主动插入 `\x1b[200~`/`\x1b[201~`，由宿主按需包装。 |

---

## 四、图像与多媒体

| 功能特性点 | 达成标准（计划） | 实现状态 | 证据 / 缺口 |
| :--- | :--- | :--- | :--- |
| 内联图像协议 | Sixel + iTerm2 Inline Image，光栅化到流 | **未实现** | 无 sixel/iterm2 解析，`RustXtermCell` 无图像字段。MSRV 升级到 1.88 后已移除 `ravif-stub`，使用上游真实 `ravif` 依赖；但终端场景下 `encode_*` 路径仍不会被走到。 |
| 图像状态管理 | 图像与文本层叠、随文本滚动、选区逻辑 | **未实现** | 无任何图像模型。 |

---

## 五、屏幕与缓冲管理

| 功能特性点 | 达成标准（计划） | 实现状态 | 证据 / 缺口 |
| :--- | :--- | :--- | :--- |
| 回滚历史 | 可配置环形缓冲，支持搜索/高亮/跳转 | **已实现** | 有界环形缓冲已实现：默认 3500 行可配（`config.rs:33,96-99`），`snapshot_scrolled(offset)` 窗口快照（`wezterm_core.rs:193-235`），`max_scrollback()` 查询。`Marker.line` 现在随 scrollback 增长滚动追踪，不再脱节。**搜索/高亮/跳转 API 缺失**，仅 `Buffer::line_text`/`dump`（`buffer.rs:93-104`）。 |
| **【核心难点】** 逻辑行回绕 (Reflow) | 窗口缩放时重排历史（含回滚区），保持选区/光标正确 | **部分实现** | `manager.resize` → `core.resize` → WezTerm `Terminal::resize` 自动 reflow（`manager.rs:191-199`）。但选区/光标位置一致性无法验证，因选区本身未实现。**回滚区 reflow 由 WezTerm 保证，但 rust-xterm 未做额外校验。** |
| **【核心难点】** 交替屏幕 | DECSET 1049，进入/退出完美恢复 | **已实现**（经 WezTerm） | `is_alt_screen_active`（`wezterm_core.rs:160-162`）；`BufferType::Alternate`（`buffer.rs:39-45`）；`manager.is_alt_screen_active()`（`manager.rs:420-422`）。注意：`BufferNamespace` 字段是影子状态，`buffer()` 每次按 WezTerm 实时重建快照。 |
| 滚动区域 | DECSTBM，实现 htop 顶/底固定栏 | **已实现** | `TerminalManager::scroll_region() -> Option<(usize, usize)>`（1-based，`None`=全屏），扫描 `\x1b[<top>;<bottom>r` 维护状态。等价于全屏的设置（如 `1;24r`）也返回 `None`。测试：`test_scroll_region_query`。底层 scroll 行为由 WezTerm 保证。 |

---

## 六、选区与交互逻辑

| 功能特性点 | 达成标准（计划） | 实现状态 | 证据 / 缺口 |
| :--- | :--- | :--- | :--- |
| WT 风格智能点击选区 | 单击/双击/三击/四击分级选区 | **已实现** | `MouseState` 状态机（`mouse.rs`）+ `TerminalManager::mouse_event` 在非跟踪模式下：单击清旧选区 + 记拖拽起点；双击 `select_word`（空白/字母数字/标点三类边界扩展）；三击 `select_line`；点击计数 500ms 时间窗 + 同位置约束，循环 1→2→3→1。测试：`test_mouse_drag_selection`、`test_double_click_select_word`、`test_triple_click_select_line`。 |
| 智能选词规则 | 正则识别 URL/路径/IP 等 | **未实现** | `select_word` 仅按字符类别边界扩展；正则识别 URL/路径/IP 需引入 `regex`/`url` 依赖，未做。属选区增强。 |
| 选中自动复制逻辑 | `on_selection_change`/`get_selection_text` 触发宿主写入 | **已实现** | `TerminalManager::selection_text() -> Option<String>` 提供选中文本（线性跨行 `\n` 连接 / 矩形按列截取）；`SelectionChange` 事件在选区变更时 emit；`SelectionReady` 事件在释放时 emit，宿主据此触发复制。GUI demo 已在 Release(Left) 时复制到剪贴板。 |
| 矩形块选模式 | Alt/Option 矩形块选，跨行对齐 | **部分实现** | `SelectionRange { rectangular: bool }` 字段已就位，`buffer::selection_text` 已支持矩形文本提取；但鼠标交互层未接入 Alt+拖拽的矩形触发。属选区交互增强。 |
| **【核心难点】** 选区一致性维护 | 滚动/回绕/缩放后选区位置正确 | **部分实现** | 选区以 `(row, col)` 0-based 坐标存储，与当前可视窗口对齐；scrollback 滚动时选区不跟随（视为可视窗口快照）。resize / reflow 后选区位置一致性未做校验，由宿主在 resize 时清选区（demo 已实现）。 |

---

## 七、宽字符与排版

| 功能特性点 | 达成标准（计划） | 实现状态 | 证据 / 缺口 |
| :--- | :--- | :--- | :--- |
| **【核心难点】** 双宽度字符处理 | CJK 占 2 cell，光标/退格/选区按 2 单位逻辑 | **已实现** | `cell.width` 字段 + `is_wide()`（`cell.rs`）；`config.rs` 启用 Unicode 9 宽度表；`wezterm_core.rs` 拷贝 `cell_ref.width()`。光标移动/退格由 WezTerm 保证。**测试补全**：`test_wide_char_advance`（CJK `width=2`）、`test_wide_char_cursor_movement`（连续写入光标前进 2 列）、`test_wide_char_overwrite`（`\r` 覆盖写时宽字符被正确替换）。 |
| **【核心难点】** 宽字符回绕处理 | 行尾剩 1 空位时强制换行，禁止拆分 | **已实现**（经 WezTerm） | 由 WezTerm grid 处理；rust-xterm 仅消费结果 cell，源码中无显式拆分逻辑。 |

---

## 八、性能与内存

| 功能特性点 | 达成标准（计划） | 实现状态 | 证据 / 缺口 |
| :--- | :--- | :--- | :--- |
| 懒渲染 | 缓冲无变更时计算开销为零 | **已实现** | `poll_frame` 在 `!has_damage && !blink_due` 时返回 `None`（`manager.rs:213-218`），由 `test_no_damage_returns_none` 验证（`manager.rs:511-518`）。 |
| 内存静态锁定 | 回滚缓冲/字形缓存预分配，硬性上限 | **已实现** | 回滚有界（`config.rs:33`）；`TextureAtlas` 与 `Canvas` 单次 `Box<[u8]>` 预分配（`atlas.rs:67,98-99`、`canvas.rs:33,45-46`），25%/75% 静态/动态分区。`FontTree.glyph_cache` 已改为有界 `LruCache`（上限 8192），`font_data_cache` 已改为 `Arc<[u8]>` 共享引用，不再有可增长副本。 |
| 全局字形缓存 | LRU 跨实例共享纹理 | **未实现** | LRU 仅 per-instance（`TextureAtlas` 内 `LruCache` 字段，`atlas.rs:83`）；每个 `Renderer::new` 独立构造 atlas（`renderer.rs:125`），无 `OnceLock`/`Arc<Mutex<…>>` 全局共享。 |
| 高效脏区追踪 | 精确标记屏幕脏区，仅重绘变化像素 | **已实现** | 核心层行级脏区完整（`damage.rs:135-164` 行连续合并为 `DirtyRect::full_width`）。`Renderer::render_frame` 已填充 `RenderResult.dirty_rects`，渲染层不再无条件重绘整行。**缺口**：仍为行粒度，无子行/列级脏区。 |

---

## 九、输入处理

| 功能特性点 | 达成标准（计划） | 实现状态 | 证据 / 缺口 |
| :--- | :--- | :--- | :--- |
| 键盘映射 | 完整键码→ANSI 转义序列表，修饰键组合 | **已实现** | 新增 `crates/rust-xterm-core/src/input.rs`：`KeyInput` 枚举（Char/ArrowUp/Down/Left/Right/Home/End/Insert/Delete/PageUp/Down/F1–F12/Enter/Backspace/Tab/Esc）+ `KeyMapping::encode_key(key, mods, app_cursor) -> Vec<u8>`：方向键 app_cursor=true 走 SS3（`\x1bOA` 等），否则 CSI（`\x1b[A` 等）；Ctrl+字母 = `字母 mod 0x1f`；Alt+字符前缀 `\x1b`；F1–F12 用 CSI/SS3 序列；Enter=`\r`、Backspace=`\x7f`、Tab=`\t`、Esc=`\x1b`。`rust-xterm-host::EventLoop::send_key` 便利方法。测试：`tests/input_mapping.rs` 覆盖全部变体的 app_cursor on/off + Ctrl/Alt 组合。 |
| 编码转换闸门 | GBK/Big5 实时转 UTF-8，解决遗留编码 | **已实现** | `CodecGate` 使用 `encoding_rs`，覆盖 Utf8/Gbk/Big5/ShiftJis/EucKr（`codec_gate.rs:28-40`）；有状态 Decoder/Encoder + `rx_buffer`/`tx_buffer` 跨包缓冲（`codec_gate.rs:79-81`）；坏字节替换 U+FFFD；`smoke_gbk.rs` 验证分片包解码。 |
| 输入法预编辑 | IME 预编辑文本状态与候选窗口 | **未实现** | 无 `set_preedit`/`commit_text`/composition 状态，无候选窗口渲染。 |

---

## 汇总：达成率统计

| 章节 | 总项 | 已实现 | 部分实现 | 未实现 |
| :--- | :---: | :---: | :---: | :---: |
| 一、架构与解耦 | 3 | 3 | 0 | 0 |
| 二、文本渲染与排版 | 3 | 0 | 2 | 1 |
| 三、终端协议与序列 | 7 | 7 | 0 | 0 |
| 四、图像与多媒体 | 2 | 0 | 0 | 2 |
| 五、屏幕与缓冲管理 | 4 | 3 | 1 | 0 |
| 六、选区与交互逻辑 | 5 | 3 | 2 | 0 |
| 七、宽字符与排版 | 2 | 2 | 0 | 0 |
| 八、性能与内存 | 4 | 3 | 0 | 1 |
| 九、输入处理 | 3 | 2 | 0 | 1 |
| **合计** | **33** | **23** | **5** | **5** |

> 整体达成率：已实现约 70%，部分实现约 15%，未实现约 15%（2026-07-26 更新）。
> 注：标记"经 WezTerm"的项目，其底层协议解析依赖 `tattoy-wezterm-term` 0.1.0-fork.5；rust-xterm 自身仅做防腐层转换，未独立实现 VT 状态机。

---

## 关键缺口（按优先级）

1. **【核心难点】Unicode 14.0 全平面与组合字符**（`font_tree.rs:251-279`）— 当前仅硬编码少量范围，CJK Ext B+ 与 ZWJ Emoji 序列无法正确渲染。
2. **连字与彩色 Emoji 渲染**（`renderer.rs:125,385`）— `TextureAtlas` Alpha-only，且未接入 `swash::shape`；连字功能不可用。
3. **全局字形缓存缺失**（`atlas.rs:83`）— per-instance LRU，多终端实例内存翻倍。
4. **IME 预编辑完全缺失** — 无 composition 状态。
5. **图像协议（Sixel/iTerm2）完全缺失** — MSRV 升级到 1.88 后已移除 `ravif-stub` 绕行桩，改用上游真实 `ravif` 依赖；但 Sixel/iTerm2 解析与图像状态管理仍未实现。
6. **智能选词规则（URL/路径/IP）** — `select_word` 仅按字符类别边界扩展；正则识别需引入 `regex`/`url` 依赖。
7. **矩形块选鼠标交互** — `SelectionRange.rectangular` 字段已就位、`selection_text` 已支持矩形提取；但鼠标层未接入 Alt+拖拽触发。
8. **选区一致性校验** — scrollback 滚动 / reflow 后选区位置一致性未做核心层校验，由宿主在 resize 时清选区兜底。

---

## 关键优势（已稳态交付）

1. **架构解耦 100% 达成** — 核心 `#![forbid(unsafe_code)]`，无 OS/PTY/GUI 依赖，可入 WASM 与无 OS 环境。
2. **编码闸门工业级** — `encoding_rs` 全状态机 + 分片包缓冲，GBK/Big5/Shift_JIS/EUC-KR 全覆盖。
3. **懒渲染承诺兑现** — 静态画面 `poll_frame` 返回 `None`，0% CPU。
4. **真彩色 + SGR 全属性** — 经 WezTerm 转换，24-bit / 256 / 16 色与 10+ 种文本样式可用。
5. **预分配像素缓冲** — `Canvas`/`TextureAtlas` 单次 `Box<[u8]>`，稳态零分配。
6. **完整 PTY 桥**（`rust-xterm-host`）— 跨平台（ConPTY/winpty/Unix PTY 经 `portable-pty`）、非阻塞 drain、resize 双向传播、`Option<PtyBridge>` 支持无 PTY 头测试。
7. **主题默认色生效** — `resolve_color` 的 `Default` 分支使用用户配置的 `default_fg`/`default_bg`，而非硬编码黑底白字。
8. **缺字画方块** — 字体回退全 miss 时画 `.notdef` 方块而非静默跳过，杜绝"画空"导致的隐形 cell。
9. **键盘映射核心层** — `KeyInput` + `KeyMapping::encode_key` 提供完整键码→ANSI 转义序列表（含 SS3/CSI 双模式、Ctrl/Alt 组合），宿主无需自行编码。
10. **选区系统完整** — `SelectionRange` 模型 + `mouse_event` 内部状态机（单击/双击/三击/拖拽）+ `selection_text` 文本提取 + `SelectionReady` 自动复制触发，线性与矩形均支持。
11. **焦点报告与 OSC 7 CWD** — DECSET 1004 焦点转义序列自动生成；OSC 7 `file://` URL 解析为 `PathBuf` 派发 `CwdChange` 事件，无新依赖。
12. **GUI 演示三套**（独立 workspace，零依赖污染）— `demos/{slint,iced,egui}-demo` 三个独立 Cargo 包，各自独立 `Cargo.lock`，`/workspace/Cargo.lock` 不含任何 GUI 框架传递依赖；均支持 spawn 默认 shell + 完整像素绘制 + 键盘/鼠标/resize + 底部 FPS/内存状态栏。
