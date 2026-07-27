# russh-slint-demo 品质对齐 wt / 现代化终端 Spec

## Why

用户在 Win11 实测发现 russh-slint-demo 存在 12 项功能/性能缺陷，与目标（高性能、低内存、默认参数下界面观感对齐 Windows Terminal、终端内功能对齐现代化终端）严重不符。诊断显示问题分布在三层：

- **库层默认行为缺陷**（rust-xterm-renderer 的 DPI/cell/font_size 不响应系统 scale_factor、rust-xterm-core 的 `encode_key` 对具名键丢弃修饰键）
- **demo 层硬编码错误**（resize 路径 `window().size()` 误当 logical 又乘一次 scale、`app_cursor` 永远传 false、滚轮事件不转发给 manager、每帧全屏 `clone_from_slice + set_terminal_image`、状态栏文本无 dirty 检查、`connected` 在 channel 关闭后永真）
- **缺失功能**（滚动条、Ctrl+Shift+V 粘贴、IME、应用关闭后清屏）

本 spec 聚焦**最小而精准的修复**，不重构架构、不引入新依赖、不改 rust-xterm 公共 API 签名（仅新增字段/方法/枚举变体）。

## What Changes

### A. 库层：DPI 与字体尺寸自适应（`crates/rust-xterm-renderer/src/renderer.rs`、`canvas.rs`、`font_tree.rs`）

- **RendererConfig 新增 `scale_factor: f32` 字段**（默认 1.0）：在 `Renderer::new` 与 `Renderer::resize` 时把 `metrics.cell_width / cell_height / baseline / font_size` 按 `scale_factor` 放大，并同步 `font_tree.set_shape_size(scaled_font_size)`
- **`Renderer::resize` 重置字形缓存**：当 cell 尺寸变化时清空 `glyph_cache` 与 `run_cache`，避免老字形被合成到新尺寸 cell 上
- **`RenderMetrics::dpi` 字段语义化为 "logical DPI baseline"**（默认 96.0），仅作 scale_factor 计算基准，不再作为渲染参数

### B. demo 层：resize 单位修正（`demos/russh-slint-demo/src/resize.rs`、`main.rs`）

- **修正 `window().size()` 单位**：Slint 1.6 `Window::size()` 返回 `PhysicalSize`，应直接用其 `width/height` 作物理像素；`STATUS_BAR_H` 改为按物理像素计算（`22px * scale`）
- **cols/rows 计算改用物理像素 + scaled cell**：`new_cols = phys_w / (CELL_W * scale)`，`new_rows = (phys_h - STATUS_BAR_H * scale) / (CELL_H * scale)`
- **renderer.resize 传物理像素**，与 `manager.resize` 的新 cols/rows 配套，避免双重 scale
- **canvas 物理尺寸精确对齐 `new_cols × scaled_cell_w × scale`**，消除 letterbox 黑边

### C. demo 层：渲染节流与脏区驱动上传（`src/render.rs`）

- **只在脏区或光标变化时上传**：把 `set_terminal_image` 移入 `if frame.dirty_spans 非空 || frame.cursor.visible 变化` 守卫内；保留 `clone_from_slice` 但仅在有像素变更时调用
- **接收 `RenderResult::dirty_rects`**：把 `render_frame` 返回值接住，作为是否上传的判定条件（`dirty_rects.is_empty() && !cursor_changed` 时跳过整张上传）
- **`connected` 关闭后停止 poll_frame / drain_output / set_terminal_image**：在 `SshEvent::Closed` 分支将 `connected` 置 false
- **状态栏 fps/mem/scroll 文本 dirty 检查**：仅在数值变化时 set（mem 已 500ms 节流，fps 取整数位，scroll 取 offset + channel_alive）

### D. demo 层：app_cursor 模式跟踪（`src/input.rs` + `crates/rust-xterm-core/src/manager.rs`）

- **Manager 暴露 `app_cursor()` 查询接口**：从 WezTerm 内部状态读取 DECSET 1 状态（`rust-xterm-core/src/wezterm_core.rs` 已有 `is_bracketed_paste_enabled`，类似新增 `app_cursor_mode`）
- **input.rs 调用 `encode_key` 时传入 `manager.app_cursor()`**：替换硬编码的 `false`

### E. 库层：具名键修饰符编码（`crates/rust-xterm-core/src/input.rs`）

- **`encode_key` 对具名键检查 `mods`**：Shift/Ctrl/Alt + 方向键/Home/End/F1-F12 输出 modifyOtherKeys 编码（`\x1b[1;{mods}A` 等）
- **`KeyMods` 新增 `shift` 字段**（如已有则复用），统一 modifier 传递

### F. demo 层：滚轮事件转发（`src/main.rs` + `src/mouse.rs` + `ui/app.slint`）

- **`scroll-event` 回调中转发给 `manager.mouse_event`**：当 `manager.is_mouse_grabbed()` 时构造 `MouseAction::WheelUp/Down` 调用 `mouse_event`，让 WezTerm 编码鼠标报告发给 SSH channel
- **非鼠标跟踪模式下保持现有 scrollback 行为**：滚轮更新 `scroll_offset`

### G. demo 层：滚动条组件（`ui/app.slint`）

- **右侧新增垂直 ScrollBar**：显示当前 scroll_offset / max_scrollback 比例，可拖拽
- **绑定 `scroll-offset` 与 `scroll-max` 属性**：demo 在 poll_frame 后更新 scroll-max，scroll-offset 双向绑定

### H. demo 层：Ctrl+Shift+V 粘贴 + 应用关闭清屏（`src/input.rs` + `src/render.rs`）

- **input.rs 新增 Ctrl+Shift+V 分支**：从 `arboard::Clipboard` 读取文本，按 bracketed paste 模式发送
- **`SshEvent::Closed` 分支调用 `renderer.clear()` 并 `set_terminal_image` 一次**：避免画面停滞，半透明遮罩下显示"SSH 已关闭"提示

### I. demo 层：默认参数对齐 wt（`src/main.rs` 常量 + `ui/app.slint`）

- **cell 尺寸对齐 wt 默认 12pt 字体**：`CELL_W=9, CELL_H=19, font_size=18.0`（wt 默认 12pt @ 96dpi = 16px 高度 cell + 18ppem）
- **窗口默认尺寸**：`preferred-width: 1000px; preferred-height: 640px`，`min-width: 400px; min-height: 300px`
- **`image-fit: fill` 改为 `image-fit: contain`**（已是 contain，保留）；但在 canvas 物理尺寸精确对齐后 contain 不会留黑边

## Impact

- Affected specs: `fix-slint-demo-render-bugs`（slint-demo 同类问题，部分修复可同步应用）、`extend-features-and-gui-demos`、`fix-feature-gaps`
- Affected code:
  - `crates/rust-xterm-renderer/src/renderer.rs`（RendererConfig.scale_factor、resize 重置 glyph_cache）
  - `crates/rust-xterm-renderer/src/canvas.rs`（无 API 变化，resize 行为不变）
  - `crates/rust-xterm-renderer/src/font_tree.rs`（set_shape_size 在 resize 时调用）
  - `crates/rust-xterm-core/src/input.rs`（encode_key 检查 mods、modifyOtherKeys 编码）
  - `crates/rust-xterm-core/src/manager.rs`（暴露 app_cursor_mode 查询）
  - `crates/rust-xterm-core/src/wezterm_core.rs`（实现 app_cursor 读取）
  - `demos/russh-slint-demo/src/{main,resize,render,input,mouse}.rs`、`ui/app.slint`
- **不影响**：rust-xterm 公共 API 签名（仅新增字段/方法/枚举变体）、SSH 协议层（russh 调用不变）、demo 工作区独立性

## ADDED Requirements

### Requirement: RendererConfig 支持 scale_factor
`RendererConfig` SHALL 新增 `scale_factor: f32` 字段（默认 1.0），在 `Renderer::new` 与 `Renderer::resize` 时将 `RenderMetrics` 的 `cell_width / cell_height / baseline / font_size` 按 `scale_factor` 放大后用于光栅化与画布坐标计算，使 HiDPI 显示器下字符物理分辨率与显示矩形物理像素 1:1。

#### Scenario: HiDPI 显示器无模糊
- **WHEN** 在 scale_factor=2.0 的显示器上启动 demo
- **THEN** renderer 内部 cell 物理尺寸 = 8×2 × 16×2 = 16×32，font_size = 32.0 ppem，swash 光栅化字形填满 HiDPI 物理像素，视觉无锯齿/模糊

### Requirement: 渲染脏区驱动上传
demo 的渲染 tick SHALL 仅在 `manager.poll_frame` 返回 `Some(frame)` 且 `frame.dirty_spans` 非空，或 `frame.cursor.visible` 变化时，才执行 `SharedPixelBuffer::clone_from_slice + set_terminal_image`；其他 tick 跳过像素上传。

#### Scenario: 空闲时无像素上传
- **WHEN** SSH 空闲、无 dirty_spans、光标闪烁未到期
- **THEN** tick 中不调用 `set_terminal_image`，Slint 不重绘 Image 元素

#### Scenario: 光标闪烁时仍上传
- **WHEN** 光标闪烁到期，`frame.cursor.visible` 翻转
- **THEN** 执行一次像素上传（光标像素已变化）

### Requirement: Manager 暴露 app_cursor_mode 查询
`TerminalManager` SHALL 提供 `pub fn app_cursor_mode(&self) -> bool`，返回当前 DECSET 1 状态（应用光标模式）。

#### Scenario: vim 启动后方向键走 SS3
- **WHEN** 远端 vim 启用 DECSET 1，用户按方向键
- **THEN** `encode_key` 收到 `app_cursor=true`，输出 `\x1bOA` 等 SS3 序列

### Requirement: 具名键修饰符编码
`KeyMapping::encode_key` SHALL 对 ArrowUp/Down/Left/Right/Home/End/F1-F12 等具名键检查 `mods` 参数，当 `ctrl/alt/shift` 任一为真时输出 modifyOtherKeys 编码（如 Ctrl+Right → `\x1b[1;5C`，Shift+Tab → `\x1b[Z`）。

#### Scenario: Ctrl+Right 输出 modifyOtherKeys
- **WHEN** 用户按 Ctrl+Right
- **THEN** 编码为 `\x1b[1;5C`，远端 vim/htop 正确识别为单词跳转

### Requirement: 滚轮事件转发给 manager
当远端程序启用鼠标跟踪（`manager.is_mouse_grabbed()`）时，滚轮事件 SHALL 构造 `MouseAction::WheelUp/Down` 调用 `manager.mouse_event`，由 WezTerm 编码鼠标报告并发送给 SSH channel。

#### Scenario: htop 中滚轮可滚动
- **WHEN** htop 启用 DECSET 1000，用户滚动滚轮
- **THEN** 鼠标滚轮报告被发送给远端，htop 屏幕滚动

### Requirement: 右侧滚动条组件
demo 的 UI SHALL 在终端显示区右侧提供垂直 ScrollBar，显示当前 `scroll_offset / max_scrollback` 比例，可拖拽调整 scroll_offset。

#### Scenario: 滚动条显示 scrollback 位置
- **WHEN** 用户用滚轮滚动 scrollback
- **THEN** 滚动条 thumb 位置反映 `scroll_offset / max_scrollback` 比例

### Requirement: Ctrl+Shift+V 粘贴
demo SHALL 在用户按下 Ctrl+Shift+V 时从系统剪贴板读取文本，按 bracketed paste 模式发送给 SSH channel。

#### Scenario: 粘贴剪贴板内容
- **WHEN** 用户按 Ctrl+Shift+V
- **THEN** 从 arboard 读取文本，若 `is_bracketed_paste_enabled()` 则发送 `\x1b[200~` + 文本 + `\x1b[201~`，否则直接发送文本

### Requirement: SSH 关闭后清屏
demo 在收到 `SshEvent::Closed` 时 SHALL 调用 `renderer.clear()` 清空画布并立即 `set_terminal_image` 一次，避免画面停滞；同时 `connected` 置 false 停止后续 poll_frame / drain_output / set_terminal_image。

#### Scenario: htop 退出后无残留画面
- **WHEN** SSH channel 关闭（如远端 shell 退出）
- **THEN** 终端画布清空为背景色，半透明遮罩显示"SSH 连接已关闭"，FPS 恢复到空闲水平

### Requirement: 默认参数对齐 wt
demo 默认 `CELL_W=9, CELL_H=19, font_size=18.0`（对齐 wt 12pt 默认）；窗口 `preferred-width: 1000px, preferred-height: 640px`，`min-width: 400px, min-height: 300px`。

#### Scenario: 默认字体大小与 wt 一致
- **WHEN** 在 96 DPI 显示器上启动 demo
- **THEN** 字符视觉高度与 Windows Terminal 默认 12pt 一致（约 16px 字高 + 3px 行距）

## MODIFIED Requirements

### Requirement: resize 路径单位一致
resize.rs SHALL 使用 `app.window().size()` 返回的 PhysicalSize 直接作为物理像素；cols/rows 计算用物理像素除以 scaled cell 尺寸；renderer.resize 传物理像素。消除"logical × scale = 双倍放大"bug。

### Requirement: 状态栏文本 dirty 检查
demo 的 fps/mem/scroll 文本 SHALL 仅在数值变化时调用 `set_*_text`；不再每帧 `format!` + `set`。

### Requirement: app_cursor 编码
input.rs 调用 `encode_key` 时 SHALL 传入 `manager.app_cursor_mode()`，不再硬编码 `false`。

### Requirement: 渲染区域精确填充
canvas 物理尺寸 SHALL 等于 `new_cols × scaled_cell_w`，使 `image-fit: contain` 在显示矩形与 canvas 比例匹配时不留 letterbox，鼠标坐标转换无需扣偏移。

## REMOVED Requirements

无移除。
