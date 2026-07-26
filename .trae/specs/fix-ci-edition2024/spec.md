# CI edition 2024 依赖漂移修复 Spec

## Why
CI 在 Rust 1.72 下失败：`idna_adapter 1.2.2` 使用 edition 2024，作为 `url 2.5.8` → `idna 1.1.0` 的传递依赖被拉入，而 Rust 1.72 的 Cargo 不支持 edition 2024。这是 `url` crate 从 2.5.5 起引入的破坏性变更。

## What Changes
- 将 `Cargo.lock` 中的 `url` 从 2.5.8 降级到 2.5.2（使用 `idna 0.5.0`，不依赖 edition 2024 的 `idna_adapter`）
- 这是纯 lock 文件变更，不修改任何 Cargo.toml

## Impact
- Affected specs: CI 稳定性
- Affected code: 仅 `Cargo.lock`

## ADDED Requirements

### Requirement: MSRV 依赖兼容
系统 SHALL 确保 Cargo.lock 中不包含使用 edition 2024 的依赖，以维护 Rust 1.72 MSRV 承诺。

#### Scenario: CI 在 Rust 1.72 下解析依赖
- **WHEN** CI 用 Rust 1.72 执行 `cargo build`
- **THEN** 不出现 "this version of Cargo is older than the 2024 edition" 错误
