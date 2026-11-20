//! 版本元数据获取与「下载计划」推导。
//!
//! 启动器大脑的第一条链路：
//! version_manifest_v2 -> 版本 JSON -> 按规则过滤后的库/客户端/资产下载任务。

use crate::download::HashSpec;
use crate::error::{Error, Result};
use crate::model::{
    maven_rel_path, AssetIndex, Library, ManifestVersion, VersionJson, VersionManifest,
};
use crate::rules::RuntimeCtx;
use serde_json::Value;
use std::path::PathBuf;

pub const VERSION_MANIFEST_URL: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
pub const LIBRARIES_BASE: &str = "https://libraries.minecraft.net/";

pub async fn fetch_version_manifest(client: &reqwest::Client) -> Result<VersionManifest> {
    let text = client
        .get(VERSION_MANIFEST_URL)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    parse_version_manifest(&text)
}

pub fn parse_version_manifest(text: &str) -> Result<VersionManifest> {
    serde_json::from_str(text).map_err(Into::into)
}

pub async fn fetch_version_json(client: &reqwest::Client, url: &str) -> Result<VersionJson> {
    let text = client
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    parse_version_json(&text)
}

pub fn parse_version_json(text: &str) -> Result<VersionJson> {
    serde_json::from_str(text).map_err(Into::into)
}

/// 找出指定版本清单项。
pub fn find_version<'a>(m: &'a VersionManifest, id: &str) -> Option<&'a ManifestVersion> {
    m.find(id)
}

// ---------------------------------------------------------------------------
// 库解析
// ---------------------------------------------------------------------------

/// 返回该库在当前上下文下是否可用（应用 rules）。
pub fn library_allowed(lib: &Library, ctx: &RuntimeCtx) -> bool {
    crate::rules::evaluate(&lib.rules, ctx)
}

/// 计算库的 jar 下载 URL：
/// 优先 downloads.artifact.url；否则旧格式 url 基址 + Maven 相对路径；
/// 再否则 Mojang 默认仓库。
pub fn library_jar_url(lib: &Library) -> Option<String> {
    if let Some(dl) = &lib.downloads {
        if let Some(a) = &dl.artifact {
            return Some(a.url.clone());
        }
    }
    let rel = maven_rel_path(&lib.name, None)?;
    Some(match &lib.url {
        Some(base) => format!("{}/{}", base.trim_end_matches('/'), rel),
        None => format!("{LIBRARIES_BASE}{rel}"),
    })
}

/// 计算库 jar 在本地仓库中的相对路径。
pub fn library_jar_path(lib: &Library) -> Option<String> {
    if let Some(dl) = &lib.downloads {
        if let Some(a) = &dl.artifact {
            return Some(a.path.clone());
        }
    }
    maven_rel_path(&lib.name, None)
}

/// 规则过滤后的主 jar（非 natives classifier）下载任务列表。
pub fn collect_library_tasks(
    version: &VersionJson,
    ctx: &RuntimeCtx,
    libraries_root: &std::path::Path,
) -> Vec<crate::download::DownloadTask> {
    let mut tasks = Vec::new();
    for lib in &version.libraries {
        if !library_allowed(lib, ctx) {
            continue;
        }
        let (Some(url), Some(rel)) = (library_jar_url(lib), library_jar_path(lib)) else {
            continue;
        };
        let hash = lib
            .downloads
            .as_ref()
            .and_then(|d| d.artifact.as_ref())
            .map(|a| HashSpec::sha1(a.sha1.clone()));
        let size = lib
            .downloads
            .as_ref()
            .and_then(|d| d.artifact.as_ref())
            .map(|a| a.size);
        tasks.push(crate::download::DownloadTask {
            source: crate::download::Source::Url(url),
            dest: libraries_root.join(&rel),
            hash,
            size,
            label: format!("lib:{}", lib.name),
        });
    }
    tasks
}

/// 客户端主 jar 任务。
pub fn client_jar_task(
    version: &VersionJson,
    versions_root: &std::path::Path,
) -> Option<crate::download::DownloadTask> {
    let client = version.downloads.get("client")?;
    let dest = match &client.path {
        Some(p) => versions_root.join(p),
        None => versions_root
            .join(&version.id)
            .join(format!("{}.jar", version.id)),
    };
    Some(crate::download::DownloadTask {
        source: crate::download::Source::Url(client.url.clone()),
        dest,
        hash: client.sha1.clone().map(HashSpec::sha1),
        size: client.size,
        label: format!("client:{}", version.id),
    })
}

/// 资产索引 JSON 任务。
pub fn asset_index_task(
    version: &VersionJson,
    assets_root: &std::path::Path,
) -> Option<crate::download::DownloadTask> {
    let idx = version.asset_index.as_ref()?;
    Some(crate::download::DownloadTask {
        source: crate::download::Source::Url(idx.url.clone()),
        dest: assets_root.join("indexes").join(format!("{}.json", idx.id)),
        hash: idx.sha1.clone().map(HashSpec::sha1),
        size: idx.total_size,
        label: format!("asset-index:{}", idx.id),
    })
}

/// 资产索引展开为按哈希分桶的对象任务（objects/<hash[:2]>/<hash>）。
pub fn expand_asset_index(
    index_text: &str,
    assets_root: &std::path::Path,
) -> Result<Vec<crate::download::DownloadTask>> {
    let index: AssetIndex = serde_json::from_str(index_text)?;
    let mut tasks = Vec::new();
    for obj in index.objects.values() {
        let hash = obj.hash.to_ascii_lowercase();
        let url = format!(
            "https://resources.download.minecraft.net/{}/{}",
            &hash[..2],
            hash
        );
        tasks.push(crate::download::DownloadTask {
            source: crate::download::Source::Url(url),
            dest: assets_root.join("objects").join(&hash[..2]).join(&hash),
            hash: Some(HashSpec::sha1(hash.clone())),
            size: Some(obj.size),
            label: format!("asset:{hash}"),
        });
    }
    Ok(tasks)
}

/// 从版本 JSON 提取资产索引名（新格式优先）。
pub fn assets_index_name(version: &VersionJson) -> Option<String> {
    version
        .asset_index
        .as_ref()
        .map(|i| i.id.clone())
        .or_else(|| version.assets.clone())
}

/// 解析版本 JSON 中可能隐藏在 misc 字段的信息（保留给未来兼容层）。
pub fn version_json_summary(v: &Value) -> Result<(String, String)> {
    let id = v
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Other("version json missing id".into()))?
        .to_string();
    let kind = v
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    Ok((id, kind))
}

/// 便捷：PathBuf 组装。
pub fn objects_dir(assets_root: &std::path::Path) -> PathBuf {
    assets_root.join("objects")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maven_path_basic() {
        assert_eq!(
            maven_rel_path("com.mojang:brigadier:1.1.8", None).unwrap(),
            "com/mojang/brigadier/1.1.8/brigadier-1.1.8.jar"
        );
        assert_eq!(
            maven_rel_path("org.lwjgl:lwjgl:3.3.1", Some("natives-macos")).unwrap(),
            "org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1-natives-macos.jar"
        );
    }

    #[test]
    fn asset_index_expansion_buckets_by_prefix() {
        let idx = json!({ "objects": { "minecraft/sounds/test.ogg": { "hash": "abcdef1234abcdef1234abcdef1234abcdef1234a", "size": 123 } } });
        let tasks = expand_asset_index(&idx.to_string(), std::path::Path::new("/assets")).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(
            tasks[0].dest,
            std::path::Path::new("/assets/objects/ab/abcdef1234abcdef1234abcdef1234abcdef1234a")
        );
        assert!(tasks[0]
            .source
            .source_url()
            .contains("https://resources.download.minecraft.net/ab/"));
    }
}
