// =============================================================================
// russh-slint-demo: rust-xterm + Slint + russh SSH 终端 demo
// =============================================================================
// 启动后：
//   1. 读取 ./config.json（或 argv[1] 指定路径）作为 SSH 连接配置
//   2. 显示连接遮罩，spawn SSH 后台线程建立 TCP+认证+PTY+shell
//   3. SSH 进度通过 mpsc 实时上报到 UI，遮罩状态文字随之刷新
//   4. 收到 SshEvent::Connected → 撤销遮罩，切换到终端视图
//   5. SSH channel 数据通过 mpsc 喂入 TerminalManager，渲染到 Slint Image
//
// 与 slint-demo 的架构差异：
//   - 不依赖 rust-xterm-host（无本地 PTY）
//   - SshBridge 替代 PtyBridge，自定义 tick 替代 EventLoop::tick
//   - 连接遮罩 + 状态机切换由本 demo 自管
// =============================================================================

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_ctx;
mod config;
mod fps;
mod input;
mod mouse;
mod render;
mod resize;
mod ssh;

use app_ctx::{AppCtx, ClipboardHandle};
use config::SshConfig;
use rust_xterm_renderer::{RenderMetrics, Renderer, RendererConfig};
use slint::{CloseRequestResponse, Timer, TimerMode};
use ssh::SshBridge;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

slint::include_modules!();

// -----------------------------------------------------------------------------
// 常量
// -----------------------------------------------------------------------------
pub(crate) const CELL_W: u32 = 9;
pub(crate) const CELL_H: u32 = 19;
pub(crate) const STATUS_BAR_H: u32 = 22;
const INITIAL_COLS: usize = 100;
const INITIAL_ROWS: usize = 30;
const TICK_INTERVAL_MS: u64 = 16;
pub(crate) const MEM_REFRESH_MS: u64 = 500;
const DEFAULT_CONFIG_PATH: &str = "config.json";

// -----------------------------------------------------------------------------
// 主函数
// -----------------------------------------------------------------------------
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // ---- 0. 解析配置路径（argv[1] 优先） ----
    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| DEFAULT_CONFIG_PATH.to_string());

    // 先创建 Slint App，让 UI 立即显示遮罩（即使后续配置加载失败也能反馈）
    let app = App::new()?;
    app.set_status_text("正在加载配置...".into());
    let app_weak = app.as_weak();

    // ---- 1. 加载 SSH 配置 ----
    let config: SshConfig = match SshConfig::load_from_file(&config_path) {
        Ok(c) => c,
        Err(e) => {
            // 配置加载失败：在遮罩显示错误并继续运行 Slint（不立即退出）
            eprintln!("[russh-slint-demo] 加载配置失败: {e:#}");
            eprintln!("[russh-slint-demo] 提示: 请复制 config.example.json 为 config.json 并填入实际 SSH 信息");
            app.set_status_text("配置加载失败".into());
            app.set_error_text(format!("{e:#}").into());
            // 仍然运行 app 让用户看到错误（关闭窗口退出）
            app.run()?;
            return Ok(());
        }
    };

    // ---- 2. 构造 TerminalManager（应用 Campbell 主题） ----
    let size = rust_xterm_core::TerminalSize::new(INITIAL_ROWS, INITIAL_COLS);
    let mut manager = rust_xterm_core::TerminalManager::utf8(size);
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
    manager.set_cursor_blinking(true);

    // ---- 3. 构造 Renderer（canvas = 初始像素尺寸） ----
    let canvas_w = INITIAL_COLS as u32 * CELL_W;
    let canvas_h = INITIAL_ROWS as u32 * CELL_H;
    let renderer_config = RendererConfig {
        metrics: RenderMetrics {
            cell_width: CELL_W,
            cell_height: CELL_H,
            baseline: CELL_H - 3,
            dpi: 96.0,
            font_size: 18.0,
        },
        atlas_width: 512,
        atlas_height: 512,
        canvas_width: canvas_w,
        canvas_height: canvas_h,
        default_fg,
        default_bg,
        enable_ligatures: true,
        scale_factor: 1.0,
    };
    let mut renderer = Renderer::new(renderer_config);
    renderer.clear();

    // ---- 4. 剪贴板 ----
    let clipboard: ClipboardHandle =
        Arc::new(Mutex::new(arboard::Clipboard::new().unwrap_or_else(|e| {
            eprintln!("[russh-slint-demo] clipboard init failed: {e:?}");
            std::process::exit(0)
        })));

    // ---- 5. 共享状态 ----
    let pid = sysinfo::get_current_pid().expect("get_current_pid");
    let ctx = Rc::new(RefCell::new(AppCtx::new(manager, renderer, pid)));

    // ---- 6. 启动 SSH 连接（异步，立即返回；遮罩已默认显示） ----
    {
        let mut ctx = ctx.borrow_mut();
        let bridge = SshBridge::connect(config, INITIAL_COLS as u16, INITIAL_ROWS as u16);
        ctx.bridge = Some(bridge);
    }

    // ---- 7. 键盘回调 ----
    let ctx_kb = Rc::clone(&ctx);
    let kb_clip = Arc::clone(&clipboard);
    app.on_key_pressed_cb(move |text, ctrl, alt, shift| {
        input::handle_key_pressed(&ctx_kb, &kb_clip, &text, ctrl, alt, shift);
    });

    // ---- 8. 鼠标回调 ----
    let ctx_mouse = Rc::clone(&ctx);
    let cb_clip = Arc::clone(&clipboard);
    let app_weak_mouse = app_weak.clone();
    app.on_pointer_event_cb(move |x, y, kind, button| {
        // HiDPI：获取窗口 scale_factor 传给 mouse 处理器（Task 4.2）
        let scale = app_weak_mouse
            .upgrade()
            .map(|a| a.window().scale_factor())
            .unwrap_or(1.0);
        mouse::handle_pointer_event(&ctx_mouse, &cb_clip, x, y, kind, button, scale);
    });

    // ---- 9. 滚轮回调 ----
    let ctx_wheel = Rc::clone(&ctx);
    app.on_scroll_cb(move |delta_y| {
        let mut ctx = ctx_wheel.borrow_mut();
        let pixels_per_row = CELL_H as f32;
        let delta_rows = (delta_y.abs() / pixels_per_row).round() as u32;

        // 远端程序启用鼠标跟踪时，转发滚轮事件给 manager
        if ctx.manager.is_mouse_grabbed() && delta_rows > 0 {
            use rust_xterm_core::{KeyMods, MouseAction, MouseButton};
            let (action, btn) = if delta_y > 0.0 {
                (MouseAction::WheelUp(delta_rows), MouseButton::Left)
            } else {
                (MouseAction::WheelDown(delta_rows), MouseButton::Left)
            };
            // 鼠标位置：用最后一次 pointer-event 的位置（如果没有，用 0,0）
            let (col, row) = ctx.last_mouse_pos.unwrap_or((0, 0));
            ctx.manager
                .mouse_event(col, row, action, btn, KeyMods::default());
            // 通过 drain_output 取出 WezTerm 编码的鼠标报告字节，发给 SSH channel
            let out = ctx.manager.drain_output();
            if !out.is_empty() {
                if let Some(bridge) = ctx.bridge.as_ref() {
                    let _ = bridge.send_input(out);
                }
            }
            return; // 鼠标跟踪模式下不更新 scroll_offset
        }

        // 非鼠标跟踪模式：保持 scrollback 行为
        let max = ctx.manager.max_scrollback();
        ctx.scroll_offset = if delta_y > 0.0 {
            (ctx.scroll_offset + delta_rows as usize).min(max)
        } else {
            ctx.scroll_offset.saturating_sub(delta_rows as usize)
        };
    });

    // ---- 9.5. ScrollBar 拖拽/点击 → ctx.scroll_offset（双向绑定） ----
    // Slint 1.6 不为 in-out property 生成 on_<prop>_changed，改用自定义
    // scroll-to(float) 回调（ScrollBar TouchArea 触发）。
    // 使用 try_borrow_mut：render::tick 持有 borrow 时安全跳过（值已在 tick 中同步）。
    let ctx_scroll = Rc::clone(&ctx);
    app.on_scroll_to(move |ratio_offset| {
        if let Ok(mut ctx) = ctx_scroll.try_borrow_mut() {
            let max = ctx.manager.max_scrollback();
            let new_offset = (ratio_offset.max(0.0) as usize).min(max);
            ctx.scroll_offset = new_offset;
        }
    });

    // ---- 10. 窗口关闭请求 ----
    let ctx_close = Rc::clone(&ctx);
    app.window().on_close_requested(move || {
        // 通知 SSH 线程优雅关闭
        let ctx = ctx_close.borrow();
        if let Some(bridge) = ctx.bridge.as_ref() {
            bridge.send_shutdown();
        }
        CloseRequestResponse::HideWindow
    });

    // ---- 11. 定时器：驱动 render::tick（drain SSH + poll_frame + 渲染 + resize 检测） ----
    // resize 检测已合并到 render::tick 中（每 16ms 检测窗口 size 变化），无需独立 200ms 定时器。
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

    // ---- 12. 运行 ----
    // 必须用真实绑定持有 Timer，不能用 `let _ = ...`，否则在 let 语句结束时立即 drop
    let _timer_holders = (timer,);
    app.run()?;
    Ok(())
}
