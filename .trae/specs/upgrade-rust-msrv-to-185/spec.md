# Upgrade Rust MSRV to 1.85 Spec

## Why
Rust 1.72 MSRV 导致无尽的依赖降级和 workaround——edition 2024 crate（tattoy fork）、rust-version 要求（schemars 1.74+、exr 1.83+）不断涌现。Rust 1.75 不够（exr 要 1.83，edition 2024 要 1.85）。升级到 1.85 能彻底解决所有问题，并撤销全部 workaround。

## What Changes
- 将 CI 中所有 `dtolnay/rust-toolchain@1.72.0` 改为 `dtolnay/rust-toolchain@1.85.0`
- 将 `rust-version = "1.72"` 改为 `rust-version = "1.85"`
- 将 core-msrv job 改为用 1.85 并恢复 `--all-targets`（不再需要区分 examples）
- **撤销所有 workaround**：
  - 删除 `vendor/` 目录（tattoy-wezterm-escape-parser、tattoy-wezterm-cell）
  - 删除 `[patch.crates-io]` 中的 tattoy vendor 和 ravif-stub 条目
  - 删除 `crates/ravif-stub/` 目录
  - 运行 `cargo update` 将所有降级的依赖恢复到最新版本（url、image、uuid、avif-serialize、schemars）
- 简化 CI：合并 core-msrv 和 demo job（1.85 下 examples 也能编译）

## Impact
- Affected specs: CI 稳定性, MSRV 承诺
- Affected code: `.github/workflows/ci.yml`、`Cargo.toml`、`Cargo.lock`、删除 `vendor/` 和 `crates/ravif-stub/`

## ADDED Requirements

### Requirement: Rust 1.85 MSRV
系统 SHALL 使用 Rust 1.85.0 作为最低支持版本，支持 edition 2024 和所有现代依赖的 rust-version 要求。

#### Scenario: CI 在 Rust 1.85 下编译
- **WHEN** CI 用 Rust 1.85 执行 `cargo build --all-targets`
- **THEN** 所有 crate 包括 examples 编译通过，无 edition 2024 错误

## REMOVED Requirements

### Requirement: Rust 1.72 MSRV
**Reason**: Rust 1.72 无法支持 edition 2024 依赖和 rust-version 1.74+ 的 crate
**Migration**: 升级到 Rust 1.85，撤销所有降级和 vendor workaround
