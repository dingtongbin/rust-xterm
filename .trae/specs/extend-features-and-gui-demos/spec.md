# rust-xterm 特性扩展与 GUI Demo 集成 Spec

## Why

`FEATURES.md` 对照显示 18 项缺口（选区系统 5 项全空、焦点报告/OSC 7/键盘映射/IME 等仍未实现）。其中"选区系统缺失"直接导致 GUI demo 无法实现最基本的"鼠标拖拽选中文本→复制"交互，是 rust-xterm 走向可用终端产品的最大阻碍。同时项目尚无任何 GUI 集成 demo 验证 API 可用性。本 spec 聚焦于**可验证、最小实现**的特性完善 + 3 个独立 GUI demo（slint/iced/egui）+ 完成度对照文档，并**严格保证 demo 依赖不污染 rust-xterm 的库依赖锁**。

## What Changes

### A. 核心层特性完善（rust-xterm-core / renderer）

- **焦点报告（DECSET 1004）**：`TerminalEvent` 新增 `FocusReport(bool)` 变体；`TerminalManager` 暴露 `set_focused(bool)` 触发核心生成 `\x1b[I`/`\x1b[O` 转义序列输出到 `drain_output()`；暴露 `is_focus_reporting_enabled()` 查询 DECSET 1004 状态
- **OSC 7 CWD 暴露**：`TerminalManager` 注册 OSC 7 handler，解析 `file://host/path` 并 emit `TerminalEvent::CwdChange(PathBuf)`
- **滚动区域查询 API**：`TerminalManager` 暴露 `scroll_region() -> Option<(usize, usize)>` 委托 WezTerm 查询当前 DECSTBM 顶/底行
- **键盘映射核心层**：新增 `rust_xterm_core::input::KeyMapping` 模块，提供 `encode_key(key: KeyInput, mods: KeyMods, app_cursor: bool) -> Vec<u8>`，覆盖功能键/方向键/编辑键/F1-F12/Alt+字符/Ctrl+字符的 CSI/SS3 编码，配合 `EventLoop::send_input` 使用
- **选区系统基础版**（核心难点）：
  - 新增 `SelectionRange { start: (usize, usize), end: (usize, usize), rectangular: bool }` 模型
  - `TerminalManager` 增加 `set_selection(Option<SelectionRange>)` / `selection() -> Option<SelectionRange>` / `selection_text() -> Option<String>`（矩形选区用 `Buffer::line_text` 按列截取）
  - `mouse_event` 在非抓取模式下：左键按下 → 设选区起点；拖拽 → 扩展选区终点并 emit `SelectionChange`；释放 → emit `SelectionReady`（宿主据此调 `selection_text()` 复制）
  - 双击 → 智能选词（以 `cell.text` 的字符类别边界扩展，参照 WT 规则：空白/标点/字母数字三类边界）
  - 三击 → 选整行
  - `SelectionChange` 事件从不 emit 的死代码状态改为真实 emit
- **双宽度字符测试补全**：新增 `test_wide_char_advance`、`test_wide_char_cursor_movement` 测试验证 `cell.width` + `is_wide()` 在渲染与光标移动中的一致性

### B. GUI Demo（独立 packages，不入 workspace members）

**依赖隔离纪律（硬约束，违反则不做 demo）**：
- 3 个 demo 放在 `/workspace/demos/{slint-demo,iced-demo,egui-demo}/`，**不加入** `/workspace/Cargo.toml` 的 `members`
- 每个 demo 有独立 `Cargo.toml` + 独立 `Cargo.lock`
- GUI 框架依赖（slint/iced/egui）**只**写在 demo 自己的 `[dependencies]`，**不**用 `.workspace = true`，**不**出现在 `/workspace/Cargo.toml` 的 `[workspace.dependencies]`
- 引用 rust-xterm 库 crate 用 `path = "../../crates/rust-xterm-{core,renderer,host}"`
- 验证标准：`cd /workspace && cargo build` 不触发任何 GUI 框架编译；`/workspace/Cargo.lock` 不含 slint/iced/egui/wgpu 等任何 GUI 传递依赖

**每个 demo 的功能要求（三者一致）**：
1. 启动 GUI 窗口，spawn 操作系统默认 shell（Unix: `$SHELL` 或 `/bin/bash`；Windows: `cmd.exe` 或 PowerShell）
2. 创建 `EventLoop` + `Renderer`，将终端像素完整绘制到窗口
3. 键盘输入：普通字符、方向键、功能键、Ctrl/Alt 组合 → `encode_key` → `EventLoop::send_input`
4. 鼠标交互：左键拖拽选区（调核心层选区 API）→ 释放自动复制到系统剪贴板；滚轮滚动 scrollback；中键粘贴
5. 窗口 resize → 计算 cols/rows → `EventLoop::resize` + `Renderer::resize`
6. 底部状态栏实时显示：当前 FPS（滑动平均）、占用内存（`sysinfo` 或各框架自带 API）
7. 交互全部功能：窗口关闭、ESC、Ctrl+C/Ctrl+D/Ctrl+L/Ctrl+Z 等终端信号正常工作
8. 不做多标签页、菜单栏、设置面板等 GUI 扩展功能

**框架特定说明**：
- **slint-demo**：用 Slint 的 `Image` 组件显示 RGBA 像素缓冲，`Timer` 驱动 `EventLoop::tick`，状态栏用 Slint 组件
- **iced-demo**：用 iced 的 `canvas` 或 `image` widget，`Subscription::tick` 驱动，状态栏用 `text` widget
- **egui-demo**：用 `egui::TextureHandle` 上传 RGBA，`request_repaint` 驱动，状态栏用 `egui::Label`

### C. 完成度对照文档

- 新建 `/workspace/PROGRESS.md`：列出本 spec 涉及的所有特性，标注"已完成/未完成（原因）/超出范围（理由）"
- 不修改 `FEATURES.md`（保持其作为"计划基准"的语义；PROGRESS.md 作为"实际进度快照"）

## Impact

- Affected specs: `fix-feature-gaps`（已完成的特性不重复）、`upgrade-rust-msrv-to-185`（MSRV 已是 1.88，本 spec 在此基础上）
- Affected code:
  - `crates/rust-xterm-core/src/events.rs`（新增 FocusReport / CwdChange / SelectionReady 事件变体）
  - `crates/rust-xterm-core/src/manager.rs`（焦点报告、OSC 7 注册、选区 API、scroll_region 查询）
  - `crates/rust-xterm-core/src/wezterm_core.rs`（set_focused、is_focus_reporting_enabled、scroll_region 委托）
  - `crates/rust-xterm-core/src/mouse.rs`（选区拖拽状态机、双击/三击）
  - `crates/rust-xterm-core/src/buffer.rs`（selection_text 提取，矩形选区）
  - 新增 `crates/rust-xterm-core/src/input.rs`（KeyMapping 模块）
  - `crates/rust-xterm-core/src/lib.rs`（pub mod input）
  - 新增 `/workspace/demos/slint-demo/`、`/workspace/demos/iced-demo/`、`/workspace/demos/egui-demo/`（各含 Cargo.toml、Cargo.lock、src/main.rs）
  - 新增 `/workspace/PROGRESS.md`
- **不影响**：`Cargo.toml` workspace members、`[workspace.dependencies]`、`/workspace/Cargo.lock`（demo 完全隔离）

## ADDED Requirements

### Requirement: 焦点报告
系统 SHALL 在 `set_focused(true/false)` 被调用且 DECSET 1004 已启用时，生成 `\x1b[I`/`\x1b[O` 转义序列到 `drain_output()`。

#### Scenario: 窗口获焦触发焦点报告
- **WHEN** 宿主调用 `mgr.set_focused(true)` 且已写 DECSET 1004
- **THEN** `mgr.drain_output()` 返回包含 `\x1b[I` 的字节

### Requirement: OSC 7 CWD 事件
系统 SHALL 注册 OSC 7 handler，解析 `file://host/path` 格式并 emit `TerminalEvent::CwdChange(PathBuf)`。

#### Scenario: 写入 OSC 7 序列
- **WHEN** 写入 `\x1b]7;file://localhost/home/user\x07`
- **THEN** 订阅 `TerminalEvent::CwdChange` 的回调收到 `PathBuf::from("/home/user")`

### Requirement: 滚动区域查询
系统 SHALL 暴露 `scroll_region() -> Option<(usize, usize)>` 返回当前 DECSTBM 设置的顶/底行（1-based，None 表示全屏）。

#### Scenario: 设置 DECSTBM 后查询
- **WHEN** 写入 `\x1b[5;20r`（顶=5，底=20）
- **THEN** `scroll_region()` 返回 `Some((5, 20))`

### Requirement: 键盘映射核心层
系统 SHALL 提供 `rust_xterm_core::input::KeyMapping::encode_key(key, mods, app_cursor) -> Vec<u8>`，覆盖方向键（app_cursor 模式下用 SS3）、F1-F12、Home/End/Insert/Delete/PageUp/PageDown、Ctrl+字母、Alt+字母。

#### Scenario: 方向键 app_cursor 模式
- **WHEN** `encode_key(KeyInput::ArrowUp, KeyMods::NONE, app_cursor=true)`
- **THEN** 返回 `b"\x1bOA"`（SS3 编码）

#### Scenario: Ctrl+C
- **WHEN** `encode_key(KeyInput::Char('c'), KeyMods::CTRL, false)`
- **THEN** 返回 `b"\x03"`（ETX）

### Requirement: 选区模型
系统 SHALL 提供 `SelectionRange { start: (usize, usize), end: (usize, usize), rectangular: bool }`，坐标为 (row, col) 0-based。

#### Scenario: 设置线性选区
- **WHEN** `mgr.set_selection(Some(SelectionRange { start: (0,0), end: (2,5), rectangular: false }))`
- **THEN** `mgr.selection()` 返回该范围，`mgr.selection_text()` 返回三行文本（行间 `\n`）

### Requirement: 选区交互
系统 SHALL 在非鼠标抓取模式下处理左键：按下设起点、拖拽扩展终点、释放 emit `SelectionReady`；双击智能选词、三击选整行。

#### Scenario: 鼠标拖拽选区
- **WHEN** 鼠标左键在 (5,10) 按下，拖拽到 (5,15) 释放
- **THEN** `selection()` 返回 `start=(5,10), end=(5,15)`，`SelectionReady` 事件被 emit，`selection_text()` 返回该 5 个字符

#### Scenario: 双击选词
- **WHEN** 鼠标左键在单词 "hello" 中央双击
- **THEN** `selection()` 覆盖整个 "hello"，按字符类别边界扩展

### Requirement: Demo 依赖隔离
3 个 demo 的 GUI 框架依赖 SHALL NOT 出现在 `/workspace/Cargo.toml` 的 `[workspace.dependencies]` 或 `/workspace/Cargo.lock` 中。

#### Scenario: 工作区构建不拉 GUI 依赖
- **WHEN** 在 `/workspace` 执行 `cargo build`
- **THEN** 编译产物仅含 3 个库 crate，`/workspace/Cargo.lock` 不含 slint/iced/egui/wgpu 任何传递依赖

### Requirement: GUI Demo 完整功能
每个 demo SHALL 实现：spawn 默认 shell、完整终端绘制、键盘全交互、鼠标选区+复制、滚轮 scrollback、窗口 resize、底部 FPS+内存显示。

#### Scenario: demo 启动并交互
- **WHEN** 运行 `cargo run`（在 demo 目录）
- **THEN** 窗口打开，shell 启动，输入 `ls` 回车显示结果，鼠标拖拽选中文字自动复制到剪贴板，底部显示实时 FPS 和内存

## MODIFIED Requirements

### Requirement: SelectionChange 事件
`SelectionChange` 从死代码改为选区变化时真实 emit。新增 `SelectionReady` 在释放时 emit（一次），`SelectionChange` 在拖拽中持续 emit（多次）。

## REMOVED Requirements

### Requirement: 超出本 spec 范围的特性
**Reason**: 连字（GSUB/GPOS 整形）、彩色 Emoji（RGBA 图集）、Unicode CJK Ext B+ 全平面、图像协议（Sixel/iTerm2）、IME 预编辑、全局字形缓存——这些需大规模重写或涉及并发安全设计，超出"可验证最小实现"原则。
**Migration**: 在 `PROGRESS.md` 中明确列出并标注"超出范围（理由）"，留待后续 spec。
