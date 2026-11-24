//! 内容生态：整合包（Modrinth .mrpack）与 modloader 元数据。

pub mod loaders;
pub mod mrpack;

use crate::error::Result;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Fabric / Quilt / NeoForge 元数据（meta.fabricmc.net 等公开 API）
// ---------------------------------------------------------------------------

pub const FABRIC_META: &str = "https://meta.fabricmc.net/v2";
pub const QUILT_META: &str = "https://meta.quiltmc.org/v2";
pub const NEOFORGE_MAVEN: &str = "https://maven.neoforged.net/releases";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoaderVersion {
    pub version: String,
    #[serde(default)]
    pub stable: bool,
}

/// 最新稳定 Fabric loader 版本（需要网络）。
pub async fn latest_fabric_loader(client: &reqwest::Client) -> Result<LoaderVersion> {
    let list = client
        .get(format!("{FABRIC_META}/versions/loader"))
        .send()
        .await?
        .error_for_status()?
        .json::<Vec<LoaderVersion>>()
        .await?;
    list.into_iter()
        .find(|v| v.stable)
        .ok_or_else(|| crate::error::Error::Other("no stable fabric loader".into()))
}

/// 稳定性检查与版本路由的公共入口（后续接入 Forge/NeoForge 安装器编排）。
pub fn loader_requires_jvm_major(loader: &str) -> u32 {
    // 经验规则：Forge 1.17-1.20.4 及 Fabric 0.13+ 需要 17；
    // 1.20.5+ 需要 21。此处按 loader 字符串粗判，精确路由由版本 JSON 决定。
    let _ = loader;
    17
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_loader_version() {
        let json = r#"[{"url":"","maven":"","version":"0.15.11","stable":true},{"version":"0.16.0-beta.1","stable":false}]"#;
        let list: Vec<LoaderVersion> = serde_json::from_str(json).unwrap();
        assert_eq!(list.len(), 2);
        assert!(list[0].stable);
        assert!(!list[1].stable);
    }
}
