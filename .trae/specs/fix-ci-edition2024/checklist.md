# Checklist
- [x] Cargo.lock 中 url 版本为 2.5.2（非 2.5.8）
- [x] idna_adapter 不再出现在依赖树中（cargo tree -i idna_adapter 报 not found）
- [x] cargo build --all-targets 通过
- [x] cargo test --all-targets 通过
- [x] cargo clippy --all-targets -- -D warnings 通过
- [x] cargo fmt --all -- --check 通过
