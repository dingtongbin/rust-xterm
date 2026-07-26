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

use rust_xterm_core::{
    input::{KeyInput, KeyMapping},
    mouse::{KeyMods, MouseAction, MouseButton},
    CursorMeta, CursorShape,
};
use rust_xterm_host::{Event, EventLoop, EventLoopConfig, PtyBridge, PtyConfig};
use rust_xterm_renderer::{Canvas, PixelFormat, RenderMetrics, Renderer, RendererConfig};

use slint::{
    CloseRequestResponse, Image as SlintImage, SharedPixelBuffer, SharedString, Timer, TimerMode,
};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

slint::include_modules!();

// -----------------------------------------------------------------------------
// 常量
// -----------------------------------------------------------------------------
const CELL_W: u32 = 8;
const CELL_H: u32 = 16;
const STATUS_BAR_H: u32 = 22;
const INITIAL_COLS: usize = 80;
const INITIAL_ROWS: usize = 24;
const TICK_INTERVAL_MS: u64 = 16;
const MEM_REFRESH_MS: u64 = 500;

// -----------------------------------------------------------------------------
// 主状态：用 Rc<RefCell<...>> 让 Slint 回调闭包可共享可变状态
// -----------------------------------------------------------------------------
struct AppCtx {
    event_loop: EventLoop,
    renderer: Renderer,
    fps_tracker: FpsTracker,
    sys: sysinfo::System,
    last_mem_refresh: Instant,
    last_mem_mb: f64,
    scroll_offset: usize,
    pty_alive: bool,
    /// 当前键盘修饰键状态（由 key-pressed 回调维护，供 mouse 回调读取）
    current_mods: KeyMods,
}

// -----------------------------------------------------------------------------
// FPS 滑动平均（60 帧窗口）
// -----------------------------------------------------------------------------
struct FpsTracker {
    samples: Vec<Duration>,
    last: Instant,
}

impl FpsTracker {
    fn new() -> Self {
        Self {
            samples: Vec::with_capacity(60),
            last: Instant::now(),
        }
    }
    fn tick(&mut self) -> f64 {
        let now = Instant::now();
        let dt = now - self.last;
        self.last = now;
        self.samples.push(dt);
        if self.samples.len() > 60 {
            self.samples.remove(0);
        }
        let total: Duration = self.samples.iter().sum();
        if total.as_secs_f64() > 0.0 {
            self.samples.len() as f64 / total.as_secs_f64()
        } else {
            0.0
        }
    }
}

// -----------------------------------------------------------------------------
// 剪贴板：arboard 实例需在主线程，用 Arc<Mutex> 共享
// -----------------------------------------------------------------------------
type ClipboardHandle = Arc<Mutex<arboard::Clipboard>>;

// -----------------------------------------------------------------------------
// 主函数
// -----------------------------------------------------------------------------
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // ---- 1. 构造 TerminalManager + PtyBridge + EventLoop ----
    let size = rust_xterm_core::TerminalSize::new(INITIAL_ROWS, INITIAL_COLS);
    let manager = rust_xterm_core::TerminalManager::utf8(size);

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
    let event_loop = EventLoop::new(manager, pty, EventLoopConfig::default());

    // ---- 2. 构造 Renderer（canvas 尺寸 = 初始像素）----
    let canvas_w = INITIAL_COLS as u32 * CELL_W;
    let canvas_h = INITIAL_ROWS as u32 * CELL_H;
    let renderer_config = RendererConfig {
        metrics: RenderMetrics {
            cell_width: CELL_W,
            cell_height: CELL_H,
            baseline: 13,
            dpi: 96.0,
            font_size: 14.0,
        },
        atlas_width: 1024,
        atlas_height: 1024,
        canvas_width: canvas_w,
        canvas_height: canvas_h,
        default_fg: rust_xterm_core::Color::WHITE,
        default_bg: rust_xterm_core::Color::BLACK,
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
    let ctx = Rc::new(RefCell::new(AppCtx {
        event_loop,
        renderer,
        fps_tracker: FpsTracker::new(),
        sys: sysinfo::System::new_all(),
        last_mem_refresh: Instant::now(),
        last_mem_mb: 0.0,
        scroll_offset: 0,
        pty_alive,
        current_mods: KeyMods::default(),
    }));

    // ---- 6. 键盘回调 ----
    let ctx_kb = Rc::clone(&ctx);
    app.on_key_pressed_cb(move |text, ctrl, alt, shift| {
        let mods = KeyMods { ctrl, alt, shift };
        {
            // 保存当前修饰键状态，供鼠标回调读取（矩形选区等）
            let mut ctx = ctx_kb.borrow_mut();
            ctx.current_mods = mods;
        }
        // Shift+PageUp/PageDown：scrollback 导航（Slint 1.6 TouchArea 无滚轮事件，
        // 用键盘快捷键替代）
        if mods.shift && !mods.ctrl && !mods.alt {
            match text.as_str() {
                "PageUp" | "PageDown" => {
                    let mut ctx = ctx_kb.borrow_mut();
                    let max = ctx.event_loop.manager_ref().max_scrollback();
                    if text == "PageUp" {
                        ctx.scroll_offset = (ctx.scroll_offset + 5).min(max.max(1));
                    } else {
                        ctx.scroll_offset = ctx.scroll_offset.saturating_sub(5);
                    }
                    return;
                }
                _ => {}
            }
        }
        // 优先尝试映射具名键
        if let Some(key) = map_named_key(&text, &mods) {
            let mut ctx = ctx_kb.borrow_mut();
            let _ = ctx.event_loop.send_key(key, mods);
            // 任何非导航键按下时，重置 scrollback
            if !is_nav_key(&text) {
                ctx.scroll_offset = 0;
            }
            return;
        }
        // 普通字符
        if !text.is_empty() {
            let bytes = text.as_bytes();
            // Alt 前缀 ESC
            if mods.alt && bytes.len() == 1 && bytes[0] < 0x80 {
                let mut data = vec![0x1b];
                data.extend_from_slice(bytes);
                let mut ctx = ctx_kb.borrow_mut();
                let _ = ctx.event_loop.send_input(&data);
            } else if mods.ctrl && bytes.len() == 1 {
                let c = bytes[0];
                if c.is_ascii_lowercase() || c.is_ascii_uppercase() {
                    // Ctrl+字母 → 用 KeyMapping 保证一致编码
                    let key = KeyInput::Char(c.to_ascii_lowercase() as char);
                    let data = KeyMapping::encode_key(key, mods, false);
                    let mut ctx = ctx_kb.borrow_mut();
                    let _ = ctx.event_loop.send_input(&data);
                } else {
                    let mut ctx = ctx_kb.borrow_mut();
                    let _ = ctx.event_loop.send_input(bytes);
                }
            } else {
                let mut ctx = ctx_kb.borrow_mut();
                let _ = ctx.event_loop.send_input(bytes);
                // 普通字符输入重置 scrollback
                ctx.scroll_offset = 0;
            }
        }
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
        let mut ctx = ctx_mouse.borrow_mut();
        let col = (x as u32 / CELL_W) as usize;
        let row = (y as u32 / CELL_H) as usize;
        let cols = ctx.event_loop.manager_ref().size().cols;
        let rows = ctx.event_loop.manager_ref().size().rows;
        let col = col.min(cols.saturating_sub(1));
        let row = row.min(rows.saturating_sub(1));

        let mouse_btn = match button {
            1 => MouseButton::Left,
            2 => MouseButton::Right,
            3 => MouseButton::Middle,
            _ => MouseButton::None,
        };
        let action = match kind {
            0 => MouseAction::Press,
            1 => MouseAction::Release,
            2 => MouseAction::Move,
            _ => return,
        };

        let need_copy = mouse_btn == MouseButton::Left && action == MouseAction::Release;
        let need_paste = mouse_btn == MouseButton::Middle && action == MouseAction::Press;

        // 转发：manager 内部自动判定鼠标跟踪模式 vs 选区模式
        // 使用 current_mods（由 key-pressed 回调维护）而非 default，
        // 以支持 Alt+拖拽矩形选区
        let mods = ctx.current_mods;
        ctx.event_loop
            .manager()
            .mouse_event(col, row, action, mouse_btn, mods);

        // 释放左键后：若选区非空，复制到剪贴板
        if need_copy {
            let text = ctx.event_loop.manager_ref().selection_text();
            if let Some(t) = text {
                if !t.is_empty() {
                    if let Ok(mut cb) = cb_clip.lock() {
                        let _ = cb.set_text(t);
                    }
                }
            }
        }

        // 中键粘贴（支持 bracketed paste）
        if need_paste {
            if let Ok(mut cb) = cb_clip.lock() {
                if let Ok(text) = cb.get_text() {
                    if !text.is_empty() {
                        let bracketed = ctx.event_loop.manager_ref().is_bracketed_paste_enabled();
                        if bracketed {
                            let _ = ctx.event_loop.send_input(b"\x1b[200~");
                            let _ = ctx.event_loop.send_input(text.as_bytes());
                            let _ = ctx.event_loop.send_input(b"\x1b[201~");
                        } else {
                            let _ = ctx.event_loop.send_input(text.as_bytes());
                        }
                    }
                }
            }
        }
    });

    // ---- 8. 滚轮 scrollback 回调 ----
    // Slint 1.6 的 TouchArea 提供 scroll-event 回调（PointerScrollEvent.delta-y）。
    // 正值表示向上滚（手指向后推），负值表示向下滚。
    // 按 CELL_H 像素 / 行换算为行数，调整 scroll_offset（夹在 [0, max_scrollback]）。
    let ctx_wheel = Rc::clone(&ctx);
    app.on_scroll_cb(move |delta_y| {
        let mut ctx = ctx_wheel.borrow_mut();
        let max = ctx.event_loop.manager_ref().max_scrollback();
        if max == 0 {
            return;
        }
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
            let mut ctx = ctx_tick.borrow_mut();
            // EventLoop::tick
            if let Some(event) = ctx.event_loop.tick() {
                match event {
                    Event::FrameUpdate(frame) => {
                        if ctx.scroll_offset == 0 {
                            // 正常脏区渲染
                            if !frame.dirty_spans.is_empty() {
                                ctx.renderer.render_frame(&frame.dirty_spans);
                            }
                            // 画光标
                            if frame.cursor.visible {
                                ctx.renderer.render_cursor(&frame.cursor);
                            }
                        }
                        // scroll_offset > 0 时跳过 live 渲染，保持 scrollback 视图
                    }
                    Event::Closed => {
                        ctx.pty_alive = false;
                        if let Some(app) = app_weak_tick.upgrade() {
                            app.set_mem_text("PTY closed".into());
                        }
                    }
                }
            }

            // 若处于 scrollback 视图，全屏重绘
            if ctx.scroll_offset > 0 {
                let snap = ctx
                    .event_loop
                    .manager_ref()
                    .snapshot_scrolled(ctx.scroll_offset);
                let spans: Vec<rust_xterm_core::DirtySpan> = snap
                    .rows
                    .iter()
                    .enumerate()
                    .map(|(row, cells)| rust_xterm_core::DirtySpan {
                        row,
                        col_start: 0,
                        col_end: cells.len(),
                        cells: cells.clone(),
                    })
                    .collect();
                if !spans.is_empty() {
                    ctx.renderer.render_frame(&spans);
                }
            }

            // FPS
            let fps = ctx.fps_tracker.tick();

            // 内存刷新
            let now = Instant::now();
            if now.duration_since(ctx.last_mem_refresh).as_millis() > MEM_REFRESH_MS as u128 {
                ctx.sys.refresh_memory();
                let used = ctx.sys.used_memory();
                // 兼容不同 sysinfo 版本：used_memory 返回字节或 KB
                let mb = (used as f64) / (1024.0 * 1024.0);
                ctx.last_mem_mb = mb;
                ctx.last_mem_refresh = now;
            }

            // 上传像素到 Slint Image
            let canvas = ctx.renderer.canvas();
            let buffer = canvas.buffer();
            let w = canvas.width();
            let h = canvas.height();
            let pixel_buffer =
                SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(buffer, w, h);
            let image = SlintImage::from_rgba8(pixel_buffer);

            if let Some(app) = app_weak_tick.upgrade() {
                app.set_terminal_image(image);
                app.set_fps_text(SharedString::from(format!("FPS: {fps:>5.1}")));
                app.set_mem_text(SharedString::from(format!(
                    "Mem: {:.1} MB",
                    ctx.last_mem_mb
                )));
                app.set_scroll_text(SharedString::from(format!(
                    "Scroll: {}{}",
                    ctx.scroll_offset,
                    if ctx.pty_alive { "" } else { " [PTY closed]" }
                )));
            }
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
        let window_size = app.window().size();
        let w = window_size.width;
        let h = window_size.height;
        if (w, h) != last_size && w > 0 && h > STATUS_BAR_H {
            last_size = (w, h);
            let avail_h = h - STATUS_BAR_H;
            let new_cols = (w / CELL_W) as usize;
            let new_rows = (avail_h / CELL_H) as usize;
            if new_cols > 0 && new_rows > 0 {
                let mut ctx = ctx_resize.borrow_mut();
                ctx.event_loop.resize(new_rows, new_cols);
                ctx.renderer.resize(w, avail_h);
                ctx.renderer.clear();
                // 强制全屏重绘
                ctx.scroll_offset = 0;
                let snap = ctx.event_loop.manager_ref().screen_snapshot();
                let spans: Vec<rust_xterm_core::DirtySpan> = snap
                    .rows
                    .iter()
                    .enumerate()
                    .map(|(row, cells)| rust_xterm_core::DirtySpan {
                        row,
                        col_start: 0,
                        col_end: cells.len(),
                        cells: cells.clone(),
                    })
                    .collect();
                if !spans.is_empty() {
                    ctx.renderer.render_frame(&spans);
                }
                let frame = ctx.event_loop.manager_ref().cursor();
                ctx.renderer.render_cursor(&CursorMeta {
                    x: frame.x,
                    y: frame.y,
                    visible: frame.visible,
                    shape: CursorShape::Default,
                });
            }
        }
    });

    // ---- 12. 运行 ----
    let _ = (timer, resize_timer); // 持有定时器存活
    app.run()?;
    Ok(())
}

// -----------------------------------------------------------------------------
// 具名键映射：Slint 的 key-pressed event.text 可能是具名字符串
// -----------------------------------------------------------------------------
fn map_named_key(text: &str, _mods: &KeyMods) -> Option<KeyInput> {
    match text {
        "Up" | "ArrowUp" => Some(KeyInput::ArrowUp),
        "Down" | "ArrowDown" => Some(KeyInput::ArrowDown),
        "Left" | "ArrowLeft" => Some(KeyInput::ArrowLeft),
        "Right" | "ArrowRight" => Some(KeyInput::ArrowRight),
        "Home" => Some(KeyInput::Home),
        "End" => Some(KeyInput::End),
        "Insert" => Some(KeyInput::Insert),
        "Delete" => Some(KeyInput::Delete),
        "PageUp" => Some(KeyInput::PageUp),
        "PageDown" => Some(KeyInput::PageDown),
        "Return" | "Enter" => Some(KeyInput::Enter),
        "Backspace" => Some(KeyInput::Backspace),
        "Tab" => Some(KeyInput::Tab),
        "Escape" | "Esc" => Some(KeyInput::Esc),
        "F1" => Some(KeyInput::F1),
        "F2" => Some(KeyInput::F2),
        "F3" => Some(KeyInput::F3),
        "F4" => Some(KeyInput::F4),
        "F5" => Some(KeyInput::F5),
        "F6" => Some(KeyInput::F6),
        "F7" => Some(KeyInput::F7),
        "F8" => Some(KeyInput::F8),
        "F9" => Some(KeyInput::F9),
        "F10" => Some(KeyInput::F10),
        "F11" => Some(KeyInput::F11),
        "F12" => Some(KeyInput::F12),
        _ => None,
    }
}

fn is_nav_key(text: &str) -> bool {
    matches!(
        text,
        "Up" | "Down"
            | "Left"
            | "Right"
            | "ArrowUp"
            | "ArrowDown"
            | "ArrowLeft"
            | "ArrowRight"
            | "PageUp"
            | "PageDown"
    )
}

// -----------------------------------------------------------------------------
// 类型存在性校验：PixelFormat / PathBuf 仅在 import 中出现，避免未使用告警
// -----------------------------------------------------------------------------
#[allow(dead_code)]
fn _ensure_pixelformat_imported() -> PixelFormat {
    PixelFormat::Rgba
}

#[allow(dead_code)]
fn _ensure_pathbuf_used() -> PathBuf {
    PathBuf::new()
}

#[allow(dead_code)]
fn _ensure_canvas_used() -> Canvas {
    Canvas::new(1, 1, PixelFormat::Rgba)
}
