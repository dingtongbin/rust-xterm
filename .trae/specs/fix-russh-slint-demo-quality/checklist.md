# Checklist

## 库层（rust-xterm-renderer / rust-xterm-core）

- [x] RendererConfig 新增 `scale_factor: f32` 字段，默认 1.0
- [x] Renderer::new 按 scale_factor 计算 scaled cell/font_size，调用 font_tree.set_shape_size
- [x] Renderer::resize 在 cell 尺寸变化时清空 atlas.clear_dynamic()（glyph_cache + run_cache）
- [x] 暴露 `pub fn set_scale_factor(&mut self, scale: f32)` 方法
- [x] scale_factor=2.0 时 scaled cell_w 与 font_size 翻倍的单元测试通过（45 passed）
- [x] encode_key 对 ArrowUp/Down/Left/Right/Home/End 在 mods 非空时输出 modifyOtherKeys 编码
- [x] encode_key 对 F1-F4/F5-F12 在 mods 非空时输出 modifyOtherKeys 编码
- [x] Shift+Tab 输出 `\x1b[Z`
- [x] Ctrl+Right 输出 `\x1b[1;5C` 单元测试通过
- [x] Ctrl+Up 输出 `\x1b[1;5A` 单元测试通过
- [x] TerminalManager 暴露 `pub fn app_cursor_mode(&self) -> bool`
- [x] wezterm_core 在 scan_csi_state 中扫描 DECSET 1 / DECRST 1 维护 app_cursor 字段（120 单元测试通过）

## demo 层（russh-slint-demo）

- [x] resize.rs 直接使用 `app.window().size()` 物理像素，删除二次 `* scale`
- [x] cols/rows 计算用物理像素 / scaled_cell_w
- [x] renderer.resize 传精确物理像素，canvas 与显示矩形 1:1
- [x] render.rs 接收 render_frame 返回的 RenderResult
- [x] set_terminal_image 仅在 dirty_rects 非空或 cursor 变化时调用
- [x] SshEvent::Closed 分支：`ctx.connected = false`、`renderer.clear()`、上传一次清屏像素
- [x] 状态栏 fps/mem/scroll 文本 dirty 检查（缓存上次字符串比较）
- [x] input.rs 调用 encode_key 传入 `manager.app_cursor_mode()`
- [x] input.rs 实现 Ctrl+Shift+V 从 arboard 读取并按 bracketed paste 发送
- [x] input.rs 补全 Ctrl+非字母字符（@ [ \\ ] ^ _）控制编码
- [x] main.rs 滚轮回调：manager.is_mouse_grabbed() 时构造 WheelUp/Down 调用 mouse_event
- [x] mouse.rs 滚轮事件非鼠标跟踪模式下保持 scrollback 行为
- [x] ui/app.slint 新增右侧垂直 ScrollBar，绑定 scroll-offset / scroll-max
- [x] ui/app.slint 窗口默认尺寸 1000×640，最小 400×300
- [x] main.rs CELL_W=9, CELL_H=19, font_size=18.0
- [x] INITIAL_COLS / INITIAL_ROWS 调整为 100×30
- [x] render.rs 在 tick 中更新 app.scroll_max / scroll_offset

## slint-demo 同步修复（同类问题）

- [x] demos/slint-demo/src/resize.rs 修正二次 scale bug
- [x] demos/slint-demo/src/render.rs 脏区驱动上传 + 状态栏 dirty 检查
- [x] demos/slint-demo/src/main.rs 默认参数对齐 wt

## 验证

- [x] `/workspace` cargo build --all-targets 通过
- [x] `/workspace` cargo clippy --all-targets -- -D warnings 零警告
- [x] `/workspace` cargo fmt --all --check 通过
- [x] `/workspace` cargo test --all-targets 全部通过（rust-xterm-renderer 45 + rust-xterm-core 120 单元 + 15 input_mapping + 7 GBK + 6 idle + 4 yes + host 3 + api_lock 9 + smoke_pty 4）
- [x] `/workspace/demos/russh-slint-demo` cargo build --all-targets 通过
- [x] `/workspace/demos/russh-slint-demo` cargo clippy --all-targets -- -D warnings 零警告
- [x] `/workspace/demos/russh-slint-demo` cargo fmt --all --check 通过
- [x] `/workspace/demos/russh-slint-demo` cargo test --all-targets 全部通过（22 单元 + 5 ssh_integration）
- [x] `/workspace/demos/slint-demo` cargo build --all-targets 通过
- [x] `/workspace/demos/slint-demo` cargo clippy --all-targets -- -D warnings 零警告

## 用户实测验证（spec 验收场景，需在 Win11 实测）

- [ ] HiDPI 显示器上启动无字体模糊（需用户实测）
- [ ] 字体大小与 Windows Terminal 默认 12pt 一致（需用户实测）
- [x] 窗口默认 1000×640，最小 400×300（代码已实现）
- [ ] htop 中滚轮可滚动屏幕（需用户实测）
- [ ] htop 中鼠标无错位（需用户实测）
- [ ] vim 中方向键不插入垃圾字符（app_cursor 模式生效，需用户实测）
- [ ] Ctrl+Right 在 vim 中正确跳单词（需用户实测）
- [ ] Ctrl+Shift+V 粘贴剪贴板内容（需用户实测）
- [ ] 空闲时 FPS 接近 0（无像素上传），htop 刷新时保持 ≥30 FPS（需用户实测）
- [ ] SSH 关闭后画面清空，无残留（代码已实现，需用户实测）
- [ ] 状态栏内存显示与 slint-demo 量级一致（差异 < 20 MB，需用户实测）
- [x] 右侧滚动条显示 scrollback 位置（代码已实现）
