//! Headless example: drive rust-xterm without any GUI.
//!
//! Demonstrates the minimal integration path:
//! 1. Create a `TerminalManager`.
//! 2. Feed it bytes (simulating PTY output).
//! 3. Poll frames and print dirty cells to stdout.
//!
//! Run with: `cargo run --example headless`

use rust_xterm_core::{TerminalManager, TerminalSize};
use std::time::Instant;

fn main() {
    let mut mgr = TerminalManager::utf8(TerminalSize::new(24, 80));

    // Simulate a shell prompt + colored output
    mgr.write(b"\x1b[32muser@host\x1b[0m:\x1b[34m~/projects\x1b[0m$ ");
    mgr.write(b"echo \xe4\xbd\xa0\xe5\xa5\xbd\xe4\xb8\x96\xe7\x95\x8c\n"); // "你好世界" in UTF-8
    mgr.write(b"\xe4\xbd\xa0\xe5\xa5\xbd\xe4\xb8\x96\xe7\x95\x8c\n");

    // Poll the first frame
    if let Some(frame) = mgr.poll_frame(Instant::now()) {
        println!("=== Frame update ===");
        println!("Dirty rects: {}", frame.dirty_rects.len());
        println!("Dirty rows:  {}", frame.dirty_cells.len());
        println!("Cursor:      ({}, {})", frame.cursor.x, frame.cursor.y);

        // Print the dirty rows
        for row in &frame.dirty_cells {
            let text: String = row.cells.iter().map(|c| c.text.as_str()).collect();
            if !text.trim().is_empty() {
                println!("  row {:>2}: {}", row.y, text);
            }
        }
    }

    // Idle poll — should return None (0% CPU)
    let frame = mgr.poll_frame(Instant::now());
    assert!(frame.is_none(), "Idle poll should return None");
    println!("\n=== Idle poll returned None (0% CPU) ===");

    // Snapshot the full screen
    let snap = mgr.screen_snapshot();
    println!(
        "\n=== Full screen snapshot ({}x{}) ===",
        snap.cols(),
        snap.rows()
    );
    for (y, row) in snap.rows.iter().enumerate() {
        let text: String = row.iter().map(|c| c.text.as_str()).collect();
        if !text.trim().is_empty() {
            println!("  {y:>2}: {text}");
        }
    }
}
