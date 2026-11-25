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

/// 统一 HTTP 客户端（rustls TLS；10s 连接超时；供全部在线下载使用，
/// 流停滞检测由 mirror::next_chunk 负责，不设整体超时以免大文件被拦腰报错）。
pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent("Quartz/0.1.0 (Rust Minecraft Launcher; offline)")
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .expect("failed to build http client")
}

pub async fn fetch_version_manifest(client: &reqwest::Client) -> Result<VersionManifest> {
    // 镜像回退：BMCLAPI 优先，官方直连兑底（见 mirror 模块）。
    let text = crate::mirror::get_text(client, VERSION_MANIFEST_URL).await?;
    parse_version_manifest(&text)
}

pub fn parse_version_manifest(text: &str) -> Result<VersionManifest> {
    serde_json::from_str(text).map_err(Into::into)
}

pub async fn fetch_version_json(client: &reqwest::Client, url: &str) -> Result<VersionJson> {
    let text = crate::mirror::get_text(client, url).await?;
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
/// 本地私有库约定：url 为空字符串 -> 本地安装的库（无下载任务，仅 classpath 引用）。
pub fn library_jar_url(lib: &Library) -> Option<String> {
    if let Some(dl) = &lib.downloads {
        if let Some(a) = &dl.artifact {
            if a.url.is_empty() {
                return None;
            }
            return Some(a.url.clone());
        }
    }
    if let Some(base) = &lib.url {
        if base.is_empty() {
            return None;
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

/// 规则过滤后的 natives 分类器下载任务（落 libraries/<maven 路径>）。
///
/// 兼容两种形态：
/// 1. 旧格式 `natives: { linux: "natives-linux" }` -> classifiers[映射名]；
/// 2. 现代版本（LWJGL 3.3+）直接取 classifiers["natives-linux"]，
///    arm64 上下文额外取 classifiers["natives-linux-arm64"]。
pub fn collect_native_tasks(
    version: &VersionJson,
    ctx: &RuntimeCtx,
    libraries_root: &std::path::Path,
) -> Vec<crate::download::DownloadTask> {
    use std::collections::BTreeSet;
    let mut tasks = Vec::new();
    let mut seen = BTreeSet::new();
    for lib in &version.libraries {
        if !library_allowed(lib, ctx) {
            continue;
        }
        let Some(downloads) = &lib.downloads else {
            continue;
        };
        let Some(classifiers) = &downloads.classifiers else {
            continue;
        };
        let mut wanted: Vec<&crate::model::Artifact> = Vec::new();
        if let Some(natives) = &lib.natives {
            if let Some(key) = natives.get(&ctx.os) {
                if let Some(artifact) = classifiers.get(key) {
                    wanted.push(artifact);
                }
            }
        }
        if let Some(artifact) = classifiers.get("natives-linux") {
            wanted.push(artifact);
        }
        if ctx.arch == "arm64" {
            if let Some(artifact) = classifiers.get("natives-linux-arm64") {
                wanted.push(artifact);
            }
        }
        for artifact in wanted {
            if seen.insert(artifact.path.clone()) {
                tasks.push(crate::download::DownloadTask {
                    source: crate::download::Source::Url(artifact.url.clone()),
                    dest: libraries_root.join(&artifact.path),
                    hash: Some(HashSpec::sha1(artifact.sha1.clone())),
                    size: Some(artifact.size),
                    label: format!("native:{}", lib.name),
                });
            }
        }
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
        // 防御：短哈希会让 &hash[..2] 切片直接 panic（整进程 abort）——
        // 损坏/半截资产索引必须报错而不是崩溃（v8 闪退加固）。
        let Some(prefix) = hash.get(..2) else {
            return Err(crate::error::Error::Other(format!(
                "资产索引含非法哈希「{}」（长度不足 2），索引文件损坏，请重新下载该版本",
                obj.hash
            )));
        };
        let url = format!("https://resources.download.minecraft.net/{prefix}/{hash}");
        tasks.push(crate::download::DownloadTask {
            source: crate::download::Source::Url(url),
            dest: assets_root.join("objects").join(prefix).join(&hash),
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

// ---------------------------------------------------------------------------
// inheritsFrom 链解析（loader 版本 -> 原版基版本）
//
// 对标官方启动器 / HMCL / Prism 的合并语义：
// - libraries：child 在前 + parent 补齐，按 maven 坐标去重（child 优先）；
// - arguments.jvm / arguments.game：parent 在前、child 追加，精确重复去重；
// - mainClass / minecraftArguments：child 有则覆盖（minecraftArguments 拼接）；
// - assetIndex / assets / javaVersion / downloads：child 缺失时回退 parent；
// - client jar：child "jar" 字段 || 链根版本 id。
// ---------------------------------------------------------------------------

/// 链解析结果：合并后的版本 + 实际 client jar 所属版本 id。
pub struct ResolvedVersion {
    pub version: crate::model::VersionJson,
    /// client jar 所在版本目录（"jar" 字段覆盖 || 链根 id）。
    pub jar_version: String,
    /// 链根版本 id（原版），客户端 jar / natives 校验用它。
    pub base_id: String,
}

const MAX_CHAIN_DEPTH: usize = 10;

/// 读单个版本 JSON（typed），未安装时报错。
pub fn load_version(version_id: &str) -> Result<crate::model::VersionJson> {
    let path = crate::paths::version_json(version_id);
    let text = std::fs::read_to_string(&path).map_err(|_| {
        Error::Other(format!(
            "版本 {version_id} 未安装（缺少 {}），请先在「下载游戏」页下载",
            path.display()
        ))
    })?;
    serde_json::from_str(&text)
        .map_err(|e| Error::Other(format!("版本 {version_id} JSON 解析失败：{e}")))
}

/// 解析 inheritsFrom 链并合并为可启动版本。
/// 循环继承 / 深度超限 / 任一环节缺失都会给出可行动报错。
pub fn resolve_version_chain(version_id: &str) -> Result<ResolvedVersion> {
    // ① 从 child 到 base 收集链（typed 解析保留继承信息）
    let mut chain: Vec<crate::model::VersionJson> = Vec::new();
    let mut cur = version_id.to_string();
    loop {
        let v = load_version(&cur)?;
        let parent = v.inherits_from.clone();
        chain.push(v);
        match parent {
            Some(p) if chain.len() < MAX_CHAIN_DEPTH => cur = p,
            Some(_) => {
                return Err(Error::Other(format!(
                    "版本继承链超过 {MAX_CHAIN_DEPTH} 层（疑似循环继承）：{version_id}"
                )))
            }
            None => break,
        }
    }
    let base_id = cur.clone();

    // ② jar 覆盖字段：任意层声明 "jar" 则采用（取最靠近 child 的声明）
    let jar_version = chain[0].jar.clone().unwrap_or_else(|| base_id.clone());

    // ③ 从 base 向 child 逐层合并
    //    合并载体用 Value 以便无损处理参数数组（Str / Ruled 混合项）。
    let mut merged: Value = serde_json::to_value(chain.last().unwrap())?;
    for child in chain[..chain.len() - 1].iter().rev() {
        merge_parent_child(&mut merged, child)?;
    }

    // ④ child 的 id 必须保留（class 目录 / natives 目录都以它命名）
    merged["id"] = Value::String(version_id.to_string());

    let version: crate::model::VersionJson = serde_json::from_str(&merged.to_string())?;
    Ok(ResolvedVersion {
        version,
        jar_version,
        base_id,
    })
}

/// 把一层 child 合并进 accumulated（acc 语义上是「当前父版本」）。
fn merge_parent_child(acc: &mut Value, child: &crate::model::VersionJson) -> Result<()> {
    // - mainClass：child 覆盖
    if !child.main_class.is_empty() {
        acc["mainClass"] = Value::String(child.main_class.clone());
    }
    // - jar 字段：child 覆盖（jar_version 已在 resolve 层处理，这里同步进 JSON）
    if let Some(j) = &child.jar {
        acc["jar"] = Value::String(j.clone());
    }
    // - assetIndex / assets / javaVersion / downloads：child 有则覆盖
    let field_overrides: Vec<(&str, Option<Value>)> = vec![
        (
            "assetIndex",
            child
                .asset_index
                .as_ref()
                .and_then(|v| serde_json::to_value(v).ok()),
        ),
        (
            "assets",
            child
                .assets
                .as_ref()
                .and_then(|v| serde_json::to_value(v).ok()),
        ),
        (
            "javaVersion",
            child
                .java_version
                .as_ref()
                .and_then(|v| serde_json::to_value(v).ok()),
        ),
        (
            "downloads",
            if child.downloads.is_empty() {
                None
            } else {
                serde_json::to_value(&child.downloads).ok()
            },
        ),
    ];
    for (key, value) in field_overrides {
        if let Some(v) = value {
            acc[key] = v;
        }
    }
    // - minecraftArguments（旧格式）：parent 在前、child 追加
    if let Some(ma) = &child.minecraft_arguments {
        let parent = acc
            .get("minecraftArguments")
            .and_then(Value::as_str)
            .unwrap_or("");
        let combined = if parent.is_empty() {
            ma.clone()
        } else {
            format!("{parent} {ma}")
        };
        acc["minecraftArguments"] = Value::String(combined);
    }
    // - arguments（新格式）：jvm / game 均 parent 在前、child 追加；精确重复去重
    let acc_jvm = acc
        .get("arguments")
        .and_then(|a| a.get("jvm"))
        .cloned()
        .unwrap_or(Value::Array(vec![]));
    let acc_game = acc
        .get("arguments")
        .and_then(|a| a.get("game"))
        .cloned()
        .unwrap_or(Value::Array(vec![]));
    let child_jvm = serde_json::to_value(child.arguments.as_ref().map(|a| &a.jvm))?;
    let child_game = serde_json::to_value(child.arguments.as_ref().map(|a| &a.game))?;
    let mut jvm = as_array_or_empty(&acc_jvm);
    let mut game = as_array_or_empty(&acc_game);
    for v in as_array_or_empty(&child_jvm) {
        if !array_contains_exact(&jvm, &v) {
            jvm.push(v);
        }
    }
    for v in as_array_or_empty(&child_game) {
        if !array_contains_exact(&game, &v) {
            game.push(v);
        }
    }
    acc["arguments"] = serde_json::json!({ "jvm": jvm, "game": game });
    // - libraries：child 在前（优先），parent 补齐；按 name 去重
    let mut libs: Vec<Value> = Vec::new();
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let push_libs =
        |items: &Value, seen: &mut std::collections::BTreeSet<String>, libs: &mut Vec<Value>| {
            if let Some(arr) = items.as_array() {
                for item in arr {
                    let name = item
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    if name.is_empty() || seen.insert(name) {
                        libs.push(item.clone());
                    }
                }
            }
        };
    push_libs(
        &serde_json::to_value(&child.libraries)?,
        &mut seen,
        &mut libs,
    );
    push_libs(
        acc.get("libraries").unwrap_or(&Value::Null),
        &mut seen,
        &mut libs,
    );
    acc["libraries"] = Value::Array(libs);
    Ok(())
}

fn as_array_or_empty(v: &Value) -> Vec<Value> {
    v.as_array().cloned().unwrap_or_default()
}

/// 精确值比较（字符串全等 / 对象全等）。
fn array_contains_exact(arr: &[Value], needle: &Value) -> bool {
    arr.iter().any(|v| v == needle)
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
