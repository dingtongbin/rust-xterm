//! Addon 插件系统（xterm.js 风格）
//!
//! 类似 xterm.js 的 `ITerminalAddon` 接口，
//! 允许第三方扩展终端功能。
//!
//! ## 使用方式
//!
//! ```rust,ignore
//! use rust_xterm_core::addon::{Addon, AddonContext};
//!
//! struct SearchAddon;
//!
//! impl Addon for SearchAddon {
//!     fn activate(&mut self, ctx: &mut AddonContext) {
//!         // 注册自定义序列处理器、事件回调等
//!     }
//!
//!     fn dispose(&mut self) {
//!         // 清理资源
//!     }
//! }
//!
//! manager.load_addon(SearchAddon);
//! ```

use crate::manager::TerminalManager;

/// Addon 上下文
///
/// 提供给 Addon 访问终端能力的接口。
pub struct AddonContext<'a> {
    /// 终端管理器引用
    pub manager: &'a mut TerminalManager,
}

/// Addon 插件 trait
///
/// 类似 xterm.js 的 `ITerminalAddon`。
pub trait Addon: Send {
    /// 激活插件
    fn activate(&mut self, ctx: &mut AddonContext);

    /// 销毁插件
    fn dispose(&mut self);

    /// 插件名称
    fn name(&self) -> &str {
        "unnamed"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TerminalSize;

    struct TestAddon {
        activated: bool,
        disposed: bool,
    }

    impl Addon for TestAddon {
        fn activate(&mut self, _ctx: &mut AddonContext) {
            self.activated = true;
        }

        fn dispose(&mut self) {
            self.disposed = true;
        }

        fn name(&self) -> &str {
            "test"
        }
    }

    #[test]
    fn test_addon_lifecycle() {
        let mut addon = TestAddon {
            activated: false,
            disposed: false,
        };

        let mut mgr = TerminalManager::utf8(TerminalSize::new(24, 80));
        let mut ctx = AddonContext { manager: &mut mgr };

        addon.activate(&mut ctx);
        assert!(addon.activated);

        addon.dispose();
        assert!(addon.disposed);

        assert_eq!(addon.name(), "test");
    }
}
