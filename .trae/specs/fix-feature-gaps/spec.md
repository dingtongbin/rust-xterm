# rust-xterm 特性缺口修复 Spec

## Why

`FEATURES.md` 对照表显示整体达成率仅约 30% 已实现、40% 未实现。其中存在多个直接影响用户体验的 bug（主题色不生效、字形鬼影）、死代码（`Parser` 从未接入数据流、`BufferNamespace` 影子状态）、以及承诺但从未 emit 的事件（`Bell`/`ClipboardRequest`/`IconNameChange`）。本 spec 聚焦于**可验证、最小实现**的缺口修复，不涉及需大规模重写的连字/彩色 Emoji/选区系统/图像协议等深水区。

## What Changes

- 修复 `resolve_color` 中 `ColorAttribute::Default` 硬编码 WHITE/BLACK 导致自定义主题不生效的 bug
- 修复 `TextureAtlas` 动态区回绕覆盖时未清理 LRU 旧条目导致的"鬼影"渲染错乱
- 清理 `BufferNamespace` 影子状态：移除从未被读取的 `normal`/`alternate`/`active` 字段，只保留 `markers`
- 让 `Marker.line` 在 scrollback 增长时随滚动更新，修复"标记不随滚动移动"的兼容性缺口
- 接线 `Parser` 到 `TerminalManager::write` 数据流，使 OSC handler 真正可被触发
- emit `Bell`/`IconNameChange`/`ClipboardRequest` 事件（`ClipboardRequest` 依赖 Parser 接线后注册 OSC 52 handler）
- 暴露 `is_bracketed_paste_enabled()` 查询 API
- 为 `RustXtermCell` 增加 `hyperlink: Option<String>` 字段并从 WezTerm attrs 提取（为 OSC 8 渲染铺路）
- 将 `FontTree` 的 `glyph_cache` 从无界 `HashMap` 改为有界 `LruCache`，停止 `font_data_cache` 的 `Vec<u8>` clone（改 `Arc<[u8]>`）
- 在 `Renderer` 中接线 `RenderResult.dirty_rects`，让渲染层真正消费核心层的行级脏区
- 渲染层缺字时画 `.notdef` 方块而非静默跳过
- 移除 `font_tree.rs` 硬编码的 `is_emoji`/`is_wide_char`，改用 `cell.width`（WezTerm 已提供权威宽度）

**BREAKING**：`buffer()` 返回的 `Buffer` 结构语义不变，但 `BufferNamespace` 公共字段移除（若外部代码直接访问 `.normal`/`.alternate`/`.active` 会编译失败）。`RustXtermCell` 新增 `hyperlink` 字段，直接构造 `RustXtermCell` 的代码需更新。

## Impact

- Affected specs: 架构解耦（保持）、文本渲染与排版（字体回退改善）、终端协议与序列（OSC 事件接线）、屏幕与缓冲管理（影子状态清理）、性能与内存（FontTree 有界化、脏区接线）
- Affected code:
  - `crates/rust-xterm-core/src/wezterm_core.rs`（resolve_color 签名改造）
  - `crates/rust-xterm-core/src/manager.rs`（Parser 接线、事件 emit、bracketed paste API）
  - `crates/rust-xterm-core/src/buffer.rs`（BufferNamespace 精简、Marker 滚动追踪）
  - `crates/rust-xterm-core/src/events.rs`（确认事件变体，无结构变更）
  - `crates/rust-xterm-core/src/cell.rs`（新增 hyperlink 字段）
  - `crates/rust-xterm-renderer/src/font_tree.rs`（LRU 化、Arc 共享、移除硬编码宽度判定）
  - `crates/rust-xterm-renderer/src/atlas.rs`（回绕清 LRU）
  - `crates/rust-xterm-renderer/src/renderer.rs`（dirty_rects 接线、notdef 兜底）

## ADDED Requirements

### Requirement: 主题默认色生效
系统 SHALL 在 `ColorAttribute::Default` 时使用 `TerminalManager` 配置的 `default_fg`/`default_bg`，而非硬编码 WHITE/BLACK。

#### Scenario: 自定义前景色生效
- **WHEN** 用户调用 `set_default_fg(BLUE)` 后写入默认色文本
- **THEN** 渲染快照中该 cell 的 `fg` 字段为蓝色

### Requirement: 字形鬼影修复
系统 SHALL 在 `TextureAtlas` 动态区回绕覆盖时，清理指向被覆盖区域的 LRU 条目。

#### Scenario: 动态区回绕后无鬼影
- **WHEN** atlas 动态区写满回绕后查找旧字形
- **THEN** 旧 entry 未命中（重新光栅化），不会读到新字形的像素

### Requirement: 事件完整 emit
系统 SHALL 在检测到响铃时 emit `Bell` 事件，在图标名变更时 emit `IconNameChange`，在收到 OSC 52 时 emit `ClipboardRequest`。

#### Scenario: BEL 字节触发 Bell 事件
- **WHEN** 写入 `\x07`（BEL）
- **THEN** 订阅 `TerminalEvent::Bell` 的回调被触发

### Requirement: 括号粘贴查询
系统 SHALL 暴露 `is_bracketed_paste_enabled() -> bool` 查询 WezTerm 的 DECSET 2004 状态。

#### Scenario: 查询括号粘贴模式
- **WHEN** 应用启用 DECSET 2004
- **THEN** `is_bracketed_paste_enabled()` 返回 `true`

### Requirement: 超链接字段透传
系统 SHALL 在 `RustXtermCell` 中携带 `hyperlink: Option<String>`，从 WezTerm cell attrs 提取。

#### Scenario: OSC 8 超链接透传
- **WHEN** WezTerm cell 含 hyperlink 属性
- **THEN** 转换后的 `RustXtermCell.hyperlink` 为 `Some(url)`

### Requirement: FontTree 内存有界
系统 SHALL 将 `FontTree.glyph_cache` 限制为有界 LRU，并将 `font_data_cache` 的值改为 `Arc<[u8]>` 共享引用而非 clone。

#### Scenario: 大量字符渲染后内存有界
- **WHEN** 渲染超过 LRU 容量的不同字符
- **THEN** `glyph_cache` 条目数不超过硬上限

### Requirement: 渲染脏区精确输出
系统 SHALL 在 `Renderer` 渲染后填充 `RenderResult.dirty_rects`，而非无条件返回整行。

#### Scenario: 单行变更仅标记该行
- **WHEN** 仅第 5 行有脏区
- **THEN** `RenderResult.dirty_rects` 仅含第 5 行的矩形

### Requirement: 缺字画方块
系统 SHALL 在字体回退全 miss 时画 `.notdef` 方块，而非静默跳过。

#### Scenario: 缺字显示方块
- **WHEN** 渲染一个所有字体均无对应字形的字符
- **THEN** 该 cell 位置显示方块而非空白

## MODIFIED Requirements

### Requirement: BufferNamespace 状态
移除从未被读取的 `normal`/`alternate`/`active` 影子字段，`buffer()` 继续从 WezTerm 实时重建快照。`markers` 字段保留并实现滚动追踪：`Marker.line` 在 scrollback 增长时按推出行数递减，推出可视区后移除。

### Requirement: Parser 接线
`Parser` 不再是死代码。`TerminalManager` 持有 `Parser` 实例，在 `write()` 中对 OSC 序列做轻量扫描并 dispatch 已注册 handler。注意：不为 CSI/DCS 做 intercept（避免与 WezTerm 重复），仅 OSC 用于 OSC 52 剪贴板桥接。

## REMOVED Requirements

### Requirement: font_tree 硬编码宽度判定
**Reason**: `is_emoji`/`is_wide_char` 硬编码范围与 WezTerm 权威宽度表不一致，且 `cell.width` 已透传 WezTerm 结果。
**Migration**: 渲染层改用 `cell.width` 和 `cell.flags` 判定，不再调用 `font_tree` 的 `is_emoji`/`is_wide_char`。这两个函数从 `font_tree.rs` 删除。
