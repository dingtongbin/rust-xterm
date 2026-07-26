// =============================================================================
// slint-demo: rust-xterm + Slint 终端 demo
// =============================================================================
// 启动 Slint 窗口，spawn 操作系统默认 shell，将 rust-xterm 终端像素完整
// 绘制到 Image 组件。键盘/鼠标/滚轮/resize 全交互，底部状态栏显示 FPS
// 与内存。左键拖拽选区 + 释放自动复制到剪贴板，中键粘贴。
//
// 选区状态机（单击/双击/三击/拖拽）由 TerminalManager.mouse_event 内部维护，
// 本 demo 仅负责：转发 GUI 事件 + Release 时读取 selection_text 复制到剪贴板
// + 中键粘贴。
//
// 依赖隔离：本 crate 不加入 /workspace 的 workspace members，独立 Cargo.lock。
// =============================================================================

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_ctx;
mod fps;
mod input;
mod mouse;
mod render;
mod resize;

use app_ctx::{AppCtx, ClipboardHandle};
use rust_xterm_host::{EventLoop, EventLoopConfig, PtyBridge, PtyConfig};
use rust_xterm_renderer::{RenderMetrics, Renderer, RendererConfig};
use slint::{CloseRequestResponse, Timer, TimerMode};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

slint::include_modules!();

// -----------------------------------------------------------------------------
// 常量
// -----------------------------------------------------------------------------
pub(crate) const CELL_W: u32 = 8;
pub(crate) const CELL_H: u32 = 16;
pub(crate) const STATUS_BAR_H: u32 = 22;
const INITIAL_COLS: usize = 80;
const INITIAL_ROWS: usize = 24;
const TICK_INTERVAL_MS: u64 = 16;
pub(crate) const MEM_REFRESH_MS: u64 = 500;

// -----------------------------------------------------------------------------
// 主函数
// -----------------------------------------------------------------------------
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // ---- 1. 构造 TerminalManager + PtyBridge + EventLoop ----
    let size = rust_xterm_core::TerminalSize::new(INITIAL_ROWS, INITIAL_COLS);
    let mut manager = rust_xterm_core::TerminalManager::utf8(size);
    // 应用 Windows Terminal Campbell 主题：默认前景/背景色与 ANSI 调色板一致，
    // 避免 demo 硬编码 WHITE/BLACK 导致"同红色不同深浅"的视觉不一致。
    // manager.default_fg/default_bg 也会被 apply_theme 更新，screen_snapshot 会用到。
    let theme = rust_xterm_core::WindowsTerminalTheme::default();
    manager.apply_theme(&theme);
    let default_fg = rust_xterm_core::Color::rgba(
        (theme.foreground.0 * 255.0) as u8,
        (theme.foreground.1 * 255.0) as u8,
        (theme.foreground.2 * 255.0) as u8,
        (theme.foreground.3 * 255.0) as u8,
    );
    let default_bg = rust_xterm_core::Color::rgba(
        (theme.background.0 * 255.0) as u8,
        (theme.background.1 * 255.0) as u8,
        (theme.background.2 * 255.0) as u8,
        (theme.background.3 * 255.0) as u8,
    );

    let shell = PtyConfig::detect_default_shell();
    eprintln!("[slint-demo] detected shell: {shell}");
    let pty_config = PtyConfig {
        shell,
        cols: INITIAL_COLS as u16,
        rows: INITIAL_ROWS as u16,
        cwd: None,
    };
    let pty = match PtyBridge::new(&pty_config) {
        Ok(p) => Some(p),
        Err(e) => {
            eprintln!("[slint-demo] PTY spawn failed: {e:?}");
            None
        }
    };
    let pty_alive = pty.is_some();
    let mut event_loop = EventLoop::new(manager, pty, EventLoopConfig::default());
    // 启用光标闪烁（EventLoop 默认关闭，需显式开启）
    event_loop.set_cursor_blinking(true);

    // ---- 2. 构造 Renderer（canvas 尺寸 = 初始像素）----
    let canvas_w = INITIAL_COLS as u32 * CELL_W;
    let canvas_h = INITIAL_ROWS as u32 * CELL_H;
    let renderer_config = RendererConfig {
        metrics: RenderMetrics {
            cell_width: CELL_W,
            cell_height: CELL_H,
            baseline: 13,
            dpi: 96.0,
            // font_size 与 cell_height 对齐：消除字形在 cell 内的垂直留白导致的模糊
            font_size: 16.0,
        },
        atlas_width: 1024,
        atlas_height: 1024,
        canvas_width: canvas_w,
        canvas_height: canvas_h,
        default_fg,
        default_bg,
        enable_ligatures: true,
    };
    let mut renderer = Renderer::new(renderer_config);
    // 初始清屏
    renderer.clear();

    // ---- 3. 剪贴板 ----
    let clipboard: ClipboardHandle =
        Arc::new(Mutex::new(arboard::Clipboard::new().unwrap_or_else(|e| {
            eprintln!("[slint-demo] clipboard init failed: {e:?}");
            // 无法用 arboard，用占位会 panic —— 降级为不复制
            std::process::exit(0)
        })));

    // ---- 4. 创建 Slint App ----
    let app = App::new()?;
    let app_weak = app.as_weak();

    // ---- 5. 共享状态 ----
    let pid = sysinfo::get_current_pid().expect("get_current_pid");
    let ctx = Rc::new(RefCell::new(AppCtx::new(
        event_loop, renderer, pty_alive, pid,
    )));

    // ---- 6. 键盘回调 ----
    let ctx_kb = Rc::clone(&ctx);
    app.on_key_pressed_cb(move |text, ctrl, alt, shift| {
        input::handle_key_pressed(&ctx_kb, &text, ctrl, alt, shift);
    });

    // ---- 7. 鼠标回调 ----
    // TerminalManager.mouse_event 内部已实现：
    //   - 鼠标跟踪模式（is_mouse_grabbed）下转发给 WezTerm 并生成报告
    //   - 非跟踪模式下：单击拖拽选区 / 双击选词 / 三击选行 / 释放触发 SelectionReady
    // 本 demo 仅负责：
    //   - 转发 GUI 事件给 mouse_event
    //   - Release(Left) 时读取 selection_text 复制到剪贴板
    //   - Press(Middle) 时从剪贴板粘贴
    let ctx_mouse = Rc::clone(&ctx);
    let cb_clip = Arc::clone(&clipboard);
    app.on_pointer_event_cb(move |x, y, kind, button| {
        mouse::handle_pointer_event(&ctx_mouse, &cb_clip, x, y, kind, button);
    });

    // ---- 8. 滚轮 scrollback 回调 ----
    // Slint 1.6 的 TouchArea 提供 scroll-event 回调（PointerScrollEvent.delta-y）。
    // 正值表示向上滚（手指向后推），负值表示向下滚。
    // 按 CELL_H 像素 / 行换算为行数，调整 scroll_offset（夹在 [0, max_scrollback]）。
    let ctx_wheel = Rc::clone(&ctx);
    app.on_scroll_cb(move |delta_y| {
        let mut ctx = ctx_wheel.borrow_mut();
        let max = ctx.event_loop.manager_ref().max_scrollback();
        // 不在 max == 0 时早返回：让 scroll_offset 自由累积，
        // 由末尾 .min(max) clamp 到 0，避免无 scrollback 时滚轮完全无响应的体感。
        // 像素 → 行：累积小数部分避免丢失
        let pixels_per_row = CELL_H as f32;
        let delta_rows = delta_y / pixels_per_row;
        ctx.scroll_offset = if delta_y > 0.0 {
            (ctx.scroll_offset as f32 + delta_rows).round() as usize
        } else {
            ctx.scroll_offset
                .saturating_sub((-delta_rows).round() as usize)
        }
        .min(max);
    });

    // ---- 9. 窗口关闭请求 ----
    let ctx_close = Rc::clone(&ctx);
    app.window().on_close_requested(move || {
        let _ctx = ctx_close.borrow();
        CloseRequestResponse::HideWindow
    });

    // ---- 10. 定时器：驱动 EventLoop::tick + 渲染 + FPS + 内存 ----
    let ctx_tick = Rc::clone(&ctx);
    let app_weak_tick = app_weak.clone();
    let timer = Timer::default();
    timer.start(
        TimerMode::Repeated,
        Duration::from_millis(TICK_INTERVAL_MS),
        move || {
            render::tick(&ctx_tick, &app_weak_tick);
        },
    );

    // ---- 11. resize 检测定时器（200ms 间隔）----
    let ctx_resize = Rc::clone(&ctx);
    let app_weak_resize = app_weak.clone();
    let mut last_size: (u32, u32) = (canvas_w, canvas_h + STATUS_BAR_H);
    let resize_timer = Timer::default();
    resize_timer.start(TimerMode::Repeated, Duration::from_millis(200), move || {
        let Some(app) = app_weak_resize.upgrade() else {
            return;
        };
        resize::handle_resize(&ctx_resize, &app, &mut last_size);
    });

    // ---- 12. 运行 ----
    // 注意：必须用真实绑定持有 Timer，不能用 `let _ = ...`！
    // `_` 是通配模式不绑定值，元组会在 let 语句结束时立即 drop，
    // 导致两个定时器被销毁、回调永不触发——表现为窗口黑屏 + 状态栏卡在 "--"。
    let _timer_holders = (timer, resize_timer);
    app.run()?;
    Ok(())
}
