//! # russh-slint-demo 库
//!
//! 暴露给集成测试使用的内部模块。
//!
//! `main.rs` 通过 `russh_slint_demo::...` 引用同名模块（避免重复 mod 声明），
//! 保证单元测试与集成测试共享同一份代码。

pub mod config;
pub mod ssh;
