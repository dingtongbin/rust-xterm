# Patch tattoy-wezterm edition 2024 Spec

## Why
`tattoy-wezterm-escape-parser 0.1.0-1` 和 `tattoy-wezterm-cell 0.1.0-1` 是 tattoy 项目发布的 fork crate，其 Cargo.toml 声明了 `edition = "2024"`，但源码不使用任何 2024 特有语法。这导致 Rust 1.72 的 CI 无法解析这些 crate。这两个 crate 是 `tattoy-wezterm-term 0.1.0-fork.5` 的核心依赖，无法移除或降级。

## What Changes
- Vendor `tattoy-wezterm-escape-parser 0.1.0-1` 和 `tattoy-wezterm-cell 0.1.0-1` 的源码到 `vendor/` 目录
- 将两个 crate 的 `edition` 从 `"2024"` 改为 `"2021"`（源码不使用 2024 特有语法）
- 在根 `Cargo.toml` 添加 `[patch.crates-io]` 指向 vendor 路径
- 保持版本号 `0.1.0-1` 不变，满足 tattoy-wezterm-term 的版本要求

## Impact
- Affected specs: CI 稳定性, MSRV 兼容性
- Affected code: `Cargo.toml`（添加 patch 段）、新增 `vendor/` 目录

## ADDED Requirements

### Requirement: Edition 2024 依赖消除
系统 SHALL 通过 `[patch.crates-io]` 将使用 edition 2024 的 tattoy fork crate 替换为 edition 2021 的本地 vendor 版本，确保 Rust 1.72 能正确解析。

#### Scenario: CI 在 Rust 1.72 下解析 tattoy crate
- **WHEN** CI 用 Rust 1.72 执行 `cargo build`
- **THEN** tattoy-wezterm-escape-parser 和 tattoy-wezterm-cell 从本地 vendor 编译，不出现 edition 2024 错误
