# Checklist
- [x] vendor/tattoy-wezterm-escape-parser 的 edition 为 "2021"
- [x] vendor/tattoy-wezterm-cell 的 edition 为 "2021"
- [x] 根 Cargo.toml 包含 [patch.crates-io] 指向 vendor 路径
- [x] cargo build --all-targets 通过
- [x] cargo test --all-targets 通过
- [x] cargo clippy --all-targets -- -D warnings 通过
- [x] cargo fmt --all -- --check 通过
- [x] 无 edition 2024 crate 在 Rust 1.72 构建路径上
