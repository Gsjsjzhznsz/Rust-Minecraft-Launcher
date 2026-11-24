//! Modloader 元数据获取（对标 FCL / ZL2 的安装器数据源）：
//! - Fabric / Quilt：官方 meta API（profile/json 即可直接当版本 JSON 使用）；
//! - Forge / NeoForge：maven-metadata.xml 版本列表 + 官方 installer 下载；
//! - OptiFine：BMCLAPI 版本列表与下载（官方站点广告墙不可直连）。
//!
//! 安装编排（写版本 JSON / 下载库 / 跑 installer）在 instance::install_loader。

use crate::error::{Error, Result};
use crate::meta::http_client;
use serde::{Deserialize, Serialize};

pub use crate::modkit::LoaderVersion;

// ---------------------------------------------------------------------------
// Fabric / Quilt（meta API v2，端点与响应结构与官方文档一致）
// ---------------------------------------------------------------------------

pub const FABRIC_META: &str = "https://meta.fabricmc.net/v2";
pub const QUILT_META: &str = "https://meta.quiltmc.org/v2";

/// 某个 MC 版本可用的 loader 版本列表（stable 优先排序）。
pub async fn list_fabric_loaders(mc_version: &str) -> Result<Vec<LoaderVersion>> {
    list_meta_loaders(&format!("{FABRIC_META}/versions/loader/{mc_version}")).await
}

pub async fn list_quilt_loaders(mc_version: &str) -> Result<Vec<LoaderVersion>> {
    list_meta_loaders(&format!("{QUILT_META}/versions/loader/{mc_version}")).await
}

/// 官方 profile JSON（可直接写为版本 JSON 的完整启动定义，含 inheritsFrom）。
pub async fn fabric_profile_json(mc_version: &str, loader: &str) -> Result<String> {
    profile_json(&format!(
        "{FABRIC_META}/versions/loader/{mc_version}/{loader}/profile/json"
    ))
    .await
}

pub async fn quilt_profile_json(mc_version: &str, loader: &str) -> Result<String> {
    profile_json(&format!(
        "{QUILT_META}/versions/loader/{mc_version}/{loader}/profile/json"
    ))
    .await
}

async fn list_meta_loaders(url: &str) -> Result<Vec<LoaderVersion>> {
    let client = http_client();
    let resp = crate::mirror::get_text(&client, url).await?;
    let mut list: Vec<LoaderVersion> = serde_json::from_str(&resp)?;
    list.sort_by(|a, b| b.stable.cmp(&a.stable));
    Ok(list)
}

async fn profile_json(url: &str) -> Result<String> {
    let client = http_client();
    let text = crate::mirror::get_text(&client, url).await?;
    // 基本校验：profile 必须声明 inheritsFrom 与 mainClass，否则安装后无法启动
    let v: serde_json::Value = serde_json::from_str(&text)?;
    if v.get("inheritsFrom").is_none() || v.get("mainClass").is_none() {
        return Err(Error::Other(format!("loader profile 不完整（缺少 inheritsFrom/mainClass）：{url}")));
    }
    Ok(text)
}

// ---------------------------------------------------------------------------
// Forge / NeoForge（maven-metadata.xml）
// ---------------------------------------------------------------------------

pub const FORGE_MAVEN: &str = "https://maven.minecraftforge.net/maven";
pub const NEOFORGE_MAVEN: &str = "https://maven.neoforged.net/releases";

/// NeoForge 版本列表（maven-metadata.xml -> versions/version[]）。
pub async fn list_neoforge_versions(mc_version: &str) -> Result<Vec<String>> {
    let url = format!("{NEOFORGE_MAVEN}/net/neoforged/neoforge/maven-metadata.xml");
    let xml = fetch_text(&url).await?;
    let all = parse_maven_versions(&xml)?;
    // NeoForge 版本号形如 21.4.47（主版本 = MC 版本）或 1.20.4-xxx；
    // 新版 NeoForge 用 MC 主版本号开头（如 21.4.x 对应 1.21.4）。
    let major_prefix = mc_major(mc_version);
    let out: Vec<String> = all
        .into_iter()
        .filter(|v| {
            v.starts_with(&major_prefix) || v.starts_with(mc_version)
        })
        .collect();
    if out.is_empty() {
        return Err(Error::Other(format!(
            "NeoForge 没有 {mc_version} 的构建（可用版本请看 {url}）"
        )));
    }
    Ok(out)
}

/// Forge 版本列表（promotions 不够全，直接用 metadata 全列表按前缀过滤）。
pub async fn list_forge_versions(mc_version: &str) -> Result<Vec<String>> {
    let url = format!("{FORGE_MAVEN}/net/minecraftforge/forge/maven-metadata.xml");
    let xml = fetch_text(&url).await?;
    let all = parse_maven_versions(&xml)?;
    let out: Vec<String> = all
        .into_iter()
        .filter(|v| v.starts_with(&format!("{mc_version}-")))
        .map(|v| v.trim_start_matches(&format!("{mc_version}-")).to_string())
        .collect();
    if out.is_empty() {
        return Err(Error::Other(format!("Forge 没有 {mc_version} 的构建")));
    }
    Ok(out)
}

/// Forge installer jar 下载地址。
pub fn forge_installer_url(mc_version: &str, forge_version: &str) -> String {
    format!("{FORGE_MAVEN}/net/minecraftforge/forge/{mc_version}-{forge_version}/forge-{mc_version}-{forge_version}-installer.jar")
}

/// NeoForge installer jar 下载地址。
pub fn neoforge_installer_url(neoforge_version: &str) -> String {
    format!("{NEOFORGE_MAVEN}/net/neoforged/neoforge/{neoforge_version}/neoforge-{neoforge_version}-installer.jar")
}

/// 解析 maven-metadata.xml 的 <versions> 列表（轻量字符串解析，无 xml crate 依赖）。
pub fn parse_maven_versions(xml: &str) -> Result<Vec<String>> {
    let start = xml
        .find("<versions>")
        .ok_or_else(|| Error::Other("maven-metadata.xml 缺少 <versions>".into()))?;
    let end = xml
        .find("</versions>")
        .ok_or_else(|| Error::Other("maven-metadata.xml 缺少 </versions>".into()))?;
    let body = &xml[start + "<versions>".len()..end];
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(s) = rest.find("<version>") {
        let after = &rest[s + "<version>".len()..];
        let e = after
            .find("</version>")
            .ok_or_else(|| Error::Other("maven-metadata.xml 版本标记未闭合".into()))?;
        out.push(after[..e].to_string());
        rest = &after[e + "</version>".len()..];
    }
    if out.is_empty() {
        return Err(Error::Other("maven-metadata.xml 没有版本条目".into()));
    }
    Ok(out)
}

/// MC 版本主版本号（"1.21.4" -> "21"，"1.20.1" -> "20"，快照按 1.x 推断失败则原样返回）。
fn mc_major(mc_version: &str) -> String {
    let parts: Vec<&str> = mc_version.split('.').collect();
    if parts.len() >= 2 && parts[0] == "1" {
        parts[1].to_string()
    } else {
        mc_version.to_string()
    }
}

async fn fetch_text(url: &str) -> Result<String> {
    let client = http_client();
    crate::mirror::get_text(&client, url).await
}

// ---------------------------------------------------------------------------
// OptiFine（BMCLAPI：官方站广告墙 + 安装器需 GUI，BMCLAPI 提供直装包）
// ---------------------------------------------------------------------------

/// BMCLAPI OptiFine 版本列表：GET /optifine/{mcVersion}
/// 返回 [{ name, mcVersion, type, patch, ... }]。
pub async fn list_optifine_versions(mc_version: &str) -> Result<Vec<OptifineEntry>> {
    let url = format!(
        "https://bmclapi2.bangbang93.com/optifine/{mc_version}"
    );
    let text = fetch_text(&url).await?;
    let list: Vec<OptifineEntry> = serde_json::from_str(&text)?;
    if list.is_empty() {
        return Err(Error::Other(format!("BMCLAPI 没有 {mc_version} 的 OptiFine 构建")));
    }
    Ok(list)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OptifineEntry {
    pub name: String,
    #[serde(default)]
    pub mc_version: String,
    #[serde(default)]
    pub r#type: String,
    #[serde(default)]
    pub patch: String,
    #[serde(default)]
    pub file: String,
}

/// OptiFine 直装包（BMCLAPI /optifine/{mcVersion}/{type}/{patch}/download，
/// 指向 OptiFine jar 本体）。安装（tweaker 版本 JSON 生成）在 instance::install_loader。
pub fn optifine_download_url(mc_version: &str, of_type: &str, patch: &str) -> String {
    format!("https://bmclapi2.bangbang93.com/optifine/{mc_version}/{of_type}/{patch}/download")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_maven_metadata() {
        let xml = r#"<?xml version="1.0"?>
<metadata><versioning><versions>
<version>21.4.47-beta</version><version>21.4.46</version><version>1.20.4-49.0.30</version>
</versions></versioning></metadata>"#;
        let v = parse_maven_versions(xml).unwrap();
        assert_eq!(v, vec!["21.4.47-beta", "21.4.46", "1.20.4-49.0.30"]);
    }

    #[test]
    fn mc_major_extraction() {
        assert_eq!(mc_major("1.21.4"), "21");
        assert_eq!(mc_major("1.20"), "20");
    }

    #[test]
    fn forge_url_shape() {
        assert_eq!(
            forge_installer_url("1.20.1", "47.2.0"),
            format!("{FORGE_MAVEN}/net/minecraftforge/forge/1.20.1-47.2.0/forge-1.20.1-47.2.0-installer.jar")
        );
        assert_eq!(
            neoforge_installer_url("21.4.47"),
            format!("{NEOFORGE_MAVEN}/net/neoforged/neoforge/21.4.47/neoforge-21.4.47-installer.jar")
        );
    }

    #[test]
    fn optifine_entry_parses() {
        let json = r#"[{ "name": "OptiFine HD U I6", "mcVersion": "1.21.4", "type": "HD", "patch": "I6", "file": "OptiFine_1.21.4_HD_U_I6.jar" }]"#;
        let list: Vec<OptifineEntry> = serde_json::from_str(json).unwrap();
        assert_eq!(list[0].patch, "I6");
    }
}
