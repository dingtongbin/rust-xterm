//! PTY example: spawn a real shell and drive rust-xterm.
//!
//! Demonstrates the full PTY → TerminalManager → frame pipeline.
//!
//! Run with: `cargo run --example pty_demo`

use rust_xterm_core::{TerminalManager, TerminalSize};
use rust_xterm_host::{Event, EventLoop, EventLoopConfig, PtyBridge, PtyConfig};
use std::time::{Duration, Instant};

fn main() {
    let mgr = TerminalManager::utf8(TerminalSize::new(24, 80));

    let pty_config = PtyConfig {
        shell: std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string()),
        cols: 80,
        rows: 24,
        cwd: None,
    };

    let pty = PtyBridge::new(&pty_config).expect("failed to spawn PTY");
    let mut event_loop = EventLoop::new(mgr, Some(pty), EventLoopConfig::default());

    // Send a command
    event_loop
        .send_input(b"echo 'Hello from rust-xterm!'\n")
        .expect("send failed");

    // Run for 2 seconds, printing frames
    let start = Instant::now();
    let mut frame_count = 0;
    while start.elapsed() < Duration::from_secs(2) {
        if let Some(event) = event_loop.tick() {
            match event {
                Event::FrameUpdate(frame) => {
                    frame_count += 1;
                    println!(
                        "[frame {}] dirty_rects={}, cursor=({}, {})",
                        frame_count,
                        frame.dirty_rects.len(),
                        frame.cursor.x,
                        frame.cursor.y
                    );
                }
                Event::Closed => {
                    println!("PTY closed");
                    break;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(16));
    }

    // Final screen snapshot
    let snap = event_loop.manager().screen_snapshot();
    println!("\n=== Final screen ({}x{}) ===", snap.cols(), snap.rows());
    for (y, row) in snap.rows.iter().enumerate() {
        let text: String = row.iter().map(|c| c.text.as_str()).collect();
        if !text.trim().is_empty() {
            println!("  {y:>2}: {text}");
        }
    }
}
