# Tasks

- [x] Task 1: 修复 resize 物理像素（HiDPI 模糊/拉伸根因）
  - [x] SubTask 1.1: 在 resize timer 闭包中读取 `app.window().scale_factor()`，将 `w`/`h` 乘以 scale_factor 得到 physical 像素
  - [x] SubTask 1.2: `renderer.resize(phys_w, phys_h)` 用 physical 像素；cols/rows 计算仍用 logical（因 cell_w/cell_h 是逻辑单位）
  - [x] SubTask 1.3: 验证 `SharedPixelBuffer::clone_from_slice(buffer, phys_w, phys_h)` 与 canvas 物理尺寸一致
  - [x] SubTask 1.4: 验证 `cargo check` 通过

- [x] Task 2: 修复 image-fit 拉伸
  - [x] SubTask 2.1: `app.slint:51` 的 `image-fit: fill` 改为 `image-fit: contain`
  - [x] SubTask 2.2: 验证非等比窗口下终端不变形

- [x] Task 3: 对齐 font_size 与 cell_height
  - [x] SubTask 3.1: main.rs:135 的 `font_size: 14.0` 改为 `16.0`（与 cell_height=16 对齐）
  - [x] SubTask 3.2: 验证 ASCII 字符垂直居中无 2px 留白模糊

- [x] Task 4: 修复 resize 路径光标形状硬编码
  - [x] SubTask 4.1: 删除 main.rs:483 的 `shape: CursorShape::Default`，改用 `frame.cursor.shape`
  - [x] SubTask 4.2: 验证 resize 后光标形状与 tick 路径一致

- [x] Task 5: 启用光标闪烁（库 + demo）
  - [x] SubTask 5.1: 在 `crates/rust-xterm-host/src/event_loop.rs` 增加 `pub fn set_cursor_blinking(&mut self, enabled: bool)` 委托 `self.manager.set_cursor_blinking(enabled)`
  - [x] SubTask 5.2: 在 demo main.rs 的 EventLoop 初始化后调用 `event_loop.set_cursor_blinking(true)`
  - [x] SubTask 5.3: 验证光标按内置周期翻转
  - [x] SubTask 5.4: 验证 `cargo build -p rust-xterm-host` 通过

- [x] Task 6: 修复内存显示为进程 RSS
  - [x] SubTask 6.1: main.rs:406-410 的 `refresh_memory()` + `used_memory()` 改为 `refresh_process(pid)` + `process(pid).map(|p| p.memory())`
  - [x] SubTask 6.2: 在 AppCtx 初始化时记录 `pid = sysinfo::get_current_pid().unwrap()`
  - [x] SubTask 6.3: 验证状态栏显示与 `ps -o rss= -p <pid>` 一致

- [x] Task 7: 放宽 scrollback 早返回
  - [x] SubTask 7.1: 删除 main.rs:322-324 的 `if max == 0 { return; }`
  - [x] SubTask 7.2: 保留 scroll_offset 的 `.min(max)` clamp（main.rs:334），让 snapshot_scrolled 内部 clamp 处理 max=0 情况
  - [x] SubTask 7.3: 验证无 scrollback 内容时滚轮不 panic，有内容时正常滚动

- [x] Task 8: 默认色从 palette 读取
  - [x] SubTask 8.1: 在 demo 初始化后，应用 `WindowsTerminalTheme::default()`（Campbell），从 `theme.foreground`/`theme.background` 转 `Color::rgba` 设给 `RendererConfig.default_fg`/`default_bg` 与 `manager.apply_theme`
  - [x] SubTask 8.2: 验证 ANSI 默认色文本与 WezTerm 调色板一致

- [x] Task 9: CursorShape::Default 映射为 Bar
  - [x] SubTask 9.1: renderer.rs 的 `match cursor.shape` 中，将 `CursorShape::Default` 从 `Block | Default` 分支拆出，与 `Bar` 合并分支
  - [x] SubTask 9.2: 新增测试 `test_default_cursor_renders_bar_not_block`：在干净画布渲染 Default，断言前两列像素为 (255,255,255,255)、第三列与末列保持背景 (0,0,0,0)
  - [x] SubTask 9.3: 检查并更新现有 cursor 测试（无回归，42 测试全过）
  - [x] SubTask 9.4: 验证 `cargo test -p rust-xterm-renderer` 通过

- [x] Task 10: 文档化 Enter 换行行为（非 bug）
  - [x] SubTask 10.1: 在 spec.md "C. Enter 换行（非 bug，仅文档化）" 已注明
  - [x] SubTask 10.2: 验证用户交互上 Enter 仍按预期工作（shell 收到 `\r`，TTY echo `\r\n` 是预期）

- [x] Task 11（可选，非阻塞）: 模块化拆分 main.rs
  - [x] SubTask 11.1: 抽出 `fps.rs`（FpsTracker）— 31 行
  - [x] SubTask 11.2: 抽出 `input.rs`（map_named_key + is_nav_key + key-pressed 闭包主体）— 123 行
  - [x] SubTask 11.3: 抽出 `mouse.rs`（pointer-event 闭包主体）— 76 行
  - [x] SubTask 11.4: 抽出 `render.rs`（tick 闭包主体 + pixel upload）— 94 行
  - [x] SubTask 11.5: 抽出 `resize.rs`（resize timer 闭包主体）— 46 行
  - [x] SubTask 11.6: 抽出 `app_ctx.rs`（AppCtx 结构 + 构造）— 47 行
  - [x] SubTask 11.7: 验证行为与拆分前完全一致（cargo build/clippy/fmt 全绿）

# Task Dependencies
- [Task 5] 的 SubTask 5.1 须先于 SubTask 5.2（demo 依赖 EventLoop 新 API）
- [Task 9] 独立于 demo 修复，可并行
- [Task 11] 依赖 [Task 1-8] 完成（先修 bug 再拆模块，避免合并冲突）
- [Task 1] 与 [Task 2] 相关但独立（物理像素 + image-fit）
- [Task 5] 与 [Task 9] 均涉及光标，但分别管"闪烁"和"形状"，互不阻塞
