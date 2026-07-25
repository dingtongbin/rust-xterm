# Tasks
- [ ] Task 1: 删除 vendor 和 ravif-stub workaround
  - [ ] SubTask 1.1: 删除 vendor/ 目录
  - [ ] SubTask 1.2: 从 Cargo.toml 的 [patch.crates-io] 移除 tattoy vendor 和 ravif-stub 条目
  - [ ] SubTask 1.3: 删除 crates/ravif-stub/ 目录
- [ ] Task 2: 升级 Rust 版本配置
  - [ ] SubTask 2.1: 将 Cargo.toml 中 rust-version 从 "1.72" 改为 "1.85"
  - [ ] SubTask 2.2: 将 ci.yml 中所有 1.72.0 改为 1.85.0
  - [ ] SubTask 2.3: 简化 CI：合并 core-msrv 和 demo job，使用 --all-targets
- [ ] Task 3: 恢复依赖到最新版本
  - [ ] SubTask 3.1: 运行 cargo update 恢复所有降级的依赖
  - [ ] SubTask 3.2: 验证 Cargo.lock 无降级残留
- [ ] Task 4: 验证并提交
  - [ ] SubTask 4.1: 验证 cargo build --all-targets 通过
  - [ ] SubTask 4.2: 验证 cargo test --all-targets 通过
  - [ ] SubTask 4.3: 验证 cargo clippy --all-targets -- -D warnings 通过
  - [ ] SubTask 4.4: 验证 cargo fmt --all -- --check 通过
  - [ ] SubTask 4.5: 提交并推送

# Task Dependencies
- [Task 2] depends on [Task 1]
- [Task 3] depends on [Task 1] and [Task 2]
- [Task 4] depends on [Task 3]
