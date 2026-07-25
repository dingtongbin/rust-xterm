# Checklist
- [x] Cargo.lock 中 uuid 版本不依赖 getrandom 0.4.x
- [x] getrandom 0.4.3 不再出现在 Cargo.lock 中
- [x] cargo build --all-targets 通过
- [x] cargo test --all-targets 通过
- [x] cargo clippy --all-targets -- -D warnings 通过
- [x] cargo fmt --all -- --check 通过
