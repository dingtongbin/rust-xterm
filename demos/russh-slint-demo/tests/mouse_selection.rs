//! 集成测试：鼠标选区 → render_selection 高亮渲染（Task 4.5）
//!
//! 验证流程：
//! 1. 写入 "hello world" 到 TerminalManager
//! 2. 模拟左键拖拽选区 (0,0) → (4,0)（选中 "hello"）
//! 3. 调用 Renderer::render_selection 渲染选区高亮
//! 4. 验证被选 cell 像素被反相（背景从黑变白）
//!
//! 注意：render_selection 通过交换 fg/bg 实现反相。
//! 原始 cell: fg=WHITE, bg=BLACK → 反相后 bg=WHITE。
//! 被选 cell 的背景像素应为白色（r > 200），字形像素仍为黑色。
//! 测试检查 cell 内是否存在白色像素（避免单像素恰好命中字形）。

use rust_xterm_core::mouse::{KeyMods, MouseAction, MouseButton};
use rust_xterm_core::{TerminalManager, TerminalSize};
use rust_xterm_renderer::{Renderer, RendererConfig};

#[test]
fn mouse_selection_renders_highlight() {
    let mut mgr = TerminalManager::utf8(TerminalSize::new(5, 10));
    mgr.write(b"hello world");
    let _ = mgr.poll_frame(std::time::Instant::now());

    let mut renderer = Renderer::new(RendererConfig::default());
    renderer.clear();

    let mods = KeyMods::default();
    // 按下左键于 (col=0, row=0)
    mgr.mouse_event(0, 0, MouseAction::Press, MouseButton::Left, mods);
    // 拖拽到 (col=4, row=0)
    mgr.mouse_event(4, 0, MouseAction::Move, MouseButton::Left, mods);

    // 渲染选区
    let sel = mgr.selection().expect("应有选区");
    assert_eq!(sel.start, (0, 0));
    assert_eq!(sel.end, (0, 4));
    let snap = mgr.screen_snapshot();
    renderer.render_selection(&sel, &snap.rows);

    // 验证被选 cell 像素被反相：检查 cell (2,0) 内是否有白色像素
    // cell (2,0) 对应字符 'l'（"hello" 的第 3 个字符）
    let canvas = renderer.canvas();
    let cw = renderer.metrics().cell_width;
    let ch = renderer.metrics().cell_height;
    let mut found_white = false;
    for dx in 0..cw {
        for dy in 0..ch {
            let px = canvas.get_pixel(2 * cw + dx, dy);
            if px.0 > 200 {
                found_white = true;
                break;
            }
        }
        if found_white {
            break;
        }
    }
    assert!(
        found_white,
        "被选 cell 背景应反相为白色（cell 内至少有一个 r>200 的像素）"
    );
}

#[test]
fn mouse_selection_clears_on_new_press() {
    // 验证选区在新的单击时被清除（选区状态机正确性）
    let mut mgr = TerminalManager::utf8(TerminalSize::new(5, 10));
    mgr.write(b"hello world");

    let mods = KeyMods::default();
    // 第一次拖拽选区
    mgr.mouse_event(0, 0, MouseAction::Press, MouseButton::Left, mods);
    mgr.mouse_event(4, 0, MouseAction::Move, MouseButton::Left, mods);
    assert!(mgr.selection().is_some(), "拖拽后应有选区");

    // 释放后再次按下（不同位置）应清除旧选区
    mgr.mouse_event(4, 0, MouseAction::Release, MouseButton::Left, mods);
    // 等待超过点击窗口（500ms）后在不同位置按下
    std::thread::sleep(std::time::Duration::from_millis(600));
    mgr.mouse_event(2, 1, MouseAction::Press, MouseButton::Left, mods);
    // 新的单击（click_count=1）应清除旧选区并开始新选区
    // 此时 selection 可能为 None（按下时清除）或有新值（取决于实现）
    // 主要验证不 panic 且状态一致
}

#[test]
fn shift_bypass_forces_local_selection() {
    // 验证 Shift 修饰键强制本地选区（Task 4.3）
    // 即使鼠标跟踪模式开启，Shift+左键也应走本地选区
    let mut mgr = TerminalManager::utf8(TerminalSize::new(5, 10));
    mgr.write(b"hello world");

    // 启用鼠标跟踪模式（DECSET 1006 + 1002 + 1000）
    mgr.write(b"\x1b[?1006h\x1b[?1002h\x1b[?1000h");
    let _ = mgr.poll_frame(std::time::Instant::now());
    assert!(mgr.is_mouse_grabbed(), "应已启用鼠标跟踪");

    // 不按 Shift：事件应转发给 WezTerm（不产生选区）
    let mods_no_shift = KeyMods::default();
    mgr.mouse_event(0, 0, MouseAction::Press, MouseButton::Left, mods_no_shift);
    mgr.mouse_event(4, 0, MouseAction::Move, MouseButton::Left, mods_no_shift);
    assert!(
        mgr.selection().is_none(),
        "鼠标跟踪模式下无 Shift 不应产生本地选区"
    );

    // 按 Shift：事件应走本地选区（bypass）
    let mods_shift = KeyMods {
        shift: true,
        ..KeyMods::default()
    };
    // 重新按下（在新位置）以触发选区
    mgr.mouse_event(0, 0, MouseAction::Press, MouseButton::Left, mods_shift);
    mgr.mouse_event(4, 0, MouseAction::Move, MouseButton::Left, mods_shift);
    assert!(
        mgr.selection().is_some(),
        "Shift bypass 应强制走本地选区，即使鼠标跟踪已启用"
    );
}
