# rust-xterm 特性完成度对照

> 本文档对照 `.trae/specs/extend-features-and-gui-demos/spec.md` 列出本轮已完成的特性、`FEATURES.md` 中超出范围的特性，以及三个独立 GUI demo 的完成状态与依赖隔离验证结果。
>
> 状态标记：
> - **已完成** — 本轮交付，代码 + 测试通过
> - **超出范围** — 本轮不交付，附理由
> - **经 WezTerm** — 底层协议由 `tattoy-wezterm-term` 0.1.0-fork.5 解析，rust-xterm 仅做防腐层转换

---

## 一、本轮已完成特性（spec.md Task 1–8）

| # | 特性 | 完成证据（代码 / 测试） |
| :-: | :--- | :--- |
| 1 | **焦点报告（DECSET 1004）** | `events::TerminalEvent::FocusReport(bool)` 变体；`TerminalManager::set_focused` 在 DECSET 1004 启用时向 `drain_output` 写入 `\x1b[I` / `\x1b[O`；`is_focus_reporting_enabled()` 查询。测试：`test_focus_report_enabled`、`test_focus_report_disabled`。 |
| 2 | **OSC 7 CWD 事件** | `events::TerminalEvent::CwdChange(PathBuf)`；构造时注册内部 OSC 7 handler，手写解析 `file://<host>/<path>`（无新依赖），emit `CwdChange`。测试：`test_osc7_cwd_event`、`test_osc7_malformed_ignored`（格式错误不触发事件不 panic）。 |
| 3 | **滚动区域查询 API** | `TerminalManager::scroll_region() -> Option<(usize, usize)>`（1-based，`None`=全屏），扫描 DECSTBM `\x1b[<top>;<bottom>r` 维护状态。测试：`test_scroll_region_query`（设 / 重置 / 等价全屏 三种情况）。 |
| 4 | **键盘映射核心层** | 新增 `crates/rust-xterm-core/src/input.rs`，定义 `KeyInput` 枚举（Char/Arrow/Home/End/Insert/Delete/PageUp/Down/F1–F12/Enter/Backspace/Tab/Esc）+ `KeyMapping::encode_key(key, mods, app_cursor) -> Vec<u8>`：方向键 app_cursor=true 走 SS3、否则 CSI；Ctrl+字母 = `字母 mod 0x1f`；Alt+字符前缀 `\x1b`；Enter=`\r`、Backspace=`\x7f`、Tab=`\t`、Esc=`\x1b`。`rust-xterm-host::EventLoop::send_key` 便利方法。测试：`tests/input_mapping.rs` 覆盖全部变体的 app_cursor on/off + 修饰键组合。 |
| 5 | **选区系统模型与 API** | `events::SelectionReady` 事件；`SelectionRange { start, end, rectangular }`；`TerminalManager::{set_selection, selection, selection_text}`；`buffer::selection_text` 实现线性跨行（`\n` 连接）+ 矩形按列截取。测试：`test_selection_linear_text`、`test_selection_rectangular_text`。 |
| 6 | **鼠标选区交互** | `MouseState` 状态机字段；`TerminalManager::mouse_event` 在非鼠标跟踪模式下：单击清旧选区 + 记拖拽起点；双击 `select_word`（空白/字母数字/标点三类边界扩展）；三击 `select_line`；拖拽扩展终点 emit `SelectionChange`；释放 emit `SelectionReady`。点击计数 500ms 时间窗 + 同位置约束，循环 1→2→3→1。测试：`test_mouse_drag_selection`、`test_double_click_select_word`、`test_triple_click_select_line`。 |
| 7 | **双宽度字符测试补全** | 验证 WezTerm 提供的 `cell.width` 字段 + `is_wide()`：CJK 字符 `width=2`；连续写入光标前进 2 列；`\r` 覆盖写普通字符时宽字符被正确替换。测试：`test_wide_char_advance`、`test_wide_char_cursor_movement`、`test_wide_char_overwrite`。 |
| 8 | **核心层 1.88 全绿** | `cargo +1.88.0 build --all-targets`、`cargo +1.88.0 test --all-targets`、`cargo +1.88.0 clippy --all-targets -- -D warnings`、`cargo +1.88.0 fmt --all -- --check` 全部通过。`rust-xterm-core` 单元测试 62 项全部通过。 |

**汇总**：spec Task 1–8 共 8 项特性 / 36 项子任务，**100% 完成**。

---

## 二、超出范围的特性（FEATURES.md 中未实现项）

下列特性在 `FEATURES.md` 中标记为"未实现"或"部分实现"，本轮**不做**，理由如下：

| 特性 | 范围分类 | 超出范围理由 |
| :--- | :--- | :--- |
| **连字（Ligature）** | 渲染层大规模重写 | 当前 `Renderer::render_cell_text` 仅取 `cell.text.chars().next()` 单字符查 atlas；连字需引入 `swash::shape` GSUB/GPOS 整形，重建 `FontTree::lookup_glyph` 的"单 char → glyph"模型为"整形上下文 → glyph run"，并修改 `TextureAtlas` 的缓存键以包含前后邻居字符。涉及 `font_tree.rs` / `atlas.rs` / `renderer.rs` 三层重写，超出本轮 spec 范围。 |
| **彩色 Emoji（Color Glyph）** | 渲染层 + Atlas 数据结构 | `TextureAtlas` 固定 `bytes_per_pixel = 1`（Alpha-only），无法承载 RGBA。改造为 RGBA atlas 需：(a) Atlas 像素格式改为 4 字节；(b) `composite_glyph` 改为源色覆盖合成（保留 emoji 原色），与现有"前景色 × alpha"路径分叉；(c) `Render::new(sources).format(Format::Alpha)` 切到 `Format::SubpixelRgb` 或扩展多 Source。需重新设计合成管线，超出本轮范围。 |
| **Unicode 14.0 全平面 / 组合字符 / ZWJ 序列** | Unicode 模型重写 | `font_tree::is_emoji` / `is_wide_char` 仅硬编码少量范围；CJK Ext B/C/D/E/F（Plane 2）未覆盖；`advance` 写死 1.0/2.0，无零宽 / 组合 mark 模型；`renderer.rs` 仅取 `chars().next()`，ZWJ 序列、国旗对、肤色修饰符均未聚簇。需引入 `unicode-width` / `unicode-segmentation` 依赖 + 改写 Cell 文本模型（从 `String` 到 `Vec<char>` + 簇边界），影响 `RustXtermCell` ABI，超出范围。 |
| **图像协议（Sixel / iTerm2 Inline Image）** | 协议解析 + 图像状态机 | `RustXtermCell` 无图像字段；无 sixel / iTerm2 解析器；无图像与文本层叠 / 随滚动 / 选区逻辑。需新增 `image` 模块（解析器 + 状态机）、扩展 Cell 模型、改写渲染管线支持 RGBA 图像块绘制，规模超出本轮 spec。 |
| **IME 预编辑（Input Method Editor）** | 状态机 + 候选窗口渲染 | 无 `set_preedit` / `commit_text` / composition 状态；无候选窗口渲染。预编辑需要：(a) 核心层增加 composition buffer + 在 cursor 处渲染带下划线的预编辑文本；(b) 宿主层处理 IME 事件（各 GUI 框架 API 不一致）；(c) 候选窗口的窗口管理与绘制。涉及核心 + 宿主双层设计，超出范围。 |
| **全局字形缓存（Cross-instance LRU）** | 并发安全设计 | `TextureAtlas` 与 `FontTree` 均 per-instance；每个 `Renderer::new` 独立构造。改造为 `OnceLock<Arc<Mutex<...>>>` 全局共享需：(a) 设计并发安全 API（多线程渲染）；(b) 处理 atlas 满淘汰时的全局锁竞争；(c) 字体 face 数据的 `Arc<[u8]>` 共享在多线程下的 swash scaler 上下文复用。需重新审视 `#![forbid(unsafe_code)]` 下的并发模型，超出范围。 |
| **智能选词规则（URL/路径/IP 识别）** | 选区增强 | 当前 `select_word` 仅按字符类别（空白 / 字母数字 / 标点）边界扩展；正则识别 URL / 路径 / IP 需引入 `regex` 或 `url` crate 依赖，并设计选区优先级（先匹配 URL，否则按字符类别）。属于选区系统增强，本轮基础选区已完成。 |
| **矩形块选模式** | 选区扩展 | `SelectionRange.rectangular` 字段已就位、`buffer::selection_text` 已支持矩形提取，但鼠标交互层未接入 Alt+拖拽的矩形选区触发。属于选区交互增强，超出本轮 spec。 |
| **行级以下脏区（子行 / 列级）** | 渲染优化 | 当前 `DamageTracker` 行粒度，`Renderer::render_frame` 按行重绘。子行 / 列级脏区需重写 `DamageTracker` 为 `BTreeSet<(row, col_range)>`，并修改 `render_row` 为按 col_range 切片合成。属于性能优化，超出范围。 |

---

## 三、GUI Demo 完成状态

### slint-demo（Slint 1.6.0）

- **位置**：`/workspace/demos/slint-demo/`
- **GUI 框架**：`slint = "=1.6.0"`（精确锁定，避免 1.17.1 要求 rustc 1.92）+ `slint-build = "=1.6.0"`
- **状态**：✅ `cargo build --release` 通过，二进制 22 MB
- **交互能力**：
  - 键盘：普通字符 / 方向键 / 功能键 F1–F12 / Ctrl+字母 / Alt 前缀 ESC
  - 鼠标：左键拖拽选区 / 双击选词 / 三击选行 / 释放自动复制到剪贴板 / 中键粘贴
  - Resize：200ms 轮询窗口尺寸 → `EventLoop::resize` + `Renderer::resize` + 全屏重绘
  - 底部状态栏：FPS（60 帧滑动平均）+ 内存（500ms 刷新）+ scrollback 偏移
  - **滚轮**：Slint 1.6 TouchArea 不暴露 wheel 事件，故 slint-demo 不支持滚轮（已在源码注释说明，iced/egui demo 已支持）
- **关键设计**：选区状态机由 `TerminalManager::mouse_event` 内部维护，demo 仅转发事件 + Release 时读取 `selection_text` 复制到剪贴板 + 中键粘贴。避免 demo 与 manager 重复实现选区逻辑。

### iced-demo（iced 0.13）

- **位置**：`/workspace/demos/iced-demo/`
- **GUI 框架**：`iced = "0.13"` features=["image","tokio"]
- **状态**：✅ `cargo build --release` 通过，二进制 25 MB
- **交互能力**：
  - 键盘：完整 `KeyMapping` 接入（含 Named 键映射、Ctrl/Alt/Shift 修饰键）
  - 鼠标：左键选区 + 释放复制 + 中键粘贴 + **滚轮 scrollback**
  - Resize：`window::resize` 事件 → `EventLoop::resize` + `Renderer::resize`
  - 底部状态栏：FPS + 内存 + 滚动偏移
- **关键设计**：基于 `Subscription::tick`（16ms）+ `time::every`（500ms）驱动，符合 iced 的 Elm 架构。

### egui-demo（eframe 0.29）

- **位置**：`/workspace/demos/egui-demo/`
- **GUI 框架**：`eframe = "0.29"` + `egui = "0.29"`
- **状态**：✅ `cargo build --release` 通过，二进制 22 MB
- **交互能力**：
  - 键盘：`egui::InputState` 的 key_down / key_pressed → `KeyMapping`
  - 鼠标：左键选区 + 释放复制 + 中键粘贴 + **滚轮 scrollback**
  - Resize：`egui::RawInput` 的 screen_rect → `EventLoop::resize`
  - 底部状态栏：egui::Label + `egui::Context::request_repaint`
- **关键设计**：基于 `TextureHandle` 上传 RGBA + `request_repaint_after` 驱动 60fps。

---

## 四、依赖隔离验证

### 工作区（rust-xterm 自身）

| 检查项 | 结果 |
| :--- | :--- |
| `/workspace/Cargo.toml` members | 仅 `crates/rust-xterm-{core,renderer,host}`，**不含** demos |
| `/workspace/Cargo.lock` 总依赖数 | 318 |
| `/workspace/Cargo.lock` 含 slint/iced/egui/eframe/wgpu/i-slint/glutin/gl | **0**（grep exit=1） |
| `cargo build` 是否编译任何 GUI 框架 | **否** |
| `swash` / `fontdb` / `lru` 版本 | `=0.1.15` / `=0.16.0` / `=0.12.0`（Cargo.lock sticky） |

### Demo（各自独立 workspace 根）

| Demo | 独立 Cargo.lock | GUI 依赖 | swash / fontdb / lru 版本 |
| :--- | :--- | :--- | :--- |
| slint-demo | ✅（175 KB） | slint 1.6.0 | swash 0.1.19 / fontdb 0.16.2 / lru 0.12.5 |
| iced-demo | ✅（152 KB） | iced 0.13 | swash 0.1.19 / fontdb 0.16.2 / lru 0.12.5 |
| egui-demo | ✅（144 KB） | eframe 0.29 + egui 0.29 + glutin | swash 0.1.19 / fontdb 0.16.2 / lru 0.12.5 |

### 隔离机制说明

1. **每个 demo 都是自身工作区根**：`Cargo.toml` 末尾的 `[workspace]` 表（空 members）使 demo 成为独立工作区根，阻止 cargo 向上查找 `/workspace` 的 workspace。
2. **rust-xterm-renderer 的版本约束放宽**：`swash = "0.1.15"` / `fontdb = "0.16"` / `lru = "0.12"` 使用 caret（允许 patch 升级），而非 workspace 的 `=` 精确锁定。原因：GUI 框架传递依赖需新 patch 版本（iced 需 swash ^0.1.17，slint 1.6 需 fontdb ^0.16.1）。
3. **workspace 自身不受影响**：`/workspace/Cargo.lock` 仍 sticky 锁定 `swash 0.1.15` / `fontdb 0.16.0` / `lru 0.12.0`，workspace 构建使用的版本与 demo 完全独立。
4. **slint 精确锁定 `=1.6.0`**：避免 caret 默认拉取最新 1.x（如 1.17.1 要求 rustc 1.92，超出 MSRV 1.88）。
5. **rust-xterm 三 crate 通过 path 引用**：demo 不发布、不依赖 crates.io 版本，直接引用本地路径，无版本漂移。

---

## 五、交付清单

### 代码

| 路径 | 类型 | 说明 |
| :--- | :--- | :--- |
| `crates/rust-xterm-core/src/events.rs` | 修改 | 新增 `FocusReport`、`CwdChange`、`SelectionReady` 事件变体 |
| `crates/rust-xterm-core/src/wezterm_core.rs` | 修改 | `set_focused` / `is_focus_reporting_enabled` / `scroll_region` |
| `crates/rust-xterm-core/src/manager.rs` | 修改 | 暴露公共 API；构造时注册 OSC 7 handler；`handle_selection_mouse` 内部状态机 |
| `crates/rust-xterm-core/src/input.rs` | 新增 | `KeyInput` 枚举 + `KeyMapping::encode_key` |
| `crates/rust-xterm-core/src/mouse.rs` | 修改 | `MouseState` 选区状态机字段 |
| `crates/rust-xterm-core/src/selection.rs` | 新增 | `SelectionRange` 结构 |
| `crates/rust-xterm-core/src/buffer.rs` | 修改 | `select_word` / `select_line` / `selection_text` |
| `crates/rust-xterm-core/tests/input_mapping.rs` | 新增 | 键盘映射完整测试 |
| `crates/rust-xterm-renderer/Cargo.toml` | 修改 | swash/fontdb/lru 改 caret 约束（允许 demo patch 升级） |
| `crates/rust-xterm-host/src/event_loop.rs` | 修改 | `send_key` 便利方法 |
| `demos/slint-demo/` | 新增 | Slint 1.6 demo（独立 workspace） |
| `demos/iced-demo/` | 新增 | iced 0.13 demo（独立 workspace） |
| `demos/egui-demo/` | 新增 | eframe 0.29 demo（独立 workspace） |

### 文档

| 路径 | 说明 |
| :--- | :--- |
| `.trae/specs/extend-features-and-gui-demos/spec.md` | 规格 |
| `.trae/specs/extend-features-and-gui-demos/tasks.md` | 任务清单（Task 1–13 全部完成） |
| `.trae/specs/extend-features-and-gui-demos/checklist.md` | 检查清单 |
| `PROGRESS.md`（本文档） | 完成度对照 |

### 测试

- `rust-xterm-core` 单元测试：**62 项全绿**（含本轮新增的焦点报告 / OSC 7 / 滚动区域 / 选区 / 双击选词 / 三击选行 / 双宽字符 共 11 项）
- `rust-xterm-core/tests/input_mapping.rs`：键盘映射集成测试覆盖全部 `KeyInput` 变体的 app_cursor on/off + Ctrl/Alt 组合
- 三个 demo：`cargo build --release` 全部通过，二进制可执行

---

## 六、特性对照总结

| 类别 | 本轮已完成 | 超出范围 | 总计 |
| :--- | :---: | :---: | :---: |
| spec.md Task 1–8 | 8 | 0 | 8 |
| FEATURES.md 未实现项 | 0 | 9 | 9 |
| GUI Demo | 3 | 0 | 3 |
| **合计** | **11** | **9** | **20** |

**结论**：本轮 spec 范围内 100% 交付，依赖隔离严格保证，三个 GUI demo 全部可构建可运行，rust-xterm 核心 workspace 不受任何 GUI 框架污染。
