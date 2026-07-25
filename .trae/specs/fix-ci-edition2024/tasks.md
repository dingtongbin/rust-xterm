# Tasks
- [x] Task 1: 降级 url 到 2.5.2 修复 edition 2024 依赖
  - [x] SubTask 1.1: 执行 `cargo update -p url --precise 2.5.2` 将 url 从 2.5.8 降级
  - [x] SubTask 1.2: 验证 idna_adapter 不再出现在依赖树中
  - [x] SubTask 1.3: 验证 build/test/clippy/fmt 全部通过

# Task Dependencies
- 无依赖，单一任务
