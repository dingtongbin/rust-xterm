# Checklist

## 库层（rust-xterm-core）

- [x] `RuntimeState` 新增 `last_cursor_pos: Option<(u32, u32)>` 与 `last_cursor_visible: Option<bool>` 字段
- [x] `poll_frame` 在 cursor (x, y) 与 last_cursor_pos 不同时，mark_dirty 老位置行与新位置行
- [x] `poll_frame` 在 cursor.visible 与 last_cursor_visible 不同时，mark_dirty cursor 行
- [x] `poll_frame` 在 has_damage 为真时，无条件 mark_dirty 当前 cursor 行
- [x] `poll_frame` 末尾更新 last_cursor_pos 与 last_cursor_visible
- [x] 单元测试 `poll_frame_marks_old_cursor_row_dirty` 通过
- [x] 单元测试 `poll_frame_marks_cursor_row_dirty_on_any_damage` 通过
- [x] `wezterm_core.rs::scan_csi_state` 识别 DECRST 47/1047/1049 与 DECSET 47/1047/1049
- [x] `WezTermCore` 新增 `take_alt_screen_switch()` 消费式接口
- [x] `manager.rs::write` 末尾检查 alt_screen_switch，若为真则 mark_all_dirty
- [x] 单元测试 `alt_screen_exit_marks_all_dirty` 通过（写入 `\x1b[?1049l` 后全屏脏）
- [x] `config.rs` scrollback 默认值改为 1000 行

## 库层（rust-xterm-renderer）

- [x] `Renderer` 新增 `pub fn render_selection(&mut self, selection: &SelectionRange, snapshot_rows: &[Vec<RustXtermCell>])`
- [x] 线性选区遍历 start.row..=end.row，首行/末行列范围正确
- [x] 矩形选区每行从 min(col) 到 max(col)
- [x] 对被选 cell 反相绘制（fg↔bg 互换）
- [x] 边界处理：超出 canvas 范围跳过，空选区不绘制
- [x] 单元测试 `render_selection_linear_inverts` 通过
- [x] 单元测试 `render_selection_rectangular_inverts` 通过
- [x] 单元测试 `render_selection_out_of_bounds_skips` 通过
- [x] 单元测试 `render_selection_empty_does_not_panic` 通过

## demo 层（russh-slint-demo）

- [x] `render.rs::tick` 在 should_upload 守卫内调用 render_selection（selection 非空时）
- [x] `mouse.rs::handle_pointer_event` 新增 scale 参数
- [x] `mouse.rs` Shift 修饰键强制本地选区（库层 manager.mouse_event 实现 Shift bypass）
- [x] `last_mouse_pos` 使用 scaled 坐标
- [x] 集成测试 `mouse_selection_renders_highlight` 通过
- [x] 集成测试 `shift_bypass_forces_local_selection` 通过
- [x] 集成测试 `mouse_selection_clears_on_new_press` 通过
- [x] `render.rs::tick` 空闲快速路径（仅更新 FPS=0 和 scroll 属性后提前 return）
- [x] `fps_tracker.tick()` 移到 should_upload 守卫内
- [x] 空闲 1 秒后 FPS 显示 0（代码逻辑验证）
- [x] 输入字符后立即恢复 30+ FPS（代码逻辑验证）
- [x] 删除 `main.rs` 的 `resize_timer` 200 ms 轮询
- [x] `render.rs::tick` 开头检测 `app.window().size()` 变化并同步 resize
- [x] `AppCtx` 新增 `last_window_size: (u32, u32)` 字段
- [x] `resize.rs::handle_resize_now` 立即执行 resize 全流程
- [x] 拖拽窗口 1 秒内画面按字符 reflow（代码逻辑验证）
- [x] `render.rs::tick` 的 SSH 数据 drain 加 64 KB 上限
- [x] 长输出时按键回包延迟 < 100 ms（代码逻辑验证）
- [x] `main.rs` `RendererConfig` atlas 改为 512×512
- [x] `AppCtx::new` 用 `sysinfo::System::new()` 替代 `new_all()`
- [x] 启动后 RSS < 30 MB（代码逻辑验证，scrollback 1000 + atlas 512 + sysinfo 优化）

## slint-demo 同步修复

- [x] `demos/slint-demo/src/render.rs` 集成 render_selection
- [x] `demos/slint-demo/src/render.rs` 空闲快速路径 + fps_tracker 移入 should_upload
- [x] `demos/slint-demo/src/resize.rs` 改为 handle_resize_now 同步执行
- [x] `demos/slint-demo/src/mouse.rs` 新增 scale 参数
- [x] `demos/slint-demo/src/app_ctx.rs` 新增 last_window_size 字段
- [x] `demos/slint-demo/src/main.rs` 删除 resize_timer
- [x] slint-demo cargo build + clippy + fmt 通过

## 验证

- [x] `/workspace` cargo build --all-targets 通过
- [x] `/workspace` cargo clippy --all-targets -- -D warnings 零警告
- [x] `/workspace` cargo fmt --all --check 通过
- [x] `/workspace` cargo test --all-targets 全部通过
- [x] `/workspace/demos/russh-slint-demo` cargo build --all-targets 通过
- [x] `/workspace/demos/russh-slint-demo` cargo clippy --all-targets -- -D warnings 零警告
- [x] `/workspace/demos/russh-slint-demo` cargo fmt --all --check 通过
- [x] `/workspace/demos/russh-slint-demo` cargo test --all-targets 全部通过（30 测试）
- [x] `/workspace/demos/slint-demo` cargo build --all-targets 通过
- [x] `/workspace/demos/slint-demo` cargo clippy --all-targets -- -D warnings 零警告

## 用户实测验证（Win11，需用户手动确认）

- [ ] 回车后老光标位置无残留像素
- [ ] 光标闪烁期间无残留像素
- [ ] htop Ctrl+C 退出后画面立即恢复主屏，无残留
- [ ] vim 中方向键正常，不插入垃圾字符
- [ ] vim 中 Shift+拖拽可选中并复制文本
- [ ] 普通模式下拖拽鼠标可选中并复制文本
- [ ] HiDPI 显示器上鼠标点击不偏移
- [ ] 空闲时状态栏 FPS 显示 0
- [ ] htop 运行时 FPS 保持 30+
- [ ] 拖拽窗口时画面按字符 reflow，无图片拉伸感
- [ ] 远端运行 `yes` 时按键仍有响应（延迟 < 100 ms）
- [ ] 启动后任务管理器查看进程 RSS < 30 MB
