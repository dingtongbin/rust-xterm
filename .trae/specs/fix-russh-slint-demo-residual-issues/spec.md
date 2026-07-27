# russh-slint-demo 残留问题修复 Spec

## Why

在 `fix-russh-slint-demo-quality` 完成后，用户在 Win11 实测仍暴露 7 项**新根因**残留问题，且这些问题并非前次修复未生效，而是前次未覆盖的代码路径：

1. **回车后光标残留**：`manager.poll_frame` 仅在 `composition.is_some()` 时标记 cursor 行为脏，**不跟踪光标移动**。`render_cursor` 只在新位置绘制，**老位置像素不擦除**。回车让光标从 (x1,y1) → (x2,y2)，老位置 (x1,y1) 不在 `dirty_spans` 中，因此老光标像素永久残留，直到下一次该行被重绘。
2. **htop Ctrl+C 退出残留**：htop 退出时远端发送 `DECRST 47/1047/1049`（退出 alt screen）。WezTerm 内部切换 buffer 后**不向外暴露"全屏脏"信号**，`changed_rows_since` 返回空，导致 demo 不知道要重绘所有行。"触发重绘清除" 暴露了根因——脏区未被标记。
3. **鼠标无法选中任何文本**：`manager.selection` 状态存在但渲染层**从未调用任何 `render_selection` 方法**，选区无视觉反馈。同时 `mouse.rs` 用 `CELL_W=9` 做像素→列换算，但 HiDPI 下物理 cell 是 `CELL_W * scale`，导致鼠标坐标错位。
4. **42 FPS 空闲、活动时帧率低**：`TICK_INTERVAL_MS=16` 固定 60 Hz tick；FPS tracker 每次 tick 自增，所以空闲也显示 42 FPS（被定时器抖动拉低）。同时每次 tick 都 borrow `ctx`、跑 fps_tracker.tick()、跑 dirty 检查，CPU 持续占用，活动时反而被争用导致掉帧。
5. **拉伸窗口像图片拉伸**：`resize_timer` 200 ms 轮询。窗口拖动时 Slint 在 canvas 未更新期间把旧 Image 按 `image-fit: contain` 拉伸，体感"图片缩放"。终端应在窗口尺寸变化的同一帧内同步 `manager.resize` + `renderer.resize`，让画面按字符 reflow 而非像素缩放。
6. **操作延迟**：所有 SSH 数据 drain、poll_frame、render、上传都在主线程 16 ms tick 内串行完成。大量 SSH 输出（如 `yes`、`find /`）会阻塞 tick，导致按键回包延迟。
7. **内存偏高，全局缓存可疑**：`atlas_width=1024, atlas_height=1024`（4 MB RGBA）+ WezTerm 内部 scrollback 默认 3500 行 × 100 列 × ~64 B/cell ≈ 22 MB。`System::new_all()` 加载全部进程信息。无 `with_global_atlas` 调用，atlas 是 per-Renderer 实例，**确实没有全局缓存**。

## What Changes

### A. 库层：cursor 移动跟踪脏区（`crates/rust-xterm-core/src/manager.rs`）

- **`poll_frame` 中跟踪 cursor 移动**：在 `state` 中保存 `last_cursor_pos: Option<(u32, u32)>`；poll_frame 时若与当前 cursor 位置不同，将**老位置行**与**新位置行**都 `mark_dirty`。
- **`poll_frame` 中跟踪 cursor 可见性变化**：当 `last_cursor_visible != cursor.visible` 时，将 cursor 行 `mark_dirty`，确保 `render_cursor` 在不可见时该行被 `render_frame` 重绘擦除。
- **`poll_frame` 始终在脏区存在时把 cursor 行标脏**：cursor 是叠加层，render_frame 重绘该行后必须再次 render_cursor 才能维持可见。否则脏区重绘会覆盖光标像素。

### B. 库层：alt screen 切换强制全屏脏（`crates/rust-xterm-core/src/wezterm_core.rs`）

- **检测 DECRST 47/1047/1049（alt screen 退出）**：在 `scan_csi_state` 中识别这些序列后，调用 `damage.mark_all_dirty()`（manager 层），或返回标志让 manager 层处理。
- **检测 DECSET 47/1047/1049（alt screen 进入）**：同样标记全屏脏，确保进入 alt screen 时立即清除主屏残留。
- **检测 ALT_SCREEN_BUFFER_REGEN（DECSET 1049 + 清屏组合）**：保证 htop/vim 启动/退出时画面干净。

### C. demo 层：渲染选区（`demos/russh-slint-demo/src/render.rs` + `crates/rust-xterm-renderer/src/renderer.rs`）

- **新增 `Renderer::render_selection(selection: &SelectionRange, cells: &[Vec<RustXtermCell>])`**：对选区覆盖的 cell 反相绘制（fg↔bg 互换或叠加半透明蓝色）。
- **render.rs 在 dirty 行包含选区覆盖范围时调用 `render_selection`**：每次有 dirty 时，遍历 selection 覆盖的行列，反相显示。
- **mouse.rs 修正坐标换算**：`col = (x / (CELL_W * scale))`，`row = (y / (CELL_H * scale))`，使用 Slint `Window::scale_factor()`。
- **mouse.rs Shift 修饰键强制本地选区**：当 `mods.shift` 为真时，**即使 `is_mouse_grabbed` 也走本地 selection 路径**，让用户在 vim/htop 中能通过 Shift+拖拽选中文本（标准 xterm 行为）。

### D. demo 层：动态 tick 间隔（`demos/russh-slint-demo/src/main.rs` + `render.rs`）

- **空闲态延长 tick 到 500 ms**：当 SSH 数据队列空 + 无 dirty + 无 blink due + 无 scrollback 视图时，把 timer interval 切到 500 ms。
- **活动态恢复 16 ms**：当 SSH 数据队列非空或 dirty 非空时，立即切回 16 ms。
- **cursor blink 期间使用 500 ms tick**：闪烁到期前不需要更频繁的 tick。
- **FPS tracker 仅在真实像素上传时计 tick**：把 `fps_tracker.tick()` 移到 `should_upload` 守卫内，空闲时 FPS 自然降到 0。

### E. demo 层：实时 resize 同步（`demos/russh-slint-demo/src/main.rs` + `resize.rs`）

- **删除 200 ms resize 轮询定时器**：改用 Slint 1.6 `Window::on_resize_event` 或在主 tick 中每帧检测 `window().size()` 变化（无开销，只是 `u32` 比较）。
- **resize 在主 tick 中同步执行**：检测到 size 变化时，立即 `manager.resize + renderer.resize + renderer.clear + render_frame + force_upload`，避免任何"中间帧"被 Slint 拉伸。
- **resize 期间禁用 image-fit 拉伸**：app.slint 中 Image 改用 `image-fit: preserve` 或在 resize 中先把 image 设为空再设回，避免拉伸感。

### F. demo 层：操作延迟优化（`demos/russh-slint-demo/src/render.rs`）

- **SSH 数据 drain 上限**：单次 tick 内最多处理 N=64 KB SSH 数据，剩余留到下一 tick，避免长输出阻塞按键回包。
- **drain_output 与 send_input 在 poll_frame 后立即执行**：减少回环延迟。
- **按键事件直接唤醒 tick**：Slint 按键回调中检测 timer 是否处于 500 ms 模式，若是则手动触发一次 tick（通过 `Timer::start` 重新启动单次 timer）。

### G. 库层 + demo 层：内存优化

- **scrollback 默认限制 1000 行**：`WezTermCore::new` 接受 `scrollback_size` 参数（默认 1000，可配置）；当前默认 3500 行偏高。
- **atlas 改用 `with_global_atlas`**：在 demo 启动时创建一个 `GlobalAtlas`，多个 Renderer 实例共享（虽然当前 demo 只有一个 Renderer，但为未来 tab 多终端做准备，且可减少误创建多 atlas 的内存浪费）。
- **`sysinfo::System::new_all()` 改为 `System::new()` + 仅 refresh_process(pid)**：避免加载全部进程列表（Win11 上百进程）。
- **atlas 默认尺寸降到 512×512**：100 cols × ~24 row glyphs ≈ 2400 glyph，512×512 / 9×19 = 1529 glyph 容量已够，2 帧冷启动后即可命中。

## Impact

- Affected specs: `fix-russh-slint-demo-quality`（残留问题修复）、`extend-features-and-gui-demos`（render_selection 新增渲染层 API）
- Affected code:
  - `crates/rust-xterm-core/src/manager.rs`（poll_frame 跟踪 cursor 移动 + 可见性变化）
  - `crates/rust-xterm-core/src/state.rs`（新增 last_cursor_pos / last_cursor_visible 字段）
  - `crates/rust-xterm-core/src/wezterm_core.rs`（alt screen 切换检测、scrollback 默认值）
  - `crates/rust-xterm-core/src/damage.rs`（暴露 mark_all_dirty 已存在）
  - `crates/rust-xterm-renderer/src/renderer.rs`（新增 render_selection）
  - `demos/russh-slint-demo/src/{main,render,resize,mouse,input}.rs`、`ui/app.slint`
- **不影响**：rust-xterm 公共 API 签名（仅新增方法/字段）、SSH 协议层、demo 工作区独立性

## ADDED Requirements

### Requirement: poll_frame 跟踪 cursor 移动并标记老位置脏
`TerminalManager::poll_frame` SHALL 在 cursor 位置（x, y）与上一次 poll_frame 时不同时，将**老 cursor 位置所在行**与**新 cursor 位置所在行**都加入 dirty_spans，确保 `render_frame` 重绘这两行以擦除老位置的光标像素。

#### Scenario: 回车后老光标位置无残留
- **WHEN** 用户按下 Enter，shell 回显 `\r\n` 使 cursor 从 (0, 5) 移动到 (0, 6)
- **THEN** poll_frame 返回的 dirty_spans 包含 row=5 和 row=6；render_frame 重绘 row=5 后老光标像素被擦除

### Requirement: poll_frame 跟踪 cursor 可见性变化并标记行脏
`TerminalManager::poll_frame` SHALL 在 cursor.visible 翻转（闪烁）时将 cursor 所在行加入 dirty_spans，确保 `render_cursor` 在 cursor 不可见时该行已被 `render_frame` 重绘以擦除上一帧的光标像素。

#### Scenario: 光标闪烁无残留
- **WHEN** cursor 闪烁周期到期，visible 从 true 翻转到 false
- **THEN** poll_frame 返回的 dirty_spans 包含 cursor 行；render_frame 重绘后 cursor 像素被擦除；下一周期 visible 翻回 true 时 render_cursor 在新像素上绘制

### Requirement: poll_frame 始终在脏区存在时把 cursor 行标脏
`TerminalManager::poll_frame` SHALL 在 has_damage 为真时，无条件将当前 cursor 所在行加入 dirty_spans，确保 `render_frame` 重绘该行后 `render_cursor` 能在干净的像素基础上叠加光标。

#### Scenario: 脏区重绘不覆盖光标
- **WHEN** SSH 输出导致 cursor 行外的其他行变脏，但 cursor 行本身未变
- **THEN** poll_frame 仍把 cursor 行加入 dirty_spans；render_frame 重绘该行后 render_cursor 重新叠加，避免脏区外光标被擦除

### Requirement: alt screen 切换强制全屏脏
`TerminalManager::write` SHALL 在数据流中检测到 `DECRST 47`、`DECRST 1047`、`DECRST 1049`（退出 alt screen）或 `DECSET 47/1047/1049`（进入 alt screen）时，调用 `damage.mark_all_dirty()`，确保下一帧 poll_frame 返回完整屏幕的 dirty_spans。

#### Scenario: htop 退出无残留
- **WHEN** htop 退出，远端发送 `\x1b[?1049l`（退出 alt screen）
- **THEN** manager.write 检测到该序列后 mark_all_dirty；下一帧 poll_frame 返回所有行 dirty；render_frame 全屏重绘，htop 残留被主屏内容覆盖

### Requirement: Renderer 支持 render_selection
`Renderer` SHALL 提供 `pub fn render_selection(&mut self, selection: &SelectionRange, snapshot: &[Vec<RustXtermCell>])`，对选区覆盖的 cell 反相绘制（fg 与 bg 互换），让用户能看到选中的文本范围。

#### Scenario: 拖拽选中文本显示蓝色高亮
- **WHEN** 用户拖拽鼠标选中 (row=0, col=0)..(row=0, col=4) 的文本
- **THEN** render_selection 对这 5 个 cell 反相绘制，前景变背景色，背景变前景色

### Requirement: 鼠标坐标使用物理像素
demo 的 `mouse.rs` SHALL 使用 `Window::scale_factor()` 把 Slint 传来的物理鼠标坐标换算为逻辑 cell 坐标：`col = (x / (CELL_W * scale)) as usize`，`row = (y / (CELL_H * scale)) as usize`。

#### Scenario: HiDPI 下鼠标点击不偏移
- **WHEN** 在 scale_factor=2.0 的显示器上点击 cell (5, 3)
- **THEN** mouse.rs 计算 col=5, row=3，与显示的 cell 位置一致

### Requirement: Shift 修饰键强制本地选区
当用户按住 Shift 拖拽鼠标时，demo SHALL 即使远端程序启用了鼠标跟踪（`is_mouse_grabbed` 为真）也走本地 selection 路径，让用户能在 vim/htop 中选中并复制文本。

#### Scenario: vim 中 Shift+拖拽选中复制
- **WHEN** vim 启用鼠标跟踪，用户按 Shift + 左键拖拽
- **THEN** demo 走本地 selection 路径，选中范围反相显示；释放后文本复制到剪贴板

### Requirement: 动态 tick 间隔
demo SHALL 在 SSH 数据队列空 + 无 dirty + 无 blink due + 无 scrollback 视图时，把 timer interval 切到 500 ms；在 SSH 数据非空或 dirty 非空时立即切回 16 ms。FPS tracker SHALL 仅在真实像素上传时调用 `tick()`，空闲时 FPS 自然降到 0。

#### Scenario: 空闲时 FPS 接近 0
- **WHEN** SSH 空闲，无 dirty，无 blink due
- **THEN** timer interval = 500 ms；fps_tracker 不被调用；状态栏显示 FPS: 0

#### Scenario: SSH 输出时立即恢复 60 FPS
- **WHEN** SSH 收到数据，drain 后 has_damage = true
- **THEN** timer interval 切回 16 ms；fps_tracker 正常计数；FPS 显示 30+

### Requirement: resize 在主 tick 中同步执行
demo SHALL 在主 render tick 中每帧检测 `app.window().size()` 变化，若变化则立即执行 `manager.resize + renderer.resize + renderer.clear + render_frame + force_upload`，删除独立的 200 ms resize 轮询定时器。

#### Scenario: 拉伸窗口实时 reflow
- **WHEN** 用户拖拽窗口右下角放大窗口
- **THEN** 每个 tick（16 ms）检测到 size 变化即同步 reflow，画面按字符重排而非像素拉伸

### Requirement: SSH 数据 drain 上限
demo 的 render tick SHALL 单次最多处理 N=64 KB SSH 数据，剩余留到下一 tick，避免长输出（如 `yes`、`find /`）阻塞按键回包。

#### Scenario: 长输出时按键仍有响应
- **WHEN** 远端运行 `yes` 持续输出，用户按键
- **THEN** 单次 tick 处理 64 KB 后立即返回，按键回包延迟 < 100 ms

### Requirement: scrollback 默认 1000 行
`WezTermCore::new` SHALL 默认限制 scrollback 为 1000 行（可通过 `with_scrollback_size` 配置），减少初始内存占用。

#### Scenario: 启动内存 < 30 MB
- **WHEN** demo 启动并完成 SSH 连接
- **THEN** 进程 RSS < 30 MB（当前 50+ MB）

## MODIFIED Requirements

### Requirement: sysinfo 仅加载当前进程
demo SHALL 用 `sysinfo::System::new()` 替代 `System::new_all()`，仅 refresh_process(pid)，避免加载全部进程列表的内存开销。

### Requirement: atlas 默认尺寸
demo 的 `RendererConfig` SHALL 默认 `atlas_width=512, atlas_height=512`，减少冷启动内存 4 MB → 1 MB。

## REMOVED Requirements

无移除。
