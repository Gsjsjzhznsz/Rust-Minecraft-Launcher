//! Mojang 元数据模型：version_manifest_v2 与版本 JSON（piston-meta）。
//!
//! 字段命名与真实清单一致（camelCase）；本 crate 只建模启动器关心的子集，
//! 未建模的字段会被 serde 忽略，保证对新增字段的向前兼容。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// version_manifest_v2
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionManifest {
    pub latest: ManifestLatest,
    pub versions: Vec<ManifestVersion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestLatest {
    pub release: String,
    pub snapshot: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestVersion {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub url: String,
    pub sha1: String,
}

impl VersionManifest {
    pub fn find(&self, id: &str) -> Option<&ManifestVersion> {
        self.versions.iter().find(|v| v.id == id)
    }
    pub fn by_kind(&self, kind: &str) -> Vec<&ManifestVersion> {
        self.versions.iter().filter(|v| v.kind == kind).collect()
    }
}

// ---------------------------------------------------------------------------
// 版本 JSON（<version>/<id>.json）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionJson {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub main_class: String,
    /// 1.13+ 新格式参数（对象数组）。
    #[serde(default)]
    pub arguments: Option<Arguments>,
    /// 1.12- 旧格式参数（空格分隔字符串）。
    #[serde(default)]
    pub minecraft_arguments: Option<String>,
    #[serde(default)]
    pub libraries: Vec<Library>,
    #[serde(default)]
    pub asset_index: Option<AssetIndexMeta>,
    /// 旧格式资产索引名（如 "legacy"），新格式优先用 asset_index.id。
    #[serde(default)]
    pub assets: Option<String>,
    #[serde(default)]
    pub java_version: Option<JavaVersion>,
    /// 键通常为 "client" / "server" / "mappings"。
    #[serde(default)]
    pub downloads: BTreeMap<String, VersionArtifact>,
    #[serde(default)]
    pub release_time: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Arguments {
    #[serde(default)]
    pub game: Vec<ArgEntry>,
    #[serde(default)]
    pub jvm: Vec<ArgEntry>,
}

/// 参数数组元素：纯字符串，或带规则的对象（值可能是字符串或字符串数组）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ArgEntry {
    Str(String),
    Ruled { rules: Vec<Rule>, value: ArgValue },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ArgValue {
    One(String),
    Many(Vec<String>),
}

// ---------------------------------------------------------------------------
// 规则
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub action: RuleAction,
    #[serde(default)]
    pub os: Option<OsRule>,
    /// 特性开关规则，如 is_demo_user / has_custom_resolution。
    #[serde(default)]
    pub features: Option<serde_json::Map<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleAction {
    Allow,
    Disallow,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OsRule {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arch: Option<String>,
}

// ---------------------------------------------------------------------------
// 库
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Library {
    /// Maven 坐标，如 "org.lwjgl:lwjgl:3.3.1"。
    pub name: String,
    #[serde(default)]
    pub downloads: Option<LibraryDownloads>,
    #[serde(default)]
    pub rules: Vec<Rule>,
    /// 旧格式 natives 声明：os -> classifier 名。
    #[serde(default)]
    pub natives: Option<BTreeMap<String, String>>,
    /// 旧格式第三方 Maven 仓库基址（会与坐标拼出下载路径）。
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryDownloads {
    #[serde(default)]
    pub artifact: Option<Artifact>,
    #[serde(default)]
    pub classifiers: Option<BTreeMap<String, Artifact>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub path: String,
    pub url: String,
    pub sha1: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetIndexMeta {
    pub id: String,
    pub url: String,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub total_size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaVersion {
    pub major_version: u32,
    #[serde(default)]
    pub component: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionArtifact {
    pub url: String,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub path: Option<String>,
}

// ---------------------------------------------------------------------------
// 资产索引（assetIndex.url 指向的 JSON）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetIndex {
    #[serde(default)]
    pub objects: BTreeMap<String, AssetObject>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetObject {
    pub hash: String,
    pub size: u64,
}

// ---------------------------------------------------------------------------
// 工具：Maven 坐标 -> 相对路径
// ---------------------------------------------------------------------------

/// "group:artifact:version[:classifier]" -> "group/path/artifact/version/artifact-version[-classifier].jar"
pub fn maven_rel_path(coords: &str, classifier: Option<&str>) -> Option<String> {
    let parts: Vec<&str> = coords.split(':').collect();
    if parts.len() < 3 {
        return None;
    }
    let (group, artifact, version) = (parts[0], parts[1], parts[2]);
    let ext = parts.get(3).copied().unwrap_or("jar");
    let group_path = group.replace('.', "/");
    let file = match classifier {
        Some(c) if !c.is_empty() => format!("{artifact}-{version}-{c}.{ext}"),
        _ => format!("{artifact}-{version}.{ext}"),
    };
    Some(format!("{group_path}/{artifact}/{version}/{file}"))
}
