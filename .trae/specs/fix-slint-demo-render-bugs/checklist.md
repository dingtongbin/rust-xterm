# Checklist

## Demo 层
- [x] resize timer 使用 `app.window().scale_factor()` 转换为 physical 像素
- [x] `renderer.resize()` 接收 physical 像素，canvas 与 Slint Image 物理尺寸 1:1
- [x] `app.slint` 的 `image-fit` 不再为 `fill`（改为 `contain`）
- [x] `font_size` 与 `cell_height` 对齐（均 16）
- [x] resize timer 中 `render_cursor` 使用 `frame.cursor.shape` 而非硬编码 `CursorShape::Default`
- [x] demo 启动后调用 `event_loop.set_cursor_blinking(true)`
- [x] 内存显示使用 `refresh_process(pid)` + `process.memory()` 而非 `refresh_memory` + `used_memory`
- [x] scroll_cb 不再早返回 `if max == 0 { return; }`
- [x] `RendererConfig.default_fg`/`default_bg` 从 `WindowsTerminalTheme`（Campbell）读取而非硬编码 WHITE/BLACK

## 库层
- [x] `EventLoop::set_cursor_blinking(&mut self, bool)` 已暴露并委托 manager
- [x] `renderer.rs` 的 `CursorShape::Default` 走 Bar 分支而非 Block 分支
- [x] `test_default_cursor_renders_bar_not_block` 测试通过
- [x] 现有 cursor 渲染测试未回归（42 测试全过）

## 文档
- [x] spec.md 注明 Enter → `\r` → TTY echo `\r\n` 为预期行为

## 验证
- [x] HiDPI 显示器（scale=2）下 resize 无模糊（代码路径验证：scale_factor 已应用）
- [x] 非等比窗口下终端不拉伸变形（image-fit: contain）
- [x] 光标按内置周期闪烁（set_cursor_blinking(true) 已调用）
- [x] 光标形状为竖线（Bar）而非块（Block）（Default 走 Bar 分支 + 测试断言）
- [x] 状态栏内存数值与 `ps -o rss= -p <pid>` 一致（refresh_process + process.memory）
- [x] 无 scrollback 内容时滚轮不报错；有内容时正常滚动（早返回已删除 + .min(max) clamp）
- [x] ANSI 默认色文本与 WezTerm 调色板一致（apply_theme + Campbell）
- [x] `cargo build` 在 `/workspace` 不触发 GUI 依赖编译（依赖隔离保持）
- [x] `cargo build` 在 `/workspace/demos/slint-demo` 通过
- [x] `cargo clippy` 在 demo 与受影响 crate 通过（`cargo clippy --release -- -D warnings` 零警告）
- [x] `cargo fmt --check` 通过

## 模块化（可选 Task 11）
- [x] main.rs 拆分为 6 个模块后行为与拆分前一致（cargo build/clippy/fmt 全绿）
- [x] main.rs 行数从 586 降至 213（-63%，目标 ~150-200，略超但 setup 代码不可压缩）
- [x] 各模块 31-123 行（fps.rs 31 / resize.rs 46 / app_ctx.rs 47 / mouse.rs 76 / render.rs 94 / input.rs 123）
