# slint-demo 渲染/行为 Bug 修复 Spec

## Why

`/workspace/demos/slint-demo/src/main.rs`（564 行）在用户 Win11 实机上暴露 8 个问题：颜色不均匀、回车多行、字体模糊、resize 拉伸、光标不闪烁、光标形状为方块（应为竖线）、状态栏显示系统内存（应为进程 RSS）、scrollback 滚轮无响应。诊断显示问题分属**库层默认行为**（cursor blink 默认关闭、`CursorShape::Default` 走 Block 分支）、**demo 硬编码错误**（resize 用 logical 像素、font_size 与 cell_height 不对齐、内存 API 用错、resize 路径光标形状硬编码、scrollback 早返回过于严格）、以及**TTY echo 行为**（Enter 多行不是 bug）。本 spec 聚焦**最小修复**，不重构架构、不引入新依赖、不改 rust-xterm 公共 API 签名（除 EventLoop 新增方法）。

## What Changes

### A. Demo 层修复（`demos/slint-demo/src/main.rs` + `ui/app.slint`）

- **resize 物理像素**：resize timer 改用 `app.window().scale_factor()` 将 logical 像素换算为 physical 像素后再传给 `renderer.resize()`；cols/rows 计算仍用 logical（因 cell_w/cell_h 是逻辑单位）——修复 HiDPI 模糊/拉伸
- **app.slint image-fit**：`image-fit: fill` 改为 `image-fit: contain`，避免非等比窗口下拉伸变形
- **font_size 与 cell_height 对齐**：`font_size: 14.0` + `cell_height: 16` 改为 `font_size: 16.0` + `cell_height: 16`，消除 2px 垂直留白导致的模糊
- **resize 路径光标形状**：删除 main.rs:483 的 `shape: CursorShape::Default` 硬编码，改用 `frame.cursor.shape`（与 tick 路径一致）
- **光标闪烁启用**：在 EventLoop 初始化后调用 `event_loop.set_cursor_blinking(true)`（manager.rs:889 已暴露 API，demo 从未调用）
- **内存显示改为进程级**：`sys.refresh_memory()` + `used_memory()` 改为 `sys.refresh_process(pid)` + `process(pid).map(|p| p.memory())`，显示 demo 自身 RSS 而非系统全局
- **scrollback 早返回放宽**：删除 main.rs:322-324 的 `if max == 0 { return; }` 早返回，让 `snapshot_scrolled` 内部 clamp 处理空内容（已存在于 wezterm_core.rs:325-326），避免"无内容时滚轮完全无响应"的体感
- **默认主题色对齐**：将 main.rs:141-142 的 `default_fg: WHITE, default_bg: BLACK` 改为从 `manager.palette()` 读取 WezTerm 调色板的 `default_fg`/`default_bg`，与 ANSI 色彩一致，消除"同红色不同深浅"的视觉不一致

### B. 库层修复（`crates/`）

- **CursorShape::Default 语义**：在 `renderer.rs:1069-1070` 的 `match cursor.shape` 中，将 `CursorShape::Default` 从 `Block | Default` 合并分支拆出，单独映射为 `Bar`（与 Windows Terminal / xterm 默认一致），`Block` 保持原块状行为
- **EventLoop 暴露 set_cursor_blinking**：在 `crates/rust-xterm-host/src/event_loop.rs` 增加 `pub fn set_cursor_blinking(&mut self, enabled: bool)` 委托 manager，使 demo 无需直接持 manager 可变引用

### C. Enter 换行（非 bug，仅文档化）

- **不修复**：`input.rs:121` 的 `KeyInput::Enter => b"\r"` 是标准终端编码；TTY 在 cooked 模式下 echo `\r\n` 是 shell 行为，非 rust-xterm 责任。在 spec 注释中说明此为预期行为

### D. 模块化（可选，非行为变更）

main.rs 564 行对当前功能集偏长但不失控。可拆分模块（行为不变）：
- `fps.rs`：`FpsTracker`（main.rs:66-93，28 行）
- `input.rs`：`map_named_key` + `is_nav_key` + key-pressed 闭包主体（main.rs:174-238 + 498-543，~110 行）
- `mouse.rs`：pointer-event 闭包主体（main.rs:240-312，73 行）
- `render.rs`：tick 闭包主体 + pixel upload（main.rs:344-437，94 行）
- `resize.rs`：resize timer 闭包主体（main.rs:439-487，49 行）
- `app_ctx.rs`：`AppCtx` 结构 + 构造（main.rs:50-172，~120 行）

拆分后 main.rs 降至 ~80 行，各模块 80-120 行。

## Impact

- Affected specs: `extend-features-and-gui-demos`（demo 行为修正）、`fix-feature-gaps`（光标闪烁 API 已存在，本 spec 仅接线）
- Affected code:
  - `demos/slint-demo/src/main.rs`（resize、font_size、cursor shape、blinking、memory、scrollback、默认色）
  - `demos/slint-demo/ui/app.slint`（image-fit）
  - `crates/rust-xterm-renderer/src/renderer.rs`（CursorShape::Default 语义拆分）
  - `crates/rust-xterm-host/src/event_loop.rs`（set_cursor_blinking 委托）
- **不影响**：rust-xterm 公共 API 签名（除 EventLoop 新增方法）、wezterm_term 依赖、demo 依赖隔离纪律（`/workspace/Cargo.lock` 不引入新 GUI 依赖）

## ADDED Requirements

### Requirement: HiDPI resize 物理像素
Demo SHALL 在 resize timer 中将 `app.window().size()` 返回的 logical 像素乘以 `scale_factor()` 转为 physical 像素，再传给 `renderer.resize()`；cols/rows 计算仍用 logical 像素。

#### Scenario: HiDPI 显示器 resize 无模糊
- **WHEN** 在 scale_factor=2.0 的显示器上 resize 窗口
- **THEN** renderer canvas 物理像素尺寸 = logical * 2，Slint Image 1:1 显示无拉伸

### Requirement: 光标闪烁默认启用
Demo SHALL 在启动后调用 `event_loop.set_cursor_blinking(true)`，使光标按 WezTerm 内置周期翻转。

#### Scenario: 光标闪烁可见
- **WHEN** demo 启动后无输入 1 秒
- **THEN** 光标在 visible/invisible 之间切换至少一次

### Requirement: 进程级内存显示
Demo SHALL 显示自身进程 RSS（KB），而非系统全局 used_memory。

#### Scenario: 内存显示为进程 RSS
- **WHEN** demo 运行中查看状态栏
- **THEN** 显示数值与 `ps -o rss= -p <pid>`（Linux）或任务管理器（Win11）一致，误差 < 1MB

### Requirement: CursorShape::Default 映射为 Bar
Renderer SHALL 将 `CursorShape::Default` 渲染为竖线（Bar），与 Windows Terminal / xterm 默认一致；`CursorShape::Block` 保持原块状行为。

#### Scenario: Default 光标为竖线
- **WHEN** 渲染 `CursorMeta { shape: CursorShape::Default, .. }`
- **THEN** 画 2px 宽竖线而非整 cell 块

### Requirement: EventLoop 暴露光标闪烁
`EventLoop` SHALL 提供 `pub fn set_cursor_blinking(&mut self, enabled: bool)` 委托 `TerminalManager::set_cursor_blinking`。

#### Scenario: demo 通过 EventLoop 启用闪烁
- **WHEN** demo 调用 `event_loop.set_cursor_blinking(true)`
- **THEN** 后续 `tick()` 返回的 `FrameUpdate.cursor.visible` 按内置周期翻转

## MODIFIED Requirements

### Requirement: slint-demo resize 路径光标形状
resize timer 中的 `render_cursor` SHALL 使用 `frame.cursor.shape`（来自 WezTerm），不再硬编码 `CursorShape::Default`。

### Requirement: scrollback 滚轮响应
scroll_cb SHALL NOT 在 `max_scrollback == 0` 时早返回；scroll_offset 允许增长，由 `snapshot_scrolled` 内部 clamp 到实际可用行数。

### Requirement: 默认前景/背景色来源
Demo 的 `RendererConfig.default_fg`/`default_bg` SHALL 从 `manager.palette()` 读取，而非硬编码 `Color::WHITE`/`Color::BLACK`。

### Requirement: 字体大小与单元格高度对齐
Demo 的 `RendererConfig.metrics.font_size` SHALL 等于 `cell_height`，消除字形在 cell 内的垂直留白导致的模糊。

## REMOVED Requirements

无移除。
