# Tasks

按依赖与并行度分组。同一组的任务可以并行执行，跨组按顺序。

## 第一组：库层 API 扩展（无 demo 依赖，可并行）

- [x] Task 1: RendererConfig 新增 scale_factor 字段 + resize 时重置字形缓存
  - [x] SubTask 1.1: 在 `crates/rust-xterm-renderer/src/renderer.rs` 的 `RendererConfig` 结构体新增 `pub scale_factor: f32`（默认 1.0）；新增内部 `scaled_metrics` 缓存
  - [x] SubTask 1.2: `Renderer::new` 中根据 `scale_factor` 计算 scaled cell_w/cell_h/baseline/font_size，调用 `font_tree.set_shape_size(scaled_font_size)` 并缓存 scaled_metrics
  - [x] SubTask 1.3: `Renderer::resize` 在 scale_factor 变化或 cell 尺寸变化时清空 `atlas.clear_dynamic()`（同时清 glyph_cache 与 run_cache）+ global_atlas 同步
  - [x] SubTask 1.4: 暴露 `pub fn set_scale_factor(&mut self, scale: f32)` 供 demo 在 resize 时动态调整
  - [x] SubTask 1.5: 新增单元测试 `scale_factor_doubles_metrics` + `set_scale_factor_updates_metrics` + `resize_keeps_metrics_when_scale_unchanged`（3 个测试通过）

- [x] Task 2: rust-xterm-core encode_key 具名键修饰符编码
  - [x] SubTask 2.1: ArrowUp/Down/Left/Right/Home/End 在 mods 非空时输出 `\x1b[1;{modifier}<final>`，modifier = 1 + shift*1 + alt*2 + ctrl*4
  - [x] SubTask 2.2: F1-F4 在 mods 非空时改用 CSI 编码 `\x1b[1;{modifier}P` 等
  - [x] SubTask 2.3: F5-F12 在 mods 非空时改用 `\x1b[15;{modifier}~` 等
  - [x] SubTask 2.4: Tab 在 shift 为真时输出 `\x1b[Z`（Shift+Tab backtab）
  - [x] SubTask 2.5: 6 个新测试通过（test_ctrl_arrow / test_shift_tab / test_ctrl_function_key / test_alt_home / test_shift_arrow / test_plain_arrow_unchanged），rust-xterm-core 总计 120 passed

- [x] Task 3: TerminalManager 暴露 app_cursor_mode 查询
  - [x] SubTask 3.1: `wezterm_core.rs` 新增 `app_cursor: bool` 字段，在 `scan_csi_state` 中拦截 DECSET 1 / DECRST 1 序列维护
  - [x] SubTask 3.2: `manager.rs` 增加 `pub fn app_cursor_mode(&self) -> bool` 委托
  - [x] SubTask 3.3: 2 个新测试通过（app_cursor_mode_default_false / app_cursor_mode_set_by_decset）

## 第二组：demo 层修复（依赖第一组）

- [x] Task 4: resize.rs 修正单位错位
  - [x] SubTask 4.1: `app.window().size()` 直接作物理像素，删除二次 `* scale`
  - [x] SubTask 4.2: `STATUS_BAR_H` 乘 scale 转物理像素；`scaled_cell_w/h = CELL_W/H * scale`；cols/rows 用物理像素 / scaled_cell
  - [x] SubTask 4.3: canvas 精确对齐 `new_cols × scaled_cell_w`，消除 letterbox
  - [x] SubTask 4.4: 调用 `ctx.renderer.set_scale_factor(scale)` 同步 scale_factor
  - [x] SubTask 4.5: slint-demo/src/resize.rs 同步修复

- [x] Task 5+8.4: render.rs 节流 + 状态栏 dirty + scroll 属性更新
  - [x] SubTask 5.1: 接收 `render_frame` 返回的 RenderResult，记录 dirty_rects.is_empty() 与 cursor 变化
  - [x] SubTask 5.2: set_terminal_image 移入 `should_upload = has_dirty || cursor_changed || scrollback_redrawn || force_upload` 守卫
  - [x] SubTask 5.3: SshEvent::Closed 分支：`ctx.connected = false`、`renderer.clear()`、`force_upload = true`
  - [x] SubTask 5.4: fps/mem/scroll 文本 dirty 检查（缓存 last_fps_text / last_mem_text / last_scroll_text）
  - [x] SubTask 5.5: slint-demo/src/render.rs 同步修复
  - [x] SubTask 8.4: tick 末尾更新 app.set_scroll_max / set_scroll_offset（带 dirty 检查）

- [x] Task 6: input.rs app_cursor 跟踪 + Ctrl+Shift+V 粘贴 + 控制编码
  - [x] SubTask 6.1: 调用 `encode_key` 时传入 `ctx.borrow().manager.app_cursor_mode()`
  - [x] SubTask 6.2: Ctrl+Shift+V 从 arboard 读取并按 bracketed paste 模式发送（函数签名新增 clipboard 参数）
  - [x] SubTask 6.3: `(0x40..=0x5F).contains(&c)` 范围内的 Ctrl+非字母字符按 `(c & 0x1f)` 控制编码（C-@、C-[、C-\、C-]、C-^、C-_）

- [x] Task 7: 鼠标滚轮事件转发给 manager
  - [x] SubTask 7.1: `on_scroll_cb` 检查 `is_mouse_grabbed()`，构造 WheelUp/Down 调用 `manager.mouse_event`
  - [x] SubTask 7.2: 非鼠标跟踪模式下保持现有 scrollback 行为
  - [x] SubTask 7.3: 通过 `bridge.send_input(manager.drain_output())` 回送鼠标报告字节
  - [x] AppCtx 新增 `last_mouse_pos: Option<(usize, usize)>` 字段，mouse.rs 中 pointer-event 时更新

## 第三组：UI 与默认参数（与第二组并行）

- [x] Task 8+9: app.slint 滚动条 + 默认参数 + main.rs 常量
  - [x] SubTask 8.1: app.slint 右侧新增手动垂直 ScrollBar（12px 宽，track + thumb + TouchArea）
  - [x] SubTask 8.2: 新增 `in-out property <int> scroll-offset` 与 `scroll-max`，自定义 `callback scroll-to(float)` 替代 Slint 1.6 不支持的 `on_<prop>_changed`
  - [x] SubTask 8.3: 窗口 `preferred-width: 1000px; preferred-height: 640px; min-width: 400px; min-height: 300px`
  - [x] SubTask 8.5: `app.on_scroll_to` 回调中 `try_borrow_mut` 更新 ctx.scroll_offset（防重入）
  - [x] SubTask 9.1: russh-slint-demo/src/main.rs `CELL_W=9, CELL_H=19, font_size=18.0`
  - [x] SubTask 9.2: slint-demo/src/main.rs 同步修改 CELL_W/CELL_H/font_size
  - [x] SubTask 9.3: `RendererConfig.metrics.baseline = CELL_H - 3 = 16`
  - [x] SubTask 9.4: `INITIAL_COLS=100, INITIAL_ROWS=30`

## 第四组：验证

- [x] Task 10: cargo build + clippy + fmt 严格验证
  - [x] SubTask 10.1: 根工作区 + 两个 demo 工作区 `cargo build --all-targets` 通过
  - [x] SubTask 10.2: 三工作区 `cargo clippy --all-targets -- -D warnings` 零警告
  - [x] SubTask 10.3: 三工作区 `cargo fmt --all --check` 通过

- [x] Task 11: 测试通过
  - [x] SubTask 11.1: rust-xterm-renderer 45 单元 + rust-xterm-core 120 单元 + 15 集成 + 7 GBK + 6 idle + 4 yes + rust-xterm-host 3 + api_lock 9 + smoke_pty 4 = 全部通过
  - [x] SubTask 11.2: russh-slint-demo 22 单元 + 5 ssh_integration 全部通过

# Task Dependencies

- Task 4 依赖 Task 1（需要 `set_scale_factor` API）✅
- Task 5 部分独立（脏区检查不依赖库层），但全量验证依赖 Task 1/2/3 ✅
- Task 6 依赖 Task 3（需要 `app_cursor_mode` API）✅
- Task 7 独立（manager.mouse_event 已存在）✅
- Task 8/9 独立（UI 改动）✅
- Task 10/11 依赖所有前置任务完成 ✅
