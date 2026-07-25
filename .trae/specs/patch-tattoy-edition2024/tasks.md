# Tasks
- [x] Task 1: Vendor tattoy-wezterm-escape-parser 并修改 edition
  - [x] SubTask 1.1: 创建 vendor/tattoy-wezterm-escape-parser 目录并复制源码
  - [x] SubTask 1.2: 将 Cargo.toml 中 edition 从 "2024" 改为 "2021"
- [x] Task 2: Vendor tattoy-wezterm-cell 并修改 edition
  - [x] SubTask 2.1: 创建 vendor/tattoy-wezterm-cell 目录并复制源码
  - [x] SubTask 2.2: 将 Cargo.toml 中 edition 从 "2024" 改为 "2021"
- [x] Task 3: 配置 [patch.crates-io]
  - [x] SubTask 3.1: 在根 Cargo.toml 添加 patch 段指向 vendor 路径
  - [x] SubTask 3.2: 运行 cargo update 更新 Cargo.lock
- [x] Task 4: 验证并提交
  - [x] SubTask 4.1: 验证 build/test/clippy/fmt 全部通过
  - [x] SubTask 4.2: 确认无 edition 2024 crate 在构建路径上
  - [ ] SubTask 4.3: 提交并推送到 PR #6

# Task Dependencies
- [Task 3] depends on [Task 1] and [Task 2]
- [Task 4] depends on [Task 3]
