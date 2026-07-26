//! egui + rust-xterm 终端 demo
//!
//! 在 eframe 窗口中嵌入 rust-xterm 终端：
//! - spawn 默认 shell 并通过 PTY 桥接驱动 [`EventLoop`]
//! - 使用 [`Renderer`] 将终端 RGBA 像素绘制到 egui [`TextureHandle`]
//! - 键盘：普通字符走 `send_input`，方向键/功能键/Ctrl/Alt 走 [`KeyMapping::encode_key`]
//! - 鼠标：左键拖拽选区、释放复制到剪贴板（[`arboard`]）、滚轮 scrollback、中键粘贴
//! - 窗口 resize → `EventLoop::resize` + `Renderer::resize`
//! - 底部状态栏：FPS 滑动平均 + 内存（[`sysinfo`]）
//!
//! 不做：多标签页。

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use arboard::Clipboard;
use eframe::egui;
use egui::{ColorImage, TextureHandle, TextureOptions};
use rust_xterm_core::{
    Color, CursorMeta, KeyInput, KeyMods, MouseAction, MouseButton, TerminalManager, TerminalSize,
};
use rust_xterm_host::{Event, EventLoop, EventLoopConfig, PtyBridge, PtyConfig};
use rust_xterm_renderer::{RenderMetrics, Renderer, RendererConfig};
use sysinfo::{Pid, System};

// 终端单元格像素尺寸（与 RenderMetrics 对齐）
const CELL_W: u32 = 10;
const CELL_H: u32 = 20;
const BASELINE: u32 = 16;
const FONT_SIZE: f32 = 16.0;
/// 状态栏高度（逻辑点）
const STATUS_BAR_HEIGHT: f32 = 26.0;
/// 面板内边距预留
const PANEL_PADDING: f32 = 8.0;
/// FPS 滑动平均窗口
const FPS_WINDOW: usize = 60;
/// 系统信息刷新间隔
const SYS_REFRESH_INTERVAL: Duration = Duration::from_millis(500);

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("egui + rust-xterm terminal demo")
            .with_inner_size([960.0, 640.0])
            .with_min_inner_size([320.0, 240.0]),
        ..Default::default()
    };
    eframe::run_native(
        "egui + rust-xterm terminal demo",
        options,
        Box::new(|cc| Ok(Box::new(TerminalApp::new(cc)))),
    )
}

/// FPS 滑动平均追踪器
struct FpsTracker {
    samples: VecDeque<f32>,
    last: Instant,
}

impl FpsTracker {
    fn new() -> Self {
        Self {
            samples: VecDeque::with_capacity(FPS_WINDOW),
            last: Instant::now(),
        }
    }

    fn tick(&mut self) -> f32 {
        let now = Instant::now();
        let dt = now.duration_since(self.last).as_secs_f32();
        self.last = now;
        if dt > 0.0 {
            let fps = 1.0 / dt;
            if self.samples.len() == FPS_WINDOW {
                self.samples.pop_front();
            }
            self.samples.push_back(fps);
        }
        let sum: f32 = self.samples.iter().sum();
        if sum > 0.0 {
            sum / self.samples.len() as f32
        } else {
            0.0
        }
    }
}

/// egui App 主体
struct TerminalApp {
    event_loop: EventLoop,
    renderer: Renderer,
    texture: Option<TextureHandle>,
    cols: usize,
    rows: usize,
    last_cursor: Option<CursorMeta>,
    scroll_offset: usize,
    texture_dirty: bool,
    need_full_redraw: bool,
    fps: FpsTracker,
    sys: System,
    pid: Pid,
    sys_last_refresh: Instant,
    last_mem_mb: f64,
    clipboard: Option<Clipboard>,
    pty_alive: bool,
    last_window_size: egui::Vec2,
}

impl TerminalApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let cols = 80usize;
        let rows = 24usize;
        let canvas_w = cols as u32 * CELL_W;
        let canvas_h = rows as u32 * CELL_H;

        // 终端管理器 + PTY
        let manager = TerminalManager::utf8(TerminalSize::new(rows, cols));
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
        let pty_cfg = PtyConfig {
            shell: shell.clone(),
            cols: cols as u16,
            rows: rows as u16,
            cwd: None,
        };
        let (pty, pty_alive) = match PtyBridge::new(&pty_cfg) {
            Ok(p) => (Some(p), true),
            Err(e) => {
                eprintln!("failed to spawn PTY shell '{shell}': {e}");
                (None, false)
            }
        };
        let mut event_loop = EventLoop::new(manager, pty, EventLoopConfig::default());

        if !pty_alive {
            let msg = format!(
                "\r\n\x1b[31m[egui-demo] Failed to spawn PTY shell '{shell}'.\r\n\
                 Keyboard input still works (echo only); no PTY output.\x1b[0m\r\n"
            );
            event_loop.manager().write(msg.as_bytes());
            event_loop.manager().write(b"\r\n$ ");
        }

        // 渲染器
        let renderer_cfg = RendererConfig {
            metrics: RenderMetrics {
                cell_width: CELL_W,
                cell_height: CELL_H,
                baseline: BASELINE,
                dpi: 96.0,
                font_size: FONT_SIZE,
            },
            atlas_width: 1024,
            atlas_height: 1024,
            canvas_width: canvas_w,
            canvas_height: canvas_h,
            default_fg: Color::WHITE,
            default_bg: Color::BLACK,
        };
        let mut renderer = Renderer::new(renderer_cfg);
        renderer.clear();

        // 系统信息
        let pid = Pid::from(std::process::id() as usize);
        let mut sys = System::new();
        sys.refresh_processes();
        let last_mem_mb = sys
            .process(pid)
            .map(|p| p.memory() as f64 / 1024.0)
            .unwrap_or(0.0);

        // 剪贴板
        let clipboard = match Clipboard::new() {
            Ok(c) => Some(c),
            Err(e) => {
                eprintln!("clipboard unavailable: {e}");
                None
            }
        };

        Self {
            event_loop,
            renderer,
            texture: None,
            cols,
            rows,
            last_cursor: None,
            scroll_offset: 0,
            texture_dirty: true,
            need_full_redraw: true,
            fps: FpsTracker::new(),
            sys,
            pid,
            sys_last_refresh: Instant::now(),
            last_mem_mb,
            clipboard,
            pty_alive,
            last_window_size: egui::Vec2::ZERO,
        }
    }

    /// 检查窗口尺寸变化，相应调整 EventLoop / Renderer。
    fn handle_resize(&mut self, ctx: &egui::Context) {
        let avail = ctx.available_rect().size();
        if avail == self.last_window_size {
            return;
        }
        self.last_window_size = avail;
        let usable_w = (avail.x - PANEL_PADDING).max(CELL_W as f32);
        let usable_h = (avail.y - STATUS_BAR_HEIGHT - PANEL_PADDING).max(CELL_H as f32);
        let new_cols = ((usable_w / CELL_W as f32).floor() as usize).max(1);
        let new_rows = ((usable_h / CELL_H as f32).floor() as usize).max(1);
        if new_cols != self.cols || new_rows != self.rows {
            self.cols = new_cols;
            self.rows = new_rows;
            let canvas_w = new_cols as u32 * CELL_W;
            let canvas_h = new_rows as u32 * CELL_H;
            self.event_loop.resize(new_rows, new_cols);
            self.renderer.resize(canvas_w, canvas_h);
            self.renderer.clear();
            self.last_cursor = None;
            self.need_full_redraw = true;
            self.texture = None; // 尺寸变化，强制重建纹理
            self.texture_dirty = true;
        }
    }

    /// 处理键盘输入。
    ///
    /// - `Event::Text`：可打印字符（包括 CJK / Shift 符号）走 `send_input`
    /// - `Event::Key`：方向键 / 功能键 / Enter / Backspace / Tab / Esc，以及
    ///   Ctrl/Alt + 字母，走 [`KeyMapping::encode_key`]
    fn handle_keyboard(&mut self, ctx: &egui::Context) {
        let events: Vec<egui::Event> = ctx.input(|i| i.events.clone());
        for event in events {
            match event {
                egui::Event::Text(text) => {
                    if !text.is_empty() {
                        self.reset_scrollback();
                        let _ = self.event_loop.send_input(text.as_bytes());
                    }
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    let mods = KeyMods {
                        shift: modifiers.shift,
                        ctrl: modifiers.ctrl,
                        alt: modifiers.alt,
                    };
                    // 方向键 / PageUp / PageDown 不重置 scrollback（用于翻页浏览历史）
                    let is_nav_key = matches!(
                        key,
                        egui::Key::ArrowUp
                            | egui::Key::ArrowDown
                            | egui::Key::PageUp
                            | egui::Key::PageDown
                    );
                    if !is_nav_key {
                        self.reset_scrollback();
                    }
                    if (mods.ctrl || mods.alt) && !mods.shift {
                        if let Some(ch) = key_to_char(key) {
                            let _ = self.event_loop.send_key(KeyInput::Char(ch), mods);
                            continue;
                        }
                    }
                    if let Some(ki) = map_special_key(key) {
                        let _ = self.event_loop.send_key(ki, mods);
                    }
                }
                _ => {}
            }
        }
    }

    fn reset_scrollback(&mut self) {
        if self.scroll_offset > 0 {
            self.scroll_offset = 0;
            self.need_full_redraw = true;
        }
    }

    /// 处理鼠标：左键拖拽选区 + 释放复制剪贴板、滚轮 scrollback、中键粘贴。
    fn handle_mouse(&mut self, ctx: &egui::Context, img_rect: egui::Rect) {
        let (
            latest_pos,
            scroll,
            primary_pressed,
            primary_released,
            primary_down,
            middle_released,
            mods,
        ) = ctx.input(|i| {
            (
                i.pointer.latest_pos(),
                i.smooth_scroll_delta,
                i.pointer.button_pressed(egui::PointerButton::Primary),
                i.pointer.button_released(egui::PointerButton::Primary),
                i.pointer.button_down(egui::PointerButton::Primary),
                i.pointer.button_released(egui::PointerButton::Middle),
                i.modifiers,
            )
        });
        let mods = KeyMods {
            shift: mods.shift,
            ctrl: mods.ctrl,
            alt: mods.alt,
        };

        // 滚轮：scrollback 或转发给鼠标跟踪应用
        if scroll.y != 0.0 {
            let lines = (scroll.y.abs() / CELL_H as f32).round() as usize;
            if lines > 0 {
                let grabbed = self.event_loop.manager_ref().is_mouse_grabbed();
                if grabbed {
                    let action = if scroll.y > 0.0 {
                        MouseAction::WheelUp(lines as u32)
                    } else {
                        MouseAction::WheelDown(lines as u32)
                    };
                    self.event_loop
                        .manager()
                        .mouse_event(0, 0, action, MouseButton::Left, mods);
                } else {
                    let max = self.event_loop.manager_ref().max_scrollback();
                    if scroll.y > 0.0 {
                        self.scroll_offset = (self.scroll_offset + lines).min(max.max(1));
                    } else {
                        self.scroll_offset = self.scroll_offset.saturating_sub(lines);
                    }
                    self.need_full_redraw = true;
                }
            }
        }

        // 鼠标位置 → cell 坐标
        let (col, row, inside) = match latest_pos {
            Some(pos) if img_rect.contains(pos) => {
                let local = pos - img_rect.min;
                let c =
                    ((local.x / CELL_W as f32).floor() as usize).min(self.cols.saturating_sub(1));
                let r =
                    ((local.y / CELL_H as f32).floor() as usize).min(self.rows.saturating_sub(1));
                (c, r, true)
            }
            _ => (0usize, 0usize, false),
        };

        if primary_pressed && inside {
            self.event_loop.manager().mouse_event(
                col,
                row,
                MouseAction::Press,
                MouseButton::Left,
                mods,
            );
        }
        if primary_down {
            self.event_loop.manager().mouse_event(
                col,
                row,
                MouseAction::Move,
                MouseButton::Left,
                mods,
            );
        }
        if primary_released {
            self.event_loop.manager().mouse_event(
                col,
                row,
                MouseAction::Release,
                MouseButton::Left,
                mods,
            );
            // 释放后复制选区文本到剪贴板
            if let Some(text) = self.event_loop.manager_ref().selection_text() {
                if !text.is_empty() {
                    if let Some(cb) = self.clipboard.as_mut() {
                        let _ = cb.set_text(text);
                    }
                }
            }
        }
        if middle_released {
            // 中键粘贴
            if let Some(cb) = self.clipboard.as_mut() {
                if let Ok(text) = cb.get_text() {
                    if !text.is_empty() {
                        self.reset_scrollback();
                        let bracketed = self.event_loop.manager_ref().is_bracketed_paste_enabled();
                        if bracketed {
                            let _ = self.event_loop.send_input(b"\x1b[200~");
                            let _ = self.event_loop.send_input(text.as_bytes());
                            let _ = self.event_loop.send_input(b"\x1b[201~");
                        } else {
                            let _ = self.event_loop.send_input(text.as_bytes());
                        }
                    }
                }
            }
        }
    }

    /// 全量重绘整屏（用于 scrollback / resize / 首帧）。
    fn render_full_screen(&mut self) {
        if self.scroll_offset > 0 {
            let snap = self
                .event_loop
                .manager_ref()
                .snapshot_scrolled(self.scroll_offset);
            for (y, row) in snap.rows.iter().enumerate() {
                self.renderer.render_row(y as u32, row);
            }
            // scrollback 模式不绘制光标
        } else {
            let snap = self.event_loop.manager_ref().screen_snapshot();
            for (y, row) in snap.rows.iter().enumerate() {
                self.renderer.render_row(y as u32, row);
            }
            let cursor = self.event_loop.manager_ref().cursor();
            self.renderer.render_cursor(&cursor);
            self.last_cursor = Some(cursor);
        }
    }

    /// 确保 egui 纹理与画布同步（必要时重建）。
    fn upload_texture_if_dirty(&mut self, ctx: &egui::Context) {
        if self.texture.is_some() && !self.texture_dirty {
            return;
        }
        let img = {
            let canvas = self.renderer.canvas();
            ColorImage::from_rgba_unmultiplied(
                [canvas.width() as usize, canvas.height() as usize],
                canvas.buffer(),
            )
        };
        self.texture = Some(ctx.load_texture("rust-xterm-terminal", img, TextureOptions::LINEAR));
        self.texture_dirty = false;
    }

    /// 周期性刷新进程内存，返回最近一次内存值（MB）。
    fn refresh_mem(&mut self) -> f64 {
        let now = Instant::now();
        if now.duration_since(self.sys_last_refresh) >= SYS_REFRESH_INTERVAL {
            self.sys_last_refresh = now;
            self.sys.refresh_processes();
            if let Some(p) = self.sys.process(self.pid) {
                self.last_mem_mb = p.memory() as f64 / 1024.0;
            }
        }
        self.last_mem_mb
    }
}

impl eframe::App for TerminalApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let fps = self.fps.tick();

        // 1. 窗口尺寸变化 → resize
        self.handle_resize(ctx);

        // 2. 事件循环 tick
        if let Some(event) = self.event_loop.tick() {
            match event {
                Event::FrameUpdate(frame) => {
                    // 新数据到来时重置 scrollback（snap to bottom）
                    if self.scroll_offset > 0 {
                        self.scroll_offset = 0;
                        self.need_full_redraw = true;
                    }
                    // 渲染脏行
                    for dirty in &frame.dirty_cells {
                        let y = dirty.y as u32;
                        self.renderer.render_row(y, &dirty.cells);
                    }
                    // 清除旧光标位置的 ghost：重绘旧光标所在行
                    if let Some(last) = self.last_cursor {
                        if last.x != frame.cursor.x
                            || last.y != frame.cursor.y
                            || last.visible != frame.cursor.visible
                        {
                            let snap = self.event_loop.manager_ref().screen_snapshot();
                            if let Some(row_cells) = snap.rows.get(last.y) {
                                self.renderer.render_row(last.y as u32, row_cells);
                            }
                        }
                    }
                    // 渲染新光标
                    self.renderer.render_cursor(&frame.cursor);
                    self.last_cursor = Some(frame.cursor);
                    self.texture_dirty = true;
                }
                Event::Closed => {
                    self.pty_alive = false;
                }
            }
        }

        // 3. 键盘
        self.handle_keyboard(ctx);

        // 4. 全量重绘（scrollback / resize / 首帧）
        if self.scroll_offset > 0 || self.need_full_redraw {
            self.render_full_screen();
            self.need_full_redraw = false;
            self.texture_dirty = true;
        }

        // 5. 绘制 UI
        egui::CentralPanel::default()
            .frame(egui::Frame::none())
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);

                // 上传纹理
                self.upload_texture_if_dirty(ctx);

                // 终端图像（按画布像素尺寸 1:1 显示）
                let img_size = egui::vec2(
                    (self.cols as u32 * CELL_W) as f32,
                    (self.rows as u32 * CELL_H) as f32,
                );
                let img_response = {
                    let tex = self.texture.as_ref().expect("texture must exist");
                    ui.add(egui::Image::from_texture(tex).fit_to_exact_size(img_size))
                };
                let img_rect = img_response.rect;

                // 鼠标
                self.handle_mouse(ctx, img_rect);

                // 状态栏
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let mem = self.refresh_mem();
                    ui.label(format!(
                        "FPS: {fps:5.1}  Mem: {mem:6.1} MB  Cells: {}x{}",
                        self.cols, self.rows
                    ));
                    if self.scroll_offset > 0 {
                        ui.colored_label(
                            egui::Color32::LIGHT_YELLOW,
                            format!("  (scrollback: {})", self.scroll_offset),
                        );
                    }
                    if !self.pty_alive {
                        ui.colored_label(egui::Color32::RED, "  [PTY closed]");
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label("egui + rust-xterm  ·  drag-select · wheel-scroll · middle-paste");
                    });
                });
            });

        // 持续重绘以驱动 PTY 轮询与终端动画
        ctx.request_repaint();
    }
}

/// 将 egui 特殊键映射为 rust-xterm [`KeyInput`]。
fn map_special_key(key: egui::Key) -> Option<KeyInput> {
    match key {
        egui::Key::ArrowUp => Some(KeyInput::ArrowUp),
        egui::Key::ArrowDown => Some(KeyInput::ArrowDown),
        egui::Key::ArrowLeft => Some(KeyInput::ArrowLeft),
        egui::Key::ArrowRight => Some(KeyInput::ArrowRight),
        egui::Key::Home => Some(KeyInput::Home),
        egui::Key::End => Some(KeyInput::End),
        egui::Key::Insert => Some(KeyInput::Insert),
        egui::Key::Delete => Some(KeyInput::Delete),
        egui::Key::PageUp => Some(KeyInput::PageUp),
        egui::Key::PageDown => Some(KeyInput::PageDown),
        egui::Key::F1 => Some(KeyInput::F1),
        egui::Key::F2 => Some(KeyInput::F2),
        egui::Key::F3 => Some(KeyInput::F3),
        egui::Key::F4 => Some(KeyInput::F4),
        egui::Key::F5 => Some(KeyInput::F5),
        egui::Key::F6 => Some(KeyInput::F6),
        egui::Key::F7 => Some(KeyInput::F7),
        egui::Key::F8 => Some(KeyInput::F8),
        egui::Key::F9 => Some(KeyInput::F9),
        egui::Key::F10 => Some(KeyInput::F10),
        egui::Key::F11 => Some(KeyInput::F11),
        egui::Key::F12 => Some(KeyInput::F12),
        egui::Key::Enter => Some(KeyInput::Enter),
        egui::Key::Backspace => Some(KeyInput::Backspace),
        egui::Key::Tab => Some(KeyInput::Tab),
        egui::Key::Escape => Some(KeyInput::Esc),
        _ => None,
    }
}

/// 将 egui 字母键映射为小写字符（用于 Ctrl/Alt 组合键编码）。
fn key_to_char(key: egui::Key) -> Option<char> {
    match key {
        egui::Key::A => Some('a'),
        egui::Key::B => Some('b'),
        egui::Key::C => Some('c'),
        egui::Key::D => Some('d'),
        egui::Key::E => Some('e'),
        egui::Key::F => Some('f'),
        egui::Key::G => Some('g'),
        egui::Key::H => Some('h'),
        egui::Key::I => Some('i'),
        egui::Key::J => Some('j'),
        egui::Key::K => Some('k'),
        egui::Key::L => Some('l'),
        egui::Key::M => Some('m'),
        egui::Key::N => Some('n'),
        egui::Key::O => Some('o'),
        egui::Key::P => Some('p'),
        egui::Key::Q => Some('q'),
        egui::Key::R => Some('r'),
        egui::Key::S => Some('s'),
        egui::Key::T => Some('t'),
        egui::Key::U => Some('u'),
        egui::Key::V => Some('v'),
        egui::Key::W => Some('w'),
        egui::Key::X => Some('x'),
        egui::Key::Y => Some('y'),
        egui::Key::Z => Some('z'),
        _ => None,
    }
}
