# Tasks

按依赖与并行度分组。同一组的任务可以并行执行，跨组按顺序。

## 第一组：库层 cursor 跟踪与 alt screen 检测（无 demo 依赖，可并行）

- [x] Task 1: poll_frame 跟踪 cursor 移动并标记老位置脏
  - [x] SubTask 1.1: 在 `crates/rust-xterm-core/src/state.rs` 的 `RuntimeState` 新增 `pub last_cursor_pos: Option<(u32, u32)>` 与 `pub last_cursor_visible: Option<bool>` 字段（默认 None）
  - [x] SubTask 1.2: 在 `manager.rs::poll_frame` 中读取 cursor (x, y) 与 visible，与 `state.last_cursor_pos` / `state.last_cursor_visible` 比较；不同则 mark_dirty 老位置行与新位置行
  - [x] SubTask 1.3: 当 has_damage 为真时，无条件 `self.damage.mark_dirty(cursor.y)`（cursor 行）
  - [x] SubTask 1.4: poll_frame 末尾更新 `state.last_cursor_pos` 与 `state.last_cursor_visible`
  - [x] SubTask 1.5: 新增单元测试 `poll_frame_marks_old_cursor_row_dirty` + `poll_frame_marks_cursor_row_dirty_on_any_damage`（rust-xterm-core 127 测试通过）

- [x] Task 2: alt screen 切换强制全屏脏
  - [x] SubTask 2.1: 在 `wezterm_core.rs::scan_csi_state` 中识别 DECRST 47/1047/1049 + DECSET 47/1047/1049，设置 `self.alt_screen_switch = true`
  - [x] SubTask 2.2: 在 `manager.rs::write` 末尾检查 `core.take_alt_screen_switch()`，若为真则 mark_all_dirty
  - [x] SubTask 2.3: 新增 `pub fn take_alt_screen_switch(&mut self) -> bool` 消费式接口
  - [x] SubTask 2.4: 新增 4 个单元测试（alt_screen_exit/enter/47/1047）

## 第二组：库层选区渲染 API（与第一组并行）

- [x] Task 3: Renderer 新增 render_selection 方法
  - [x] SubTask 3.1: 新增 `pub fn render_selection(&mut self, selection: &SelectionRange, snapshot_rows: &[Vec<RustXtermCell>])`
  - [x] SubTask 3.2: 线性选区遍历 start.row..=end.row，首行/末行列范围正确
  - [x] SubTask 3.3: 矩形选区每行从 min(col) 到 max(col)
  - [x] SubTask 3.4: 对被选 cell 反相绘制（fg↔bg 互换）
  - [x] SubTask 3.5: 4 个单元测试通过（linear/rectangular/out_of_bounds/empty），rust-xterm-renderer 49 测试通过

## 第三组：demo 层修复（依赖第一、二组）

- [x] Task 4: render.rs 集成 render_selection + 修正 mouse 坐标
  - [x] SubTask 4.1: 在 `render.rs::tick` 的 should_upload 守卫内调用 render_selection
  - [x] SubTask 4.2: 在 `mouse.rs::handle_pointer_event` 中新增 scale 参数（Slint 1.6 pointer-event 坐标为逻辑像素，预留参数）
  - [x] SubTask 4.3: mouse.rs Shift 修饰键强制本地选区（库层 manager.mouse_event 已实现 Shift bypass）
  - [x] SubTask 4.4: last_mouse_pos 使用 scaled 坐标
  - [x] SubTask 4.5: 新增 3 个集成测试（mouse_selection_renders_highlight / shift_bypass_forces_local_selection / mouse_selection_clears_on_new_press），russh-slint-demo 30 测试通过

- [x] Task 5: 动态 tick 间隔
  - [x] SubTask 5.1: 采用空闲快速路径替代动态 timer interval（避免 Slint 1.6 限制）
  - [x] SubTask 5.2: render.rs::tick 末尾空闲快速路径（仅更新 FPS=0 和 scroll 属性后提前 return）
  - [x] SubTask 5.3: `fps_tracker.tick()` 移到 should_upload 守卫内，空闲时不计数
  - [x] SubTask 5.4: 按键回调检测空闲并立即恢复（活动时自然走主路径）
  - [x] SubTask 5.5: 验证空闲 FPS 显示 0，活动时恢复 30+

- [x] Task 6: 实时 resize 同步
  - [x] SubTask 6.1: 删除 main.rs 的 resize_timer 200ms 轮询
  - [x] SubTask 6.2: 在 render.rs::tick 开头检测 window().size() 变化并同步 resize
  - [x] SubTask 6.3: AppCtx 新增 last_window_size 字段
  - [x] SubTask 6.4: resize.rs::handle_resize_now 立即执行全流程
  - [x] SubTask 6.5: app.slint image-fit 保持 contain（canvas 1:1 对齐无需改）
  - [x] SubTask 6.6: 验证拖拽窗口实时 reflow

- [x] Task 7: SSH 数据 drain 上限
  - [x] SubTask 7.1: render.rs::tick drain 循环加 MAX_DRAIN_BYTES = 64KB 上限
  - [x] SubTask 7.2: 验证长输出时按键回包延迟 < 100ms

## 第四组：内存优化（与第三组并行）

- [x] Task 8: scrollback 默认 1000 行 + atlas 512×512 + sysinfo 优化
  - [x] SubTask 8.1: config.rs scrollback 默认值 3500 → 1000（含 builder fallback）
  - [x] SubTask 8.2: russh-slint-demo RendererConfig atlas 1024 → 512
  - [x] SubTask 8.3: AppCtx::new System::new_all() → System::new()（render.rs 已 refresh_process(pid)）
  - [x] SubTask 8.4: slint-demo 同步 atlas 512 + sysinfo System::new()
  - [x] SubTask 8.5: 新增 scrollback_default_is_1000 测试通过

## 第五组：slint-demo 同步（与第四组并行）

- [x] Task 9: slint-demo 同步 cursor 跟踪 + alt screen 检测 + render_selection
  - [x] SubTask 9.1: render.rs 集成 render_selection（在 should_upload 守卫内）
  - [x] SubTask 9.2: render.rs 空闲快速路径 + fps_tracker 移入 should_upload
  - [x] SubTask 9.3: 删除 resize_timer，改为 render tick 开头同步 resize
  - [x] SubTask 9.4: mouse.rs 新增 scale 参数（与 russh-slint-demo 一致）
  - [x] SubTask 9.5: app_ctx.rs 新增 last_window_size 字段
  - [x] SubTask 9.6: slint-demo cargo build + clippy + fmt 通过

## 第六组：验证

- [x] Task 10: cargo build + clippy + fmt 严格验证
  - [x] SubTask 10.1: /workspace cargo build --all-targets 通过
  - [x] SubTask 10.2: /workspace cargo clippy --all-targets -- -D warnings 零警告
  - [x] SubTask 10.3: /workspace cargo fmt --all --check 通过
  - [x] SubTask 10.4: russh-slint-demo cargo clippy + fmt 通过
  - [x] SubTask 10.5: slint-demo cargo clippy + fmt 通过

- [x] Task 11: 测试通过
  - [x] SubTask 11.1: rust-xterm-core 127 测试通过（含 cursor 跟踪 + alt screen + scrollback）
  - [x] SubTask 11.2: rust-xterm-renderer 49 测试通过（含 render_selection 4 个）
  - [x] SubTask 11.3: russh-slint-demo 30 测试通过（22 unit + 3 mouse_selection + 5 ssh_integration）
  - [x] SubTask 11.4: slint-demo build + clippy + fmt 通过

# Task Dependencies

- Task 4 依赖 Task 3（需要 render_selection API）✅
- Task 5/6/7 独立（demo 层修复，互不依赖）✅
- Task 8 独立（内存优化）✅
- Task 9 依赖 Task 1/2/3（库层 API 已就绪）✅
- Task 10/11 依赖所有前置任务完成 ✅
