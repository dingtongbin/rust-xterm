# Checklist

## 核心层特性
- [x] 焦点报告：TerminalEvent::FocusReport 变体已加
- [x] 焦点报告：set_focused 在 DECSET 1004 启用时生成 `\x1b[I`/`\x1b[O`
- [x] 焦点报告：is_focus_reporting_enabled() 查询 API 已暴露
- [x] 焦点报告：test_focus_report_enabled / _disabled 测试通过
- [x] OSC 7：TerminalEvent::CwdChange(PathBuf) 变体已加
- [x] OSC 7：OSC 7 handler 解析 file:// 并 emit CwdChange
- [x] OSC 7：test_osc7_cwd_event / _malformed_ignored 测试通过
- [x] 滚动区域：scroll_region() -> Option<(usize, usize)> 已暴露
- [x] 滚动区域：test_scroll_region_query 测试通过
- [x] 键盘映射：input.rs 模块已创建，KeyInput/KeyMods/KeyMapping 类型已导出
- [x] 键盘映射：encode_key 覆盖方向键(SS3/CSI)/F1-F12/Home/End/Insert/Delete/PageUp/PageDown/Ctrl+字母/Alt+字符/Enter/Backspace/Tab/Esc
- [x] 键盘映射：tests/input_mapping.rs 测试覆盖所有变体 app_cursor on/off
- [x] 键盘映射：EventLoop::send_key 便利方法已加
- [x] 选区模型：SelectionRange 结构已定义
- [x] 选区模型：TerminalManager.selection 字段 + set_selection/selection/selection_text API
- [x] 选区模型：buffer.selection_text 线性选区（跨行 \n 连接）
- [x] 选区模型：buffer.selection_text 矩形选区（按列截取）
- [x] 选区模型：test_selection_linear_text / _rectangular_text 测试通过
- [x] 选区交互：mouse.rs 选区状态机字段已加
- [x] 选区交互：左键按下设起点、拖拽扩展终点 emit SelectionChange、释放 emit SelectionReady
- [x] 选区交互：双击智能选词（字符类别边界扩展）
- [x] 选区交互：三击选整行
- [x] 选区交互：select_word / select_line 辅助函数已实现
- [x] 选区交互：test_mouse_drag_selection / _double_click_select_word / _triple_click_select_line 测试通过
- [x] 双宽度字符：test_wide_char_advance / _cursor_movement / _overwrite 测试通过

## 核心层验证（Rust 1.88）
- [x] cargo +1.88.0 build --all-targets 通过
- [x] cargo +1.88.0 test --all-targets 通过（含新增测试）
- [x] cargo +1.88.0 clippy --all-targets -- -D warnings 通过
- [x] cargo +1.88.0 fmt --all -- --check 通过

## GUI Demo 依赖隔离
- [ ] /workspace/Cargo.toml 的 members 不含 demos/*
- [ ] /workspace/Cargo.toml 的 [workspace.dependencies] 不含 slint/iced/egui/eframe
- [ ] 三个 demo 的 Cargo.toml 用 path 引用 rust-xterm 库 crate，GUI 依赖写完整版本（非 .workspace = true）
- [ ] cd /workspace && cargo build 不编译任何 GUI 框架
- [ ] grep -E "name = \"(slint|iced|egui|eframe|wgpu|i-slint|glutin|gl)\"" /workspace/Cargo.lock 无输出

## slint-demo
- [ ] demos/slint-demo/Cargo.toml 独立 package，含独立 Cargo.lock
- [ ] 启动窗口 + spawn 默认 shell
- [ ] 终端像素完整绘制（RGBA Image 组件）
- [ ] 键盘全交互（普通字符/方向键/功能键/Ctrl+Alt 组合，走 encode_key）
- [ ] 鼠标左键拖拽选区 + 释放自动复制到剪贴板
- [ ] 滚轮滚动 scrollback
- [ ] 窗口 resize → EventLoop::resize + Renderer::resize
- [ ] 底部状态栏实时 FPS（滑动平均）+ 占用内存
- [ ] cd demos/slint-demo && cargo build 通过

## iced-demo
- [ ] demos/iced-demo/Cargo.toml 独立 package，含独立 Cargo.lock
- [ ] 启动窗口 + spawn 默认 shell
- [ ] 终端像素完整绘制（canvas/image widget）
- [ ] 键盘全交互
- [ ] 鼠标选区 + 自动复制
- [ ] 滚轮 scrollback
- [ ] 窗口 resize
- [ ] 底部状态栏 FPS + 内存
- [ ] cd demos/iced-demo && cargo build 通过

## egui-demo
- [ ] demos/egui-demo/Cargo.toml 独立 package，含独立 Cargo.lock
- [ ] 启动窗口 + spawn 默认 shell
- [ ] 终端像素完整绘制（TextureHandle RGBA）
- [ ] 键盘全交互
- [ ] 鼠标选区 + 自动复制
- [ ] 滚轮 scrollback
- [ ] 窗口 resize
- [ ] 底部状态栏 FPS + 内存
- [ ] cd demos/egui-demo && cargo build 通过

## PROGRESS.md 对照文档
- [ ] /workspace/PROGRESS.md 已创建
- [ ] 列出本 spec 所有特性及完成状态（已完成打勾）
- [ ] 列出超出范围特性及理由（连字/彩色Emoji/Unicode CJK Ext/图像协议/IME/全局字形缓存）
- [ ] 列出 3 个 demo 完成状态和依赖隔离验证结果
