//! Quartz Core：启动器「大脑」。纯逻辑、零平台依赖。
//!
//! 模块地图（对应 docs/ARCHITECTURE.md 分层）：
//! - [`model`]   Mojang 元数据模型（manifest / 版本 JSON / 资产索引）
//! - [`rules`]   规则引擎（os/arch/features）
//! - [`meta`]    版本清单获取与下载计划推导
//! - [`download`] 下载编排（SHA-1/256/512 校验 / 断点跳过 / 并发 / 进度事件）
//! - [`paths`]   标准目录布局唯一权威（与官方 .minecraft / ZL2 互通）
//! - [`instance`] 实例体系（隔离目录 + 共享仓库）
//! - [`jvmops`]  JVM 启动计划装配（五端统一契约）
//! - [`auth`]    账号（离线 UUID / 微软设备码 + XBL 链路）
//! - [`modkit`]  内容生态（mrpack 整合包 / modloader 元数据）
//! - [`renderbus`] 渲染路由（gl4es/ANGLE/Zink/MetalANGLE/Dozen 选择矩阵）
//! - [`inputbus`] 输入协议（触摸→指针/视角状态机）
//! - [`platform`] 平台 HAL（桌面子进程 / 移动端进程内）
//! - [`uipack`]  ui-pack 清单/校验/灰度/回滚（L2 热更协议核心侧）
//!
//! 测试数据全部内嵌于 `fixtures`（合成样例，结构与真实清单一致，哈希为占位）。

pub mod auth;
pub mod content;
pub mod download;
pub mod error;
pub mod inputbus;
pub mod instance;
pub mod jvmops;
pub mod lwjgl;
pub mod meta;
pub mod mirror;
pub mod model;
pub mod modkit;
pub mod paths;
pub mod platform;
pub mod renderbus;
pub mod renderers;
pub mod rules;
pub mod runtime;
pub mod settings;
pub mod space;
pub mod uipack;

/// 内嵌测试夹具（合成样例数据）。
pub mod fixtures {
    pub const VERSION_MANIFEST_SAMPLE: &str =
        include_str!("../fixtures/version_manifest_sample.json");
    pub const VERSION_1_20_4: &str = include_str!("../fixtures/version_1_20_4.json");
}

pub use auth::Account;
pub use error::{Error, Result};
pub use platform::{host_arch, host_os};
pub use rules::RuntimeCtx;
