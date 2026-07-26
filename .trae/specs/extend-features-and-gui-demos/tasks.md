# Tasks

- [x] Task 1: 焦点报告（DECSET 1004）
  - [x] SubTask 1.1: `events.rs` 新增 `TerminalEvent::FocusReport(bool)` 变体
  - [x] SubTask 1.2: `wezterm_core.rs` 增加 `set_focused(bool)` 方法，DECSET 1004 启用时生成 `\x1b[I`/`\x1b[O` 写入 drain_output 缓冲；增加 `is_focus_reporting_enabled() -> bool` 查询
  - [x] SubTask 1.3: `manager.rs` 暴露 `set_focused` / `is_focus_reporting_enabled` 公共方法
  - [x] SubTask 1.4: 新增测试 `test_focus_report_enabled`：写 DECSET 1004 后调 set_focused(true)，断言 drain_output 含 `\x1b[I`；未启用时返回空
  - [x] SubTask 1.5: 新增测试 `test_focus_report_disabled`：未写 DECSET 1004 时 set_focused 不产生输出

- [x] Task 2: OSC 7 CWD 事件
  - [x] SubTask 2.1: `events.rs` 新增 `TerminalEvent::CwdChange(PathBuf)` 变体
  - [x] SubTask 2.2: `manager.rs` 在构造时注册 OSC 7 handler，解析 `file://[host]/[path]`（用 url crate 或手写解析，避免新依赖则手写），提取 path 部分，emit `CwdChange`
  - [x] SubTask 2.3: 新增测试 `test_osc7_cwd_event`：写 `\x1b]7;file://localhost/home/user\x07`，断言订阅回调收到 `PathBuf::from("/home/user")`
  - [x] SubTask 2.4: 新增测试 `test_osc7_malformed_ignored`：写格式错误的 OSC 7，断言不 emit 事件不 panic

- [x] Task 3: 滚动区域查询 API
  - [x] SubTask 3.1: `wezterm_core.rs` 增加 `scroll_region() -> Option<(usize, usize)>` 委托 WezTerm 查询 DECSTBM（1-based，None=全屏）
  - [x] SubTask 3.2: `manager.rs` 暴露公共 `scroll_region()`
  - [x] SubTask 3.3: 新增测试 `test_scroll_region_query`：写 `\x1b[5;20r` 后查询返回 `Some((5,20))`；重置 `\x1b[r` 后返回 `None`

- [x] Task 4: 键盘映射核心层
  - [x] SubTask 4.1: 新增 `crates/rust-xterm-core/src/input.rs`，定义 `KeyInput` 枚举（Char/ArrowUp/ArrowDown/ArrowLeft/ArrowRight/Home/End/Insert/Delete/PageUp/PageDown/F1-F12/Enter/Backspace/Tab/Esc）和 `KeyMods`（已有则复用）
  - [x] SubTask 4.2: 实现 `KeyMapping::encode_key(key, mods, app_cursor) -> Vec<u8>`：方向键 app_cursor=true 用 SS3（`\x1bOA` 等），否则 CSI（`\x1b[A` 等）；Ctrl+字母 = 字母 mod 0x1f；Alt+字符前缀 `\x1b`；F1-F12 用 CSI/SS3 序列；Enter=`\r`、Backspace=`\x7f`、Tab=`\t`、Esc=`\x1b`
  - [x] SubTask 4.3: `lib.rs` 加 `pub mod input;` + 重导出 `KeyInput`、`KeyMods`、`KeyMapping`
  - [x] SubTask 4.4: 新增 `crates/rust-xterm-core/tests/input_mapping.rs` 测试：覆盖所有 KeyInput 变体的 app_cursor on/off、Ctrl/Alt 组合，断言编码字节正确
  - [x] SubTask 4.5: `rust-xterm-host/src/event_loop.rs` 增加 `send_key(key, mods)` 便利方法，内部调 `encode_key` + `send_input`

- [x] Task 5: 选区系统模型与 API
  - [x] SubTask 5.1: `events.rs` 新增 `SelectionReady` 事件变体（无 payload，表示释放可复制）
  - [x] SubTask 5.2: 新增 `SelectionRange { start: (usize, usize), end: (usize, usize), rectangular: bool }` 结构（放在 `manager.rs` 或新建 `selection.rs`），`#[derive(Clone, Copy, Debug, PartialEq, Eq)]`
  - [x] SubTask 5.3: `manager.rs` 增加 `selection: Option<SelectionRange>` 字段、`set_selection(Option<SelectionRange>)` / `selection() -> Option<SelectionRange>` 公共方法
  - [x] SubTask 5.4: `buffer.rs` 增加 `selection_text(range: SelectionRange) -> String`：线性选区跨行用 `\n` 连接，矩形选区每行按列截取
  - [x] SubTask 5.5: `manager.rs` 暴露 `selection_text() -> Option<String>` 委托 buffer
  - [x] SubTask 5.6: 新增测试 `test_selection_linear_text`：设置跨 3 行选区，断言 selection_text 返回 3 行文本 `\n` 连接
  - [x] SubTask 5.7: 新增测试 `test_selection_rectangular_text`：设置矩形选区，断言每行按列截取

- [x] Task 6: 鼠标选区交互
  - [x] SubTask 6.1: `mouse.rs` 增加选区状态机字段（`selecting: bool`、`select_start: (usize, usize)`、`click_count: u32`、`last_click_time: Instant`、`last_click_pos: (usize, usize)`）
  - [x] SubTask 6.2: `mouse_event` 在非 `is_mouse_grabbed` 且左键 Press 时：设 select_start、clearing 旧选区、click_count 按时间窗（500ms）和位置递增
  - [x] SubTask 6.3: 双击（click_count=2）→ 调用 `select_word(pos)` 智能选词（按字符类别边界扩展：空白/标点/字母数字三类），设选区
  - [x] SubTask 6.4: 三击（click_count=3）→ `select_line(pos)` 选整行
  - [x] SubTask 6.5: 拖拽（Drag 且 selecting=true）→ 扩展选区终点，emit `SelectionChange`
  - [x] SubTask 6.6: 释放（Release 且 selecting=true）→ emit `SelectionReady`，selecting=false
  - [x] SubTask 6.7: `buffer.rs` 增加 `select_word(pos) -> SelectionRange` 和 `select_line(pos) -> SelectionRange` 辅助函数
  - [x] SubTask 6.8: 新增测试 `test_mouse_drag_selection`：模拟按下/拖拽/释放，断言 selection 正确、SelectionReady 被 emit
  - [x] SubTask 6.9: 新增测试 `test_double_click_select_word`：在 "hello world" 的 "hello" 中央双击，断言选区覆盖 "hello"
  - [x] SubTask 6.10: 新增测试 `test_triple_click_select_line`：三击断言选区为整行

- [x] Task 7: 双宽度字符测试补全
  - [x] SubTask 7.1: 新增测试 `test_wide_char_advance`：写入 CJK 字符断言 cell.width=2、advance 正确
  - [x] SubTask 7.2: 新增测试 `test_wide_char_cursor_movement`：光标在宽字符上移动断言跳 2 列
  - [x] SubTask 7.3: 新增测试 `test_wide_char_overwrite`：宽字符后写普通字符断言宽字符被正确覆盖占 2 格

- [x] Task 8: 验证核心层全部通过（Rust 1.88）
  - [x] SubTask 8.1: `cargo +1.88.0 build --all-targets` 通过
  - [x] SubTask 8.2: `cargo +1.88.0 test --all-targets` 通过
  - [x] SubTask 8.3: `cargo +1.88.0 clippy --all-targets -- -D warnings` 通过
  - [x] SubTask 8.4: `cargo +1.88.0 fmt --all -- --check` 通过

- [ ] Task 9: slint-demo（独立 package）
  - [ ] SubTask 9.1: 创建 `/workspace/demos/slint-demo/Cargo.toml`（独立 [package]，[dependencies] 含 `slint = "1.6"` + path 引用 rust-xterm 三 crate，**不**用 .workspace = true）
  - [ ] SubTask 9.2: 实现 `src/main.rs`：Slint 窗口 + Image 组件显示 RGBA、Timer 驱动 EventLoop::tick、键盘/鼠标/滚轮/resize 事件、底部状态栏 FPS+内存
  - [ ] SubTask 9.3: 验证 `cd demos/slint-demo && cargo build` 通过，demo 能启动交互
  - [ ] SubTask 9.4: 验证 `/workspace/Cargo.lock` 不含 slint 传递依赖

- [ ] Task 10: iced-demo（独立 package）
  - [ ] SubTask 10.1: 创建 `/workspace/demos/iced-demo/Cargo.toml`（独立，`iced = "0.13"` features=["image","tokio"]）
  - [ ] SubTask 10.2: 实现 `src/main.rs`：iced Application + canvas/image widget + Subscription::tick + 键盘/鼠标/resize + 底部 text 状态栏
  - [ ] SubTask 10.3: 验证 `cd demos/iced-demo && cargo build` 通过
  - [ ] SubTask 10.4: 验证 `/workspace/Cargo.lock` 不含 iced 传递依赖

- [ ] Task 11: egui-demo（独立 package）
  - [ ] SubTask 11.1: 创建 `/workspace/demos/egui-demo/Cargo.toml`（独立，`eframe = "0.29"`）
  - [ ] SubTask 11.2: 实现 `src/main.rs`：eframe App + TextureHandle 上传 RGBA + request_repaint + 键盘/鼠标/resize + egui::Label 状态栏
  - [ ] SubTask 11.3: 验证 `cd demos/egui-demo && cargo build` 通过
  - [ ] SubTask 11.4: 验证 `/workspace/Cargo.lock` 不含 egui 传递依赖

- [ ] Task 12: 依赖隔离硬验证
  - [ ] SubTask 12.1: `cd /workspace && cargo build` 成功且不编译任何 GUI 框架
  - [ ] SubTask 12.2: `grep -E "name = \"(slint|iced|egui|eframe|wgpu|i-slint|glutin|gl)\"" /workspace/Cargo.lock` 无任何输出
  - [ ] SubTask 12.3: 三个 demo 各自 `Cargo.lock` 含 GUI 依赖但 `/workspace/Cargo.lock` 不含

- [ ] Task 13: PROGRESS.md 完成度对照文档
  - [ ] SubTask 13.1: 新建 `/workspace/PROGRESS.md`，列出本 spec 所有特性，标注"已完成"（打勾）/ "超出范围（理由）"
  - [ ] SubTask 13.2: 列出 FEATURES.md 中其他未实现特性（连字/彩色Emoji/Unicode CJK Ext/图像协议/IME/全局字形缓存）并标注"超出范围（理由：需大规模重写/并发安全设计）"
  - [ ] SubTask 13.3: 列出 3 个 demo 的完成状态和依赖隔离验证结果

# Task Dependencies
- [Task 5] 依赖 [Task 5 SubTask 5.1 events] — 同 task 内有序
- [Task 6] 依赖 [Task 5] — 选区交互需要选区模型
- [Task 9/10/11] 依赖 [Task 4 + Task 5 + Task 6] — demo 需要 KeyMapping + 选区 API
- [Task 9/10/11] 之间相互独立，可并行
- [Task 12] 依赖 [Task 9 + 10 + 11]
- [Task 13] 依赖 [Task 8 + 12]
