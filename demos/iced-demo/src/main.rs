//! iced + rust-xterm 终端 demo
//!
//! 在 iced 窗口中嵌入 rust-xterm 终端：
//! - spawn 默认 shell 并通过 PTY 桥接驱动 [`EventLoop`]
//! - 使用 [`Renderer`] 将终端 RGBA 像素绘制到 iced [`image::Handle`]
//! - 键盘：普通字符走 `send_input`，方向键/功能键/Ctrl/Alt 走 [`KeyMapping::encode_key`]
//! - 鼠标：左键拖拽选区、释放复制到剪贴板（[`arboard`]）、滚轮 scrollback、中键粘贴
//! - 窗口 resize → `EventLoop::resize` + `Renderer::resize`
//! - 底部状态栏：FPS 滑动平均 + 内存（[`sysinfo`]）
//!
//! 不做：多标签页。

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use arboard::Clipboard;
use iced::keyboard::key::Named;
use iced::keyboard::{on_key_press, Key, Modifiers};
use iced::mouse::ScrollDelta;
use iced::time;
use iced::widget::{container, image, mouse_area, text, Column};
use iced::window;
use iced::{application, Color, Element, Fill, Length, Size, Subscription, Task, Theme};
use rust_xterm_core::{KeyInput, KeyMods, MouseAction, MouseButton, TerminalManager, TerminalSize};
use rust_xterm_host::{Event, EventLoop, EventLoopConfig, PtyBridge, PtyConfig};
use rust_xterm_renderer::{RenderMetrics, Renderer, RendererConfig};
use sysinfo::System;

/// 终端单元格像素尺寸（与 [`RenderMetrics`] 对齐）
const CELL_W: u32 = 10;
const CELL_H: u32 = 20;
const BASELINE: u32 = 16;
const FONT_SIZE: f32 = 16.0;
/// 状态栏高度（逻辑点）
const STATUS_BAR_HEIGHT: f32 = 26.0;
/// FPS 滑动平均窗口
const FPS_WINDOW: usize = 60;
/// 系统信息刷新间隔
const SYS_REFRESH_INTERVAL: Duration = Duration::from_millis(500);
/// EventLoop 轮询间隔（~60fps）
const POLL_INTERVAL: Duration = Duration::from_millis(16);
/// 初始终端尺寸
const INIT_COLS: u16 = 80;
const INIT_ROWS: u16 = 24;

fn main() -> iced::Result {
    application(App::title, App::update, App::view)
        .theme(|_app: &App| Theme::Dark)
        .subscription(App::subscription)
        .window(iced::window::Settings {
            size: Size::new(960.0, 640.0),
            min_size: Some(Size::new(320.0, 240.0)),
            ..Default::default()
        })
        .run_with(App::new)
}

/// 应用状态
struct App {
    event_loop: EventLoop,
    renderer: Renderer,
    /// 当前显示的图像 Handle（来自 renderer.canvas() 的 RGBA 像素）
    image_handle: Option<image::Handle>,
    /// 窗口标题（OSC 0/2）
    title: String,
    /// PTY 是否仍然存活
    pty_alive: bool,
    /// FPS 滑动平均追踪器
    fps: FpsTracker,
    /// 系统信息
    sys: System,
    /// 当前进程内存（字节）
    mem_used: u64,
    /// 系统总内存（字节）
    mem_total: u64,
    /// 上次 sysinfo 刷新时间
    last_sys_refresh: Instant,
    /// 当前鼠标位置（相对 image widget 左上角）
    cursor_pos: iced::Point,
    /// 滚动偏移（行数，0 = 实时可视窗口）
    scroll_offset: usize,
    /// 上次渲染的画布尺寸（像素）
    last_canvas: (u32, u32),
    /// 剪贴板（可选，初始化失败则为 None）
    clipboard: Option<Clipboard>,
    /// 是否请求退出
    should_exit: bool,
}

/// FPS 滑动平均追踪器
struct FpsTracker {
    samples: VecDeque<f32>,
    last: Instant,
    avg: f32,
}

impl FpsTracker {
    fn new() -> Self {
        Self {
            samples: VecDeque::with_capacity(FPS_WINDOW),
            last: Instant::now(),
            avg: 0.0,
        }
    }

    /// 记录一次帧渲染，返回当前滑动平均 FPS
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
            let sum: f32 = self.samples.iter().sum();
            self.avg = sum / self.samples.len() as f32;
        }
        self.avg
    }
}

#[derive(Debug, Clone)]
enum Message {
    /// EventLoop 轮询：drain PTY + poll_frame
    Poll,
    /// 键盘按键（来自 `keyboard::on_key_press`）
    KeyPressed(Key, Modifiers),
    /// 窗口 resize
    Resize(Size),
    /// 鼠标移动（point 相对 image widget）
    MouseMove(iced::Point),
    /// 鼠标左键按下
    MousePress,
    /// 鼠标左键释放（同时触发选区复制到剪贴板）
    MouseRelease,
    /// 鼠标中键按下（粘贴剪贴板）
    MiddlePress,
    /// 滚轮
    Scroll(ScrollDelta),
    /// sysinfo 刷新
    SysRefresh(Instant),
    /// 退出应用
    Exit,
}

impl App {
    fn new() -> (Self, Task<Message>) {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());

        let mgr = TerminalManager::utf8(TerminalSize::new(INIT_ROWS as usize, INIT_COLS as usize));
        let pty_cfg = PtyConfig {
            shell: shell.clone(),
            cols: INIT_COLS,
            rows: INIT_ROWS,
            cwd: None,
        };
        let (pty, pty_alive) = match PtyBridge::new(&pty_cfg) {
            Ok(p) => (Some(p), true),
            Err(e) => {
                eprintln!("failed to spawn PTY shell '{shell}': {e}");
                (None, false)
            }
        };

        let event_loop = EventLoop::new(mgr, pty, EventLoopConfig::default());

        let metrics = RenderMetrics {
            cell_width: CELL_W,
            cell_height: CELL_H,
            baseline: BASELINE,
            dpi: 96.0,
            font_size: FONT_SIZE,
        };
        let canvas_w = INIT_COLS as u32 * metrics.cell_width;
        let canvas_h = INIT_ROWS as u32 * metrics.cell_height;
        let renderer_cfg = RendererConfig {
            metrics,
            atlas_width: 1024,
            atlas_height: 1024,
            canvas_width: canvas_w,
            canvas_height: canvas_h,
            default_fg: rust_xterm_core::Color::WHITE,
            default_bg: rust_xterm_core::Color::BLACK,
        };
        let mut renderer = Renderer::new(renderer_cfg);
        renderer.clear();

        let clipboard = match Clipboard::new() {
            Ok(c) => Some(c),
            Err(e) => {
                eprintln!("clipboard unavailable: {e}");
                None
            }
        };

        let now = Instant::now();
        let app = App {
            event_loop,
            renderer,
            image_handle: None,
            title: "iced-demo".to_string(),
            pty_alive,
            fps: FpsTracker::new(),
            sys: System::new(),
            mem_used: 0,
            mem_total: 0,
            last_sys_refresh: now,
            cursor_pos: iced::Point::ORIGIN,
            scroll_offset: 0,
            last_canvas: (canvas_w, canvas_h),
            clipboard,
            should_exit: false,
        };
        (app, Task::none())
    }

    fn title(&self) -> String {
        format!("iced-demo - {}", self.title)
    }

    fn update(&mut self, msg: Message) -> Task<Message> {
        match msg {
            Message::Poll => {
                if self.pty_alive {
                    // 限制每 tick 最多处理 8 帧避免主线程卡死
                    for _ in 0..8 {
                        match self.event_loop.tick() {
                            Some(Event::FrameUpdate(frame)) => {
                                self.handle_frame(frame);
                            }
                            Some(Event::Closed) => {
                                self.pty_alive = false;
                                return Task::perform(async {}, |_| Message::Exit);
                            }
                            None => break,
                        }
                    }
                }
            }
            Message::KeyPressed(key, mods) => {
                let keymods = KeyMods {
                    shift: mods.shift(),
                    alt: mods.alt(),
                    ctrl: mods.control(),
                };
                if let Some(input) = map_key(&key) {
                    let _ = self.event_loop.send_key(input, keymods);
                } else if let Key::Character(s) = &key {
                    // 普通字符直接走 send_input（KeyMapping 也会处理，
                    // 但走 send_input 可保留多字节 UTF-8 字符）
                    if mods.control() || mods.alt() {
                        if let Some(input) = map_key(&key) {
                            let _ = self.event_loop.send_key(input, keymods);
                        }
                    } else if let Some(ch) = s.chars().next() {
                        let mut buf = [0u8; 4];
                        let s = ch.encode_utf8(&mut buf);
                        let _ = self.event_loop.send_input(s.as_bytes());
                    }
                }
            }
            Message::Resize(size) => {
                let metrics = self.renderer.metrics();
                // 减去状态栏高度
                let avail_h = (size.height - STATUS_BAR_HEIGHT).max(metrics.cell_height as f32);
                let new_cols = ((size.width as u32) / metrics.cell_width).max(1) as usize;
                let new_rows = (avail_h as u32 / metrics.cell_height).max(1) as usize;
                self.event_loop.resize(new_rows, new_cols);
                let new_w = new_cols as u32 * metrics.cell_width;
                let new_h = new_rows as u32 * metrics.cell_height;
                if (new_w, new_h) != self.last_canvas {
                    self.renderer.resize(new_w, new_h);
                    self.renderer.clear();
                    self.event_loop.manager().invalidate();
                    self.last_canvas = (new_w, new_h);
                    self.scroll_offset = 0;
                    // 立即重绘一帧
                    let snap = self.event_loop.manager_ref().screen_snapshot();
                    for (y, row) in snap.rows.iter().enumerate() {
                        self.renderer.render_row(y as u32, row);
                    }
                    self.flush_image();
                }
            }
            Message::MouseMove(p) => {
                self.cursor_pos = p;
                if self.scroll_offset == 0 {
                    let (x, y) = self.point_to_cell(p);
                    self.event_loop.manager().mouse_event(
                        x,
                        y,
                        MouseAction::Move,
                        MouseButton::Left,
                        KeyMods::default(),
                    );
                }
            }
            Message::MousePress => {
                if self.scroll_offset == 0 {
                    let (x, y) = self.point_to_cell(self.cursor_pos);
                    self.event_loop.manager().mouse_event(
                        x,
                        y,
                        MouseAction::Press,
                        MouseButton::Left,
                        KeyMods::default(),
                    );
                }
            }
            Message::MouseRelease => {
                if self.scroll_offset == 0 {
                    let (x, y) = self.point_to_cell(self.cursor_pos);
                    self.event_loop.manager().mouse_event(
                        x,
                        y,
                        MouseAction::Release,
                        MouseButton::Left,
                        KeyMods::default(),
                    );
                    // 选区文本复制到剪贴板
                    if let Some(text) = self.event_loop.manager_ref().selection_text() {
                        if let Some(cb) = self.clipboard.as_mut() {
                            if let Err(e) = cb.set_text(text) {
                                eprintln!("clipboard set_text failed: {e}");
                            }
                        }
                    }
                }
            }
            Message::MiddlePress => {
                if let Some(cb) = self.clipboard.as_mut() {
                    if let Ok(text) = cb.get_text() {
                        // bracketed paste 包裹（如果终端启用了）
                        let payload = if self.event_loop.manager_ref().is_bracketed_paste_enabled()
                        {
                            format!("\x1b[2004{text}\x1b[2014")
                        } else {
                            text
                        };
                        let _ = self.event_loop.send_input(payload.as_bytes());
                    }
                }
            }
            Message::Scroll(delta) => {
                let lines = match delta {
                    ScrollDelta::Lines { y, .. } => y as i32,
                    ScrollDelta::Pixels { y, .. } => (y / (CELL_H as f32)) as i32,
                };
                if lines == 0 {
                    return Task::none();
                }
                let max_sb = self.event_loop.manager_ref().max_scrollback();
                let new_offset = if lines > 0 {
                    self.scroll_offset
                        .saturating_add(lines as usize)
                        .min(max_sb)
                } else {
                    self.scroll_offset.saturating_sub((-lines) as usize)
                };
                if new_offset != self.scroll_offset {
                    self.scroll_offset = new_offset;
                    self.rerender_scrolled();
                }
            }
            Message::SysRefresh(_now) => {
                self.sys.refresh_memory();
                self.mem_used = self.sys.used_memory();
                self.mem_total = self.sys.total_memory();
                self.last_sys_refresh = Instant::now();
            }
            Message::Exit => {
                self.should_exit = true;
                return iced::exit();
            }
        }
        Task::none()
    }

    /// 处理一帧 PTY 数据 + 渲染
    fn handle_frame(&mut self, frame: rust_xterm_core::FrameUpdate) {
        // 滚动中：跳过渲染（用户看到的是 scrollback 视图）
        if self.scroll_offset > 0 {
            // 但仍要更新 title
            let new_title = self.event_loop.manager_ref().title();
            if new_title != self.title {
                self.title = new_title;
            }
            return;
        }

        for row in &frame.dirty_cells {
            self.renderer.render_row(row.y as u32, &row.cells);
        }
        // 渲染光标
        self.renderer.render_cursor(&frame.cursor);
        // 更新 FPS
        let fps = self.fps.tick();
        // 更新 title
        let new_title = self.event_loop.manager_ref().title();
        if new_title != self.title {
            self.title = new_title;
        }
        // 刷新图像 Handle
        self.flush_image();
        // 用 fps 防止 unused 警告（实际显示在 view 中读取 self.fps.avg）
        let _ = fps;
    }

    /// 把 renderer 的画布像素刷到 image_handle
    fn flush_image(&mut self) {
        let canvas = self.renderer.canvas();
        let pixels = canvas.buffer().to_vec();
        self.image_handle = Some(image::Handle::from_rgba(
            canvas.width(),
            canvas.height(),
            pixels,
        ));
    }

    /// 滚动偏移变更后，从 scrollback 快照重绘整个画布
    fn rerender_scrolled(&mut self) {
        let snap = self
            .event_loop
            .manager_ref()
            .snapshot_scrolled(self.scroll_offset);
        // 先清屏
        self.renderer.clear();
        for (y, row) in snap.rows.iter().enumerate() {
            self.renderer.render_row(y as u32, row);
        }
        self.flush_image();
    }

    /// 把鼠标像素坐标转换为终端 (col, row)
    fn point_to_cell(&self, p: iced::Point) -> (usize, usize) {
        let metrics = self.renderer.metrics();
        let x = ((p.x.max(0.0) as u32) / metrics.cell_width) as usize;
        let y = ((p.y.max(0.0) as u32) / metrics.cell_height) as usize;
        (x, y)
    }

    fn view(&self) -> Element<'_, Message> {
        let img_widget = if let Some(h) = &self.image_handle {
            image(h)
                .width(Length::Fill)
                .height(Length::Fill)
                .filter_method(iced::widget::image::FilterMethod::Nearest)
        } else {
            // 占位：1x1 黑色像素，等首个 tick 到来
            image(image::Handle::from_rgba(1, 1, vec![0, 0, 0, 255]))
                .width(Length::Fill)
                .height(Length::Fill)
        };

        let term_area = mouse_area(img_widget)
            .on_press(Message::MousePress)
            .on_release(Message::MouseRelease)
            .on_middle_press(Message::MiddlePress)
            .on_move(Message::MouseMove)
            .on_scroll(Message::Scroll);

        let status_text = format!(
            "FPS: {:5.1}  |  Mem: {:6.1} / {:6.1} MB  |  Scroll: +{}  |  PTY: {}  |  ESC/Ctrl+C/Ctrl+D → shell",
            self.fps.avg,
            self.mem_used as f64 / 1_048_576.0,
            self.mem_total as f64 / 1_048_576.0,
            self.scroll_offset,
            if self.pty_alive { "alive" } else { "closed" },
        );

        let status_bar = container(text(status_text).size(13))
            .width(Fill)
            .height(STATUS_BAR_HEIGHT)
            .padding([4, 8])
            .style(|_theme: &Theme| {
                container::Style::default()
                    .background(Color::from_rgb8(0x1e, 0x1e, 0x1e))
                    .color(Color::from_rgb8(0xcc, 0xcc, 0xcc))
            });

        Column::new()
            .width(Fill)
            .height(Fill)
            .push(term_area)
            .push(status_bar)
            .into()
    }

    fn subscription(&self) -> Subscription<Message> {
        let poll = time::every(POLL_INTERVAL).map(|_| Message::Poll);
        let sys = time::every(SYS_REFRESH_INTERVAL).map(Message::SysRefresh);
        let keys = on_key_press(|key, mods| Some(Message::KeyPressed(key, mods)));
        let resize = window::resize_events().map(|(_id, size)| Message::Resize(size));
        Subscription::batch(vec![poll, sys, keys, resize])
    }
}

/// 把 iced [`Key`] 映射为 rust-xterm [`KeyInput`]。
///
/// 仅方向键、功能键、编辑键等具名键走此映射；
/// 普通字符在调用方走 `send_input` 以保留多字节 UTF-8。
fn map_key(key: &Key) -> Option<KeyInput> {
    let named = match key.as_ref() {
        Key::Named(n) => n,
        Key::Character(_) => return None,
        Key::Unidentified => return None,
    };
    Some(match named {
        Named::ArrowUp => KeyInput::ArrowUp,
        Named::ArrowDown => KeyInput::ArrowDown,
        Named::ArrowLeft => KeyInput::ArrowLeft,
        Named::ArrowRight => KeyInput::ArrowRight,
        Named::Home => KeyInput::Home,
        Named::End => KeyInput::End,
        Named::Insert => KeyInput::Insert,
        Named::Delete => KeyInput::Delete,
        Named::PageUp => KeyInput::PageUp,
        Named::PageDown => KeyInput::PageDown,
        Named::Enter => KeyInput::Enter,
        Named::Backspace => KeyInput::Backspace,
        Named::Tab => KeyInput::Tab,
        Named::Escape => KeyInput::Esc,
        Named::F1 => KeyInput::F1,
        Named::F2 => KeyInput::F2,
        Named::F3 => KeyInput::F3,
        Named::F4 => KeyInput::F4,
        Named::F5 => KeyInput::F5,
        Named::F6 => KeyInput::F6,
        Named::F7 => KeyInput::F7,
        Named::F8 => KeyInput::F8,
        Named::F9 => KeyInput::F9,
        Named::F10 => KeyInput::F10,
        Named::F11 => KeyInput::F11,
        Named::F12 => KeyInput::F12,
        _ => return None,
    })
}
