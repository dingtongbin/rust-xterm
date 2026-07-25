//! rust-xterm Windows / cross-platform PTY demo (egui).
//!
//! 一个**真正可用的**终端前端：按单元格渲染真彩色前景/背景与文本样式，
//! 支持滚动回溯（滚动条 + 鼠标滚轮）、鼠标报告转发（SGR mouse）、
//! 全键盘（Ctrl+C / 方向键 / 功能键 / Home/End/Delete/PgUp/PgDn …）。
//!
//! 运行：`cargo run --example win-pty-demo`

use eframe::egui;
use rust_xterm_core::{
    CellFlags, Color, CursorShape, KeyMods, MouseAction, MouseButton, TerminalManager, TerminalSize,
};
use rust_xterm_host::{Event, EventLoop, EventLoopConfig, PtyBridge, PtyConfig};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

const FONT_SIZE: f32 = 14.0;
const STATUSBAR_H: f32 = 22.0;
const PAD: f32 = 4.0;

struct TerminalApp {
    el: Rc<RefCell<EventLoop>>,
    last_size: (f32, f32),
    /// 滚动回溯偏移（0 = 实时可视窗口）
    scroll_offset: usize,
    /// 是否跟随底部（有新输出时自动滚到底）
    follow: bool,
    closed: bool,
    /// 鼠标按下状态（用于转发 drag/move 报告）
    mouse_down: bool,
    title: String,
}

impl TerminalApp {
    fn new(el: Rc<RefCell<EventLoop>>) -> Self {
        Self {
            el,
            last_size: (760.0, 480.0),
            scroll_offset: 0,
            follow: true,
            closed: false,
            mouse_down: false,
            title: "rust-xterm - PTY Demo".to_string(),
        }
    }
}

fn color32(c: Color) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
}

/// 测量等宽字体的单元格宽度
fn measure_cell_w(ctx: &egui::Context) -> f32 {
    let galley = ctx.fonts(|f| {
        f.layout_no_wrap(
            "M".to_string(),
            egui::FontId::monospace(FONT_SIZE),
            egui::Color32::WHITE,
        )
    });
    galley.size().x.ceil().max(6.0)
}

/// 将 egui 按键编码为终端输入字节
fn encode_key(key: egui::Key, modifiers: &egui::Modifiers) -> Option<Vec<u8>> {
    if modifiers.ctrl {
        // Ctrl+字母 → 控制字符 1..26；Ctrl+Space → 0
        let seq: Option<Vec<u8>> = match key {
            egui::Key::Space => Some(vec![0]),
            egui::Key::A => Some(vec![1]),
            egui::Key::B => Some(vec![2]),
            egui::Key::C => Some(vec![3]),
            egui::Key::D => Some(vec![4]),
            egui::Key::E => Some(vec![5]),
            egui::Key::F => Some(vec![6]),
            egui::Key::G => Some(vec![7]),
            egui::Key::H => Some(vec![8]),
            egui::Key::I => Some(vec![9]),
            egui::Key::J => Some(vec![10]),
            egui::Key::K => Some(vec![11]),
            egui::Key::L => Some(vec![12]),
            egui::Key::M => Some(vec![13]),
            egui::Key::N => Some(vec![14]),
            egui::Key::O => Some(vec![15]),
            egui::Key::P => Some(vec![16]),
            egui::Key::Q => Some(vec![17]),
            egui::Key::R => Some(vec![18]),
            egui::Key::S => Some(vec![19]),
            egui::Key::T => Some(vec![20]),
            egui::Key::U => Some(vec![21]),
            egui::Key::V => Some(vec![22]),
            egui::Key::W => Some(vec![23]),
            egui::Key::X => Some(vec![24]),
            egui::Key::Y => Some(vec![25]),
            egui::Key::Z => Some(vec![26]),
            egui::Key::OpenBracket => Some(b"\x1b".to_vec()), // Ctrl+[ = ESC
            egui::Key::CloseBracket => Some(b"\x1d".to_vec()), // Ctrl+] = GS
            egui::Key::Backslash => Some(b"\x1c".to_vec()),   // Ctrl+\ = FS
            _ => None,
        };
        return seq;
    }

    match key {
        egui::Key::Enter => Some(b"\r".to_vec()),
        egui::Key::Backspace => Some(b"\x7f".to_vec()), // DEL（多数 shell 期望）
        egui::Key::Tab => Some(b"\t".to_vec()),
        egui::Key::Escape => Some(b"\x1b".to_vec()),
        egui::Key::ArrowUp => Some(b"\x1b[A".to_vec()),
        egui::Key::ArrowDown => Some(b"\x1b[B".to_vec()),
        egui::Key::ArrowRight => Some(b"\x1b[C".to_vec()),
        egui::Key::ArrowLeft => Some(b"\x1b[D".to_vec()),
        egui::Key::Home => Some(b"\x1b[H".to_vec()),
        egui::Key::End => Some(b"\x1b[F".to_vec()),
        egui::Key::Delete => Some(b"\x1b[3~".to_vec()),
        egui::Key::PageUp => Some(b"\x1b[5~".to_vec()),
        egui::Key::PageDown => Some(b"\x1b[6~".to_vec()),
        egui::Key::Insert => Some(b"\x1b[2~".to_vec()),
        egui::Key::F1 => Some(b"\x1bOP".to_vec()),
        egui::Key::F2 => Some(b"\x1bOQ".to_vec()),
        egui::Key::F3 => Some(b"\x1bOR".to_vec()),
        egui::Key::F4 => Some(b"\x1bOS".to_vec()),
        egui::Key::F5 => Some(b"\x1b[15~".to_vec()),
        egui::Key::F6 => Some(b"\x1b[17~".to_vec()),
        egui::Key::F7 => Some(b"\x1b[18~".to_vec()),
        egui::Key::F8 => Some(b"\x1b[19~".to_vec()),
        egui::Key::F9 => Some(b"\x1b[20~".to_vec()),
        egui::Key::F10 => Some(b"\x1b[21~".to_vec()),
        egui::Key::F11 => Some(b"\x1b[23~".to_vec()),
        egui::Key::F12 => Some(b"\x1b[24~".to_vec()),
        _ => None,
    }
}

fn mods_from_egui(m: &egui::Modifiers) -> KeyMods {
    KeyMods {
        shift: m.shift,
        alt: m.alt,
        ctrl: m.ctrl,
    }
}

fn button_from_egui(b: egui::PointerButton) -> MouseButton {
    match b {
        egui::PointerButton::Primary => MouseButton::Left,
        egui::PointerButton::Middle => MouseButton::Middle,
        egui::PointerButton::Secondary => MouseButton::Right,
        _ => MouseButton::None,
    }
}

impl eframe::App for TerminalApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let cell_w = measure_cell_w(ctx);
        let cell_h = (FONT_SIZE * 1.5).ceil();

        // ===================== 1. 事件循环 tick =====================
        let mut keys_to_send: Vec<Vec<u8>> = Vec::new();
        let mut mouse_events: Vec<(usize, usize, MouseAction, MouseButton, KeyMods)> = Vec::new();
        let mut new_frame = false;

        {
            let mut el = self.el.borrow_mut();
            match el.tick() {
                Some(Event::FrameUpdate(_)) => {
                    new_frame = true;
                    if self.follow {
                        self.scroll_offset = 0;
                    }
                }
                Some(Event::Closed) => self.closed = true,
                None => {}
            }

            // 窗口尺寸变化 → 重算行列并 resize
            let avail = ctx.screen_rect().size();
            let w = avail.x;
            let h = avail.y;
            if (w - self.last_size.0).abs() > 5.0 || (h - self.last_size.1).abs() > 5.0 {
                self.last_size = (w, h);
            }
        }

        // ===================== 2. 输入事件采集 =====================
        let events: Vec<egui::Event> = ctx.input(|i| i.events.clone());
        for ev in &events {
            match ev {
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    if let Some(bytes) = encode_key(*key, modifiers) {
                        keys_to_send.push(bytes);
                        self.follow = true;
                        self.scroll_offset = 0;
                    }
                }
                egui::Event::Text(text) => {
                    for ch in text.chars() {
                        if (ch as u32) <= 0x1f {
                            continue; // 控制字符已在 Key 分支处理
                        }
                        let mut buf = [0u8; 4];
                        let s = ch.encode_utf8(&mut buf);
                        keys_to_send.push(s.as_bytes().to_vec());
                        self.follow = true;
                        self.scroll_offset = 0;
                    }
                }
                _ => {}
            }
        }

        // ===================== 3. 状态栏 =====================
        let closed = self.closed;
        let grabbed = self.el.borrow().manager_ref().is_mouse_grabbed();
        egui::TopBottomPanel::bottom("status_bar")
            .frame(egui::Frame::NONE.fill(egui::Color32::from_rgb(30, 30, 30)))
            .exact_height(STATUSBAR_H)
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    let status = if closed {
                        "PTY closed — close window to exit".to_string()
                    } else if grabbed {
                        format!(
                            "Mouse captured by app (offset={}) — wheel/click forwarded",
                            self.scroll_offset
                        )
                    } else {
                        format!(
                            "rows×cols  |  scrollback offset: {}  |  Ctrl+C / arrows / F-keys / PgUp/PgDn / wheel-scroll",
                            self.scroll_offset
                        )
                    };
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(status)
                                .font(egui::FontId::monospace(11.0))
                                .color(egui::Color32::from_rgb(140, 190, 140)),
                        )
                        .wrap(),
                    );
                });
            });

        // ===================== 4. 滚动条（右侧） =====================
        // 计算回溯上界
        let (max_sb, vis_rows) = {
            let el = self.el.borrow();
            let mgr = el.manager_ref();
            (mgr.max_scrollback(), mgr.size().rows)
        };
        if max_sb > 0 {
            let offset_ptr = &mut self.scroll_offset;
            egui::SidePanel::right("scrollbar")
                .frame(egui::Frame::NONE.fill(egui::Color32::from_rgb(20, 20, 20)))
                .min_width(12.0)
                .max_width(12.0)
                .show(ctx, |ui| {
                    let avail = ui.available_size();
                    let (rect, _) = ui.allocate_exact_size(avail, egui::Sense::click_and_drag());
                    let painter = ui.painter();
                    painter.rect_filled(rect, 0.0, egui::Color32::from_gray(28));
                    let total = (max_sb + vis_rows) as f32;
                    let thumb_h = (rect.height() * (vis_rows as f32 / total)).max(18.0);
                    // offset=max → 顶部；offset=0 → 底部
                    let frac = if max_sb > 0 {
                        1.0 - (*offset_ptr as f32 / max_sb as f32)
                    } else {
                        0.0
                    };
                    let thumb_y = rect.top() + (rect.height() - thumb_h) * frac.clamp(0.0, 1.0);
                    let thumb_rect = egui::Rect::from_min_size(
                        egui::pos2(rect.left() + 2.0, thumb_y),
                        egui::vec2(rect.width() - 4.0, thumb_h),
                    );
                    painter.rect_filled(thumb_rect, 3.0, egui::Color32::from_gray(120));
                    let resp =
                        ui.interact(rect, ui.id().with("sb_drag"), egui::Sense::click_and_drag());
                    if resp.dragged() {
                        if let Some(p) = resp.interact_pointer_pos() {
                            let f = 1.0 - ((p.y - rect.top()) / rect.height()).clamp(0.0, 1.0);
                            *offset_ptr = (f * max_sb as f32).round() as usize;
                        }
                    }
                });
        }

        // ===================== 5. 终端绘制（中央面板） =====================
        let central = egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(egui::Color32::from_rgb(12, 12, 12)))
            .show(ctx, |ui| {
                let (rect, _) =
                    ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
                let painter = ui.painter();

                // 重新计算行列
                let cols = ((rect.width() - PAD * 2.0) / cell_w).floor() as usize;
                let rows = ((rect.height() - PAD * 2.0) / cell_h).floor() as usize;
                let cols = cols.max(10);
                let rows = rows.max(2);

                // 首次或尺寸变化时 resize PTY
                {
                    let mut el = self.el.borrow_mut();
                    let cur = el.manager().size();
                    if cur.rows != rows || cur.cols != cols {
                        el.resize(rows, cols);
                    }
                }

                // 取快照
                let (snap, cursor, default_fg, default_bg) = {
                    let el = self.el.borrow();
                    let mgr = el.manager_ref();
                    (
                        mgr.snapshot_scrolled(self.scroll_offset),
                        mgr.cursor(),
                        mgr.default_fg(),
                        mgr.default_bg(),
                    )
                };

                let grid_origin = egui::pos2(rect.left() + PAD, rect.top() + PAD);
                let grid_w = cols as f32 * cell_w;
                let grid_h = rows as f32 * cell_h;
                let grid_rect = egui::Rect::from_min_size(grid_origin, egui::vec2(grid_w, grid_h));

                // 背景
                painter.rect_filled(grid_rect, 0.0, color32(default_bg));

                let font = egui::FontId::monospace(FONT_SIZE);

                // 逐单元格渲染
                for (y, row) in snap.rows.iter().enumerate().take(rows) {
                    let mut x = 0usize;
                    let row_top = grid_origin.y + (y as f32) * cell_h;
                    for cell in row {
                        if x >= cols {
                            break;
                        }
                        let span = cell.width.max(1) as f32;
                        let cell_left = grid_origin.x + (x as f32) * cell_w;
                        let cell_rect = egui::Rect::from_min_size(
                            egui::pos2(cell_left, row_top),
                            egui::vec2(span * cell_w, cell_h),
                        );

                        let reverse = cell.flags.contains(CellFlags::REVERSE);
                        let (fg, bg) = if reverse {
                            (cell.bg, cell.fg)
                        } else {
                            (cell.fg, cell.bg)
                        };

                        // 背景（仅当非默认色，减少 overdraw）
                        if bg != default_bg {
                            painter.rect_filled(cell_rect, 0.0, color32(bg));
                        }

                        // 文本
                        if !cell.is_blank() && !cell.flags.contains(CellFlags::INVISIBLE) {
                            let mut tc = color32(fg);
                            if cell.flags.contains(CellFlags::DIM) {
                                tc = tc.gamma_multiply(0.55);
                            }
                            let glyph_pos =
                                egui::pos2(cell_left, row_top + (cell_h - FONT_SIZE) * 0.5);
                            painter.text(
                                glyph_pos,
                                egui::Align2::LEFT_TOP,
                                &cell.text,
                                font.clone(),
                                tc,
                            );
                            // 伪粗体：再画一次，偏移 0.6px
                            if cell.flags.contains(CellFlags::BOLD) {
                                painter.text(
                                    egui::pos2(glyph_pos.x + 0.6, glyph_pos.y),
                                    egui::Align2::LEFT_TOP,
                                    &cell.text,
                                    font.clone(),
                                    tc,
                                );
                            }
                        }

                        // 下划线族
                        let line_color = color32(fg);
                        if cell.flags.contains(CellFlags::UNDERLINE) {
                            let uy = cell_rect.bottom() - 1.0;
                            painter.line_segment(
                                [
                                    egui::pos2(cell_rect.left(), uy),
                                    egui::pos2(cell_rect.right(), uy),
                                ],
                                egui::Stroke::new(1.0, line_color),
                            );
                        }
                        if cell.flags.contains(CellFlags::DOUBLE_UNDERLINE) {
                            let uy1 = cell_rect.bottom() - 3.0;
                            let uy2 = cell_rect.bottom() - 1.0;
                            painter.line_segment(
                                [
                                    egui::pos2(cell_rect.left(), uy1),
                                    egui::pos2(cell_rect.right(), uy1),
                                ],
                                egui::Stroke::new(1.0, line_color),
                            );
                            painter.line_segment(
                                [
                                    egui::pos2(cell_rect.left(), uy2),
                                    egui::pos2(cell_rect.right(), uy2),
                                ],
                                egui::Stroke::new(1.0, line_color),
                            );
                        }
                        if cell.flags.contains(CellFlags::UNDERCURL) {
                            // 简化为一条波浪线（用多段折线近似）
                            let uy = cell_rect.bottom() - 2.0;
                            let mut pts = Vec::new();
                            let steps = 4;
                            for i in 0..=steps {
                                let px = cell_rect.left()
                                    + (cell_rect.width() / steps as f32) * i as f32;
                                let py = uy + if i % 2 == 0 { -1.5 } else { 1.5 };
                                pts.push(egui::pos2(px, py));
                            }
                            painter.add(egui::Shape::line(pts, egui::Stroke::new(1.0, line_color)));
                        }
                        if cell.flags.contains(CellFlags::STRIKETHROUGH) {
                            let sy = cell_rect.center().y;
                            painter.line_segment(
                                [
                                    egui::pos2(cell_rect.left(), sy),
                                    egui::pos2(cell_rect.right(), sy),
                                ],
                                egui::Stroke::new(1.0, line_color),
                            );
                        }

                        x += cell.width.max(1);
                    }
                }

                // 光标（仅实时窗口）
                if self.scroll_offset == 0 && cursor.visible {
                    let cx = cursor.x;
                    let cy = cursor.y;
                    if cx < cols && cy < rows {
                        let left = grid_origin.x + (cx as f32) * cell_w;
                        let top = grid_origin.y + (cy as f32) * cell_h;
                        let cur_color = color32(default_fg);
                        match cursor.shape {
                            CursorShape::Block | CursorShape::Default => {
                                painter.rect_filled(
                                    egui::Rect::from_min_size(
                                        egui::pos2(left, top),
                                        egui::vec2(cell_w, cell_h),
                                    ),
                                    0.0,
                                    cur_color,
                                );
                            }
                            CursorShape::Bar => {
                                painter.rect_filled(
                                    egui::Rect::from_min_size(
                                        egui::pos2(left, top),
                                        egui::vec2(2.0, cell_h),
                                    ),
                                    0.0,
                                    cur_color,
                                );
                            }
                            CursorShape::Underline => {
                                let uy = top + cell_h - 2.0;
                                painter.line_segment(
                                    [egui::pos2(left, uy), egui::pos2(left + cell_w, uy)],
                                    egui::Stroke::new(2.0, cur_color),
                                );
                            }
                        }
                    }
                }

                // 鼠标事件采集（坐标映射 → 转发或滚动）
                for ev in &events {
                    match ev {
                        egui::Event::PointerButton {
                            pos,
                            button,
                            pressed,
                            modifiers,
                        } => {
                            // 仅在终端区域内
                            if !grid_rect.contains(*pos) {
                                continue;
                            }
                            let col = (((pos.x - grid_origin.x) / cell_w) as i64)
                                .clamp(0, (cols - 1) as i64)
                                as usize;
                            let row = (((pos.y - grid_origin.y) / cell_h) as i64)
                                .clamp(0, (rows - 1) as i64)
                                as usize;
                            if grabbed {
                                let action = if *pressed {
                                    MouseAction::Press
                                } else {
                                    MouseAction::Release
                                };
                                mouse_events.push((
                                    col,
                                    row,
                                    action,
                                    button_from_egui(*button),
                                    mods_from_egui(modifiers),
                                ));
                            }
                        }
                        egui::Event::PointerMoved(pos) => {
                            if !grid_rect.contains(*pos) {
                                continue;
                            }
                            let col = (((pos.x - grid_origin.x) / cell_w) as i64)
                                .clamp(0, (cols - 1) as i64)
                                as usize;
                            let row = (((pos.y - grid_origin.y) / cell_h) as i64)
                                .clamp(0, (rows - 1) as i64)
                                as usize;
                            if grabbed {
                                // 仅在按住时转发 Move
                                if self.mouse_down {
                                    mouse_events.push((
                                        col,
                                        row,
                                        MouseAction::Move,
                                        MouseButton::None,
                                        KeyMods::default(),
                                    ));
                                }
                            }
                        }
                        egui::Event::MouseWheel {
                            delta,
                            unit,
                            modifiers,
                        } => {
                            let lines = match unit {
                                egui::MouseWheelUnit::Line => delta.y.round() as i32,
                                egui::MouseWheelUnit::Page => {
                                    (delta.y * (rows as f32)).round() as i32
                                }
                                egui::MouseWheelUnit::Point => (delta.y / cell_h).round() as i32,
                            };
                            if lines == 0 {
                                continue;
                            }
                            if grabbed {
                                let n: u32 = lines.unsigned_abs();
                                let action = if lines > 0 {
                                    MouseAction::WheelUp(n)
                                } else {
                                    MouseAction::WheelDown(n)
                                };
                                // 滚轮坐标用屏幕中心近似
                                mouse_events.push((
                                    cols / 2,
                                    rows / 2,
                                    action,
                                    MouseButton::None,
                                    mods_from_egui(modifiers),
                                ));
                            } else if lines > 0 {
                                // 向上滚 → 回溯历史
                                self.scroll_offset =
                                    (self.scroll_offset + lines as usize).min(max_sb);
                                self.follow = self.scroll_offset == 0;
                            } else {
                                // 向下滚 → 趋近底部
                                let dec = lines.unsigned_abs() as usize;
                                self.scroll_offset = self.scroll_offset.saturating_sub(dec);
                                self.follow = self.scroll_offset == 0;
                            }
                        }
                        _ => {}
                    }
                }

                // 更新鼠标按下状态（用于 Move 转发判断）
                let primary_down = ctx.input(|i| i.pointer.primary_down());
                if primary_down != self.mouse_down {
                    self.mouse_down = primary_down;
                }

                // 若用户在终端区域点击（非 grabbed），也跟随到底部
                if ctx.input(|i| i.pointer.primary_clicked()) {
                    let p = ctx.input(|i| i.pointer.interact_pos());
                    if let Some(p) = p {
                        if grid_rect.contains(p) && !grabbed {
                            self.follow = true;
                            self.scroll_offset = 0;
                        }
                    }
                }
            });

        // ===================== 6. 发送输入与鼠标 =====================
        {
            let mut el = self.el.borrow_mut();
            for bytes in &keys_to_send {
                let _ = el.send_input(bytes);
            }
            for (col, row, action, button, mods) in &mouse_events {
                el.manager()
                    .mouse_event(*col, *row, *action, *button, *mods);
            }
        }

        // 同步窗口标题（OSC 0/2）
        let new_title = self.el.borrow().manager_ref().title();
        if !new_title.is_empty() && new_title != self.title {
            self.title = new_title.clone();
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(new_title));
        }

        let _ = new_frame;
        let _ = central;
        ctx.request_repaint_after(Duration::from_millis(16));
    }
}

fn main() {
    let mgr = TerminalManager::utf8(TerminalSize::new(24, 80));

    let shell = if cfg!(target_os = "windows") {
        "cmd.exe".to_string()
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string())
    };

    let pty_config = PtyConfig {
        shell,
        cols: 80,
        rows: 24,
        cwd: None,
    };

    let pty = PtyBridge::new(&pty_config).expect("failed to spawn PTY");
    let event_loop = EventLoop::new(mgr, Some(pty), EventLoopConfig::default());

    let el = Rc::new(RefCell::new(event_loop));
    let app = TerminalApp::new(el);

    // 引导：在 Windows 上切到 UTF-8 代码页，避免乱码
    if cfg!(target_os = "windows") {
        let _ = app.el.borrow_mut().send_input(b"chcp 65001\r\n");
    }

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(egui::vec2(820.0, 520.0))
            .with_min_inner_size(egui::vec2(400.0, 200.0))
            .with_title("rust-xterm - PTY Demo"),
        ..Default::default()
    };

    eframe::run_native(
        "rust-xterm - PTY Demo",
        native_options,
        Box::new(|_cc| Ok(Box::new(app))),
    )
    .expect("eframe error");
}
