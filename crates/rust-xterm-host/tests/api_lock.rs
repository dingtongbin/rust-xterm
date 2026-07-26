//! API 签名锁定测试
//!
//! 此测试文件锁定 rust-xterm 所有公共 API 的签名，
//! 确保未来重构不会意外破坏接口兼容性。
//!
//! 如果此测试编译失败，说明 API 签名已变更，
//! 需要更新版本号并通知下游用户。
//!
//! 此处刻意导入全部公共类型以锁定其存在性，故允许未使用导入。

#![allow(unused_imports)]
#![allow(unused_variables)]

use rust_xterm_core::{
    integration::RenderSurface, Addon, AddonContext, Buffer, BufferNamespace, BufferType,
    CellFlags, Codec, CodecGate, CodecStats, Color, CursorMeta, CursorShape, DamageTracker,
    DirtyRect, DirtySpan, EventBus, EventSubscription, FrameUpdate, Marker, NullRenderSurface,
    NullWriter, Parser, RenderMetrics, RuntimeState, RustXtermCell, RustXtermConfig,
    RustXtermConfigBuilder, ScreenSnapshot, TerminalEvent, TerminalManager, TerminalSize,
    WezTermCore, WindowsTerminalTheme,
};
use rust_xterm_host::{Event, EventLoop, EventLoopConfig, PtyBridge, PtyConfig, PtyError};
use rust_xterm_renderer::{
    AtlasEntry, AtlasStats, Canvas, FontFace, FontTree, GlyphInfo, PixelFormat, RenderResult,
    Renderer, RendererConfig, TextureAtlas,
};
use std::time::Instant;

/// 锁定 rust_xterm_core 的所有公共类型签名
#[test]
fn test_core_api_signatures() {
    let mut mgr = TerminalManager::utf8(TerminalSize::new(24, 80));

    // TerminalManager 方法签名验证
    mgr.write(b"test");
    let _frame: Option<FrameUpdate> = mgr.poll_frame(Instant::now());
    mgr.resize(TerminalSize::new(30, 100));
    let _size: TerminalSize = mgr.size();
    let _damage: &DamageTracker = mgr.damage();
    let _snap: ScreenSnapshot = mgr.screen_snapshot();
    let _cursor: CursorMeta = mgr.cursor();
    mgr.set_cursor_blinking(true);
    mgr.set_codec(Codec::Gbk);

    // xterm.js 风格 API
    let _sub: EventSubscription = mgr.on(|_event| {});
    let _title: String = mgr.title();
    let _icon: String = mgr.icon_name();
    let _alt: bool = mgr.is_alt_screen_active();
    let _marker: Marker = mgr.add_marker(5);
    let _markers: Vec<Marker> = mgr.markers();
    let _removed: bool = mgr.remove_marker(0);
    let _buffer: Buffer = mgr.buffer();
    let _fg: Color = mgr.default_fg();
    let _bg: Color = mgr.default_bg();
    mgr.apply_theme(&WindowsTerminalTheme::campbell());

    // CodecGate
    let mut gate = CodecGate::utf8();
    let _s: String = gate.decode(b"test");
    let _b: Vec<u8> = gate.encode("test");
    let _c: Codec = gate.codec();
    gate.set_codec(Codec::Gbk);
    let _stats: CodecStats = gate.stats();

    // DamageTracker
    let mut dt = DamageTracker::new(24, 80);
    dt.mark_dirty(5);
    dt.mark_dirty_range(0, 5);
    dt.mark_all_dirty();
    let _empty: bool = dt.is_empty();
    let _rects: Vec<DirtyRect> = dt.drain_rects();
    dt.resize(30, 100);

    // Buffer / Marker
    let _ns = BufferNamespace::new();
    let _buf = Buffer {
        kind: BufferType::Normal,
        cursor_y: 0,
        cursor_x: 0,
        base_y: 0,
        height: 24,
        width: 80,
        lines: vec![vec![RustXtermCell::blank(); 80]; 24],
    };

    // Parser
    let mut parser = Parser::new();
    parser.register_csi(b'H', |_data| {});
    parser.register_osc(8, |_data| {});
    parser.register_dcs(b'q', |_data| {});
    let _csi_count: usize = parser.csi_handler_count();
    let _osc_count: usize = parser.osc_handler_count();
    parser.unregister_csi(b'H');
    parser.unregister_osc(8);

    // Theme
    let _theme = WindowsTerminalTheme::campbell();
    let _theme2 = WindowsTerminalTheme::vintage();
    let _palette = _theme.to_palette();

    // Integration traits
    let mut surface = NullRenderSurface::new();
    surface.update_row(0, &[]);
    surface.update_cursor(CursorMeta {
        x: 0,
        y: 0,
        visible: true,
        shape: CursorShape::Default,
    });
    surface.resize(800, 600);
    surface.present();
    let _m: RenderMetrics = surface.metrics();
}

/// 锁定 rust_xterm_renderer 的所有公共类型签名
#[test]
fn test_renderer_api_signatures() {
    let _cfg = RendererConfig::default();
    let _metrics: RenderMetrics = RenderMetrics::default();
    let _ = Renderer::new(RendererConfig::default());

    let mut atlas = TextureAtlas::new(512, 512, 1, 20);
    let _ = atlas.lookup_static('A', false, false);
    let _ = atlas.lookup_dynamic('X', false, false);
    let _stats: AtlasStats = atlas.stats();

    let _canvas = Canvas::new(100, 100, PixelFormat::Rgba);
    let _tree = FontTree::new();
}

/// 锁定 rust_xterm_host 的所有公共类型签名
#[test]
fn test_host_api_signatures() {
    let mgr = TerminalManager::utf8(TerminalSize::new(24, 80));
    let cfg = PtyConfig::default();
    let _ = PtyConfig {
        shell: "/bin/bash".to_string(),
        cols: 80,
        rows: 24,
        cwd: None,
    };

    let mut el = EventLoop::new(mgr, None, EventLoopConfig::default());
    let _tick: Option<Event> = el.tick();
    let _ = el.send_input(b"test");
    el.resize(30, 100);

    let _: PtyConfig = PtyConfig::default();
    let _: EventLoopConfig = EventLoopConfig::default();
    let _: PtyError;
}

/// 验证核心数据流：write -> poll_frame -> screen_snapshot
#[test]
fn test_core_data_flow() {
    let mut mgr = TerminalManager::utf8(TerminalSize::new(24, 80));
    mgr.write(b"Hello, rust-xterm!\n");

    let frame = mgr.poll_frame(Instant::now());
    assert!(frame.is_some());
    let frame = frame.unwrap();

    assert!(!frame.dirty_rects.is_empty());
    assert!(!frame.dirty_spans.is_empty());

    let snapshot = mgr.screen_snapshot();
    assert_eq!(snapshot.size.rows, 24);
    assert_eq!(snapshot.size.cols, 80);

    let cursor = mgr.cursor();
    assert!(cursor.visible);
}

/// 验证编码切换数据流
#[test]
fn test_codec_switch_flow() {
    let mut mgr = TerminalManager::utf8(TerminalSize::new(24, 80));

    mgr.write("你好".as_bytes());
    let snap = mgr.screen_snapshot();
    let text: String = snap
        .rows
        .iter()
        .flat_map(|r| r.iter().map(|c| c.text.as_str()))
        .collect();
    assert!(text.contains("你好"));

    mgr.set_codec(Codec::Gbk);
    mgr.write(&[0xCA, 0xC0, 0xBD, 0xE7]);
    let snap = mgr.screen_snapshot();
    let text: String = snap
        .rows
        .iter()
        .flat_map(|r| r.iter().map(|c| c.text.as_str()))
        .collect();
    assert!(text.contains("世界"));
}

/// 验证事件系统
#[test]
fn test_event_system() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    let mut mgr = TerminalManager::utf8(TerminalSize::new(24, 80));
    let counter = Arc::new(AtomicUsize::new(0));
    let c = counter.clone();

    let _sub = mgr.on(move |event| {
        if matches!(event, TerminalEvent::TitleChange(_)) {
            c.fetch_add(1, Ordering::Relaxed);
        }
    });

    // 触发 title 变更
    mgr.write(b"\x1b]0;My Title\x07");

    assert!(counter.load(Ordering::Relaxed) >= 1);
}

/// 验证 Buffer/Marker 系统
#[test]
fn test_buffer_marker_system() {
    let mut mgr = TerminalManager::utf8(TerminalSize::new(24, 80));

    let marker = mgr.add_marker(10);
    assert_eq!(marker.line, 10);
    assert_eq!(mgr.markers().len(), 1);

    let buffer = mgr.buffer();
    assert_eq!(buffer.kind, BufferType::Normal);
    assert_eq!(buffer.height, 24);
}

/// 验证 Addon 系统
#[test]
fn test_addon_system() {
    struct TestAddon {
        activated: bool,
    }
    impl Addon for TestAddon {
        fn activate(&mut self, _ctx: &mut AddonContext) {
            self.activated = true;
        }
        fn dispose(&mut self) {}
        fn name(&self) -> &str {
            "test"
        }
    }

    let mut mgr = TerminalManager::utf8(TerminalSize::new(24, 80));
    mgr.load_addon(TestAddon { activated: false });
}

/// 验证 Windows Terminal 主题
#[test]
fn test_windows_terminal_theme() {
    let mut mgr = TerminalManager::utf8(TerminalSize::new(24, 80));
    let theme = WindowsTerminalTheme::campbell();
    mgr.apply_theme(&theme);

    // Campbell 背景色是 0x0C0C0C
    let bg = mgr.default_bg();
    assert_eq!(bg.r, 0x0C);
    assert_eq!(bg.g, 0x0C);
    assert_eq!(bg.b, 0x0C);
}
