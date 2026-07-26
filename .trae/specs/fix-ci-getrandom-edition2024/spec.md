# getrandom edition 2024 依赖修复 Spec

## Why
CI 在 Rust 1.72 下失败：`getrandom 0.4.3` 使用 edition 2024，作为 `uuid 1.24.0` 的依赖被拉入，而 Rust 1.72 的 Cargo 不支持 edition 2024。

## What Changes
- 将 `Cargo.lock` 中的 `uuid` 降级到不依赖 getrandom 0.4.x 的版本
- 纯 lock 文件变更，不修改任何 Cargo.toml 或源码

## Impact
- Affected specs: CI 稳定性
- Affected code: 仅 `Cargo.lock`

## ADDED Requirements

### Requirement: MSRV 依赖兼容
系统 SHALL 确保 Cargo.lock 中不包含使用 edition 2024 的依赖，以维护 Rust 1.72 MSRV 承诺。

#### Scenario: CI 在 Rust 1.72 下解析依赖
- **WHEN** CI 用 Rust 1.72 执行 `cargo build`
- **THEN** 不出现 "this version of Cargo is older than the 2024 edition" 错误
