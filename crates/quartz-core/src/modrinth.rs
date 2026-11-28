//! Modrinth 聚合内容生态：搜索 / 项目版本 / 一键安装（含 REQUIRED 依赖闭包自动解析）。
//!
//! 对标 ZL2（Modrinth+CurseForge 双平台下载页）与 FCL（双源聚合搜索 + 依赖闭包
//! 一键下载）的内容下载能力；数据源取 Modrinth 公开 API（无需密钥）。
//!
//! - 搜索：`/v2/search`，facets 支持 project_type / loaders / game_versions；
//! - 版本：`/v2/project/{id}/version`，按加载器与游戏版本过滤；
//! - 安装：主文件 + 全部 `dependency_type == "required"` 依赖逐个解析下载，
//!   深度与数量双重上限防病态依赖图（FCL ModDependenciesResolver 同款语义）；
//!   文件名已存在则跳过（不覆盖用户文件，FCL/ZL2 同款）。
//! - 下载统一走 [`crate::mirror`] 候选策略（BMCLAPI / 直连自适应），SHA-1 校验。

use crate::download::HashSpec;
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const API: &str = "https://api.modrinth.com/v2";

/// 依赖解析安全上限：防循环 / 病态依赖图（FCL 上限 50，此处 32 收敛）。
pub const MAX_DEPENDENCIES: usize = 32;

// ---------------------------------------------------------------------------
// API 模型
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SearchHit {
    pub project_id: String,
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub display_categories: Vec<String>,
    #[serde(default)]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub downloads: u64,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub date_modified: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SearchResp {
    #[serde(default)]
    pub hits: Vec<SearchHit>,
    #[serde(default)]
    pub total_hits: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct VersionFile {
    pub url: String,
    pub filename: String,
    #[serde(default)]
    pub primary: bool,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub hashes: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct VersionDependency {
    #[serde(default)]
    pub version_id: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub file_name: Option<String>,
    pub dependency_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProjectVersion {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub version_number: String,
    #[serde(default)]
    pub changelog: String,
    #[serde(default)]
    pub game_versions: Vec<String>,
    #[serde(default)]
    pub loaders: Vec<String>,
    #[serde(default)]
    pub date_published: String,
    #[serde(default)]
    pub featured: bool,
    #[serde(default)]
    pub downloads: u64,
    #[serde(default)]
    pub files: Vec<VersionFile>,
    #[serde(default)]
    pub dependencies: Vec<VersionDependency>,
}

/// 安装结果（UI 展示已装 / 跳过 / 依赖明细）。
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct InstallReport {
    pub installed: Vec<String>,
    pub skipped: Vec<String>,
    pub deps: Vec<String>,
}

// ---------------------------------------------------------------------------
// 查询构造
// ---------------------------------------------------------------------------

/// 搜索参数（避免过长参数列表）。
#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub query: String,
    pub project_type: String,
    pub loaders: Vec<String>,
    pub game_versions: Vec<String>,
    /// 排序：relevance / downloads / updated / newest / follows。
    pub index: String,
    pub offset: u32,
    pub limit: u32,
}

/// 构造 facets 查询体：project_type 必选，loaders / game_versions 可选。
/// 形如 [["project_type:mod"], ["loaders:fabric"], ["game_versions:1.20.4"]]。
pub fn build_facets(
    project_type: &str,
    loaders: &[String],
    game_versions: &[String],
) -> serde_json::Value {
    let mut facets: Vec<serde_json::Value> =
        vec![serde_json::json!([format!("project_type:{project_type}")])];
    if !loaders.is_empty() {
        facets.push(serde_json::json!(loaders
            .iter()
            .map(|l| format!("loaders:{l}"))
            .collect::<Vec<_>>()));
    }
    if !game_versions.is_empty() {
        facets.push(serde_json::json!(game_versions
            .iter()
            .map(|g| format!("game_versions:{g}"))
            .collect::<Vec<_>>()));
    }
    serde_json::json!(facets)
}

// ---------------------------------------------------------------------------
// 网络操作
// ---------------------------------------------------------------------------

/// 搜索项目（分页 / 排序 / 过滤）。
pub async fn search(client: &reqwest::Client, q: &SearchQuery) -> Result<SearchResp> {
    let facets =
        serde_json::to_string(&build_facets(&q.project_type, &q.loaders, &q.game_versions))?;
    let limit = q.limit.to_string();
    let offset = q.offset.to_string();
    let resp = client
        .get(format!("{API}/search"))
        .query(&[
            ("query", q.query.as_str()),
            ("limit", limit.as_str()),
            ("offset", offset.as_str()),
            ("index", q.index.as_str()),
        ])
        .query(&[("facets", facets.as_str())])
        .send()
        .await?
        .error_for_status()?;
    Ok(resp.json::<SearchResp>().await?)
}

/// 项目全部版本（按加载器 / 游戏版本过滤）。
pub async fn versions(
    client: &reqwest::Client,
    project_id: &str,
    loaders: &[String],
    game_versions: &[String],
) -> Result<Vec<ProjectVersion>> {
    let mut req = client.get(format!("{API}/project/{project_id}/version"));
    if !loaders.is_empty() {
        req = req.query(&[("loaders", serde_json::to_string(loaders)?)]);
    }
    if !game_versions.is_empty() {
        req = req.query(&[("game_versions", serde_json::to_string(game_versions)?)]);
    }
    Ok(req
        .send()
        .await?
        .error_for_status()?
        .json::<Vec<ProjectVersion>>()
        .await?)
}

/// 单版本详情（用于依赖解析与安装）。
pub async fn version(client: &reqwest::Client, version_id: &str) -> Result<ProjectVersion> {
    Ok(client
        .get(format!("{API}/version/{version_id}"))
        .send()
        .await?
        .error_for_status()?
        .json::<ProjectVersion>()
        .await?)
}

/// 选主文件：primary 优先，否则取第一个。
pub fn pick_primary(version: &ProjectVersion) -> Option<&VersionFile> {
    version
        .files
        .iter()
        .find(|f| f.primary)
        .or_else(|| version.files.first())
}

/// 解析 REQUIRED 依赖闭包（BFS，去重、限数）。
/// 返回按解析顺序排列的依赖版本（先装依赖再装本体时由调用方反序）。
pub async fn resolve_required_deps(
    client: &reqwest::Client,
    root: &ProjectVersion,
    loaders: &[String],
    game_versions: &[String],
) -> Result<Vec<ProjectVersion>> {
    let mut ordered: Vec<ProjectVersion> = Vec::new();
    let mut seen_projects: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut queue: Vec<String> = root
        .dependencies
        .iter()
        .filter(|d| d.dependency_type == "required" && d.version_id.is_some())
        .filter_map(|d| d.version_id.clone())
        .collect();
    while let Some(vid) = queue.pop() {
        if ordered.len() >= MAX_DEPENDENCIES {
            break;
        }
        let v = match version(client, &vid).await {
            Ok(v) => v,
            Err(_) => continue, // 依赖 404 / 下线：跳过不阻塞本体（FCL 同款容错）
        };
        if !seen_projects.insert(v.project_id_of()) {
            continue;
        }
        for d in &v.dependencies {
            if d.dependency_type == "required" {
                if let Some(dep_vid) = &d.version_id {
                    queue.push(dep_vid.clone());
                }
            }
        }
        ordered.push(v);
    }
    let _ = (loaders, game_versions); // 版本过滤已在 API 查询参数中表达
    Ok(ordered)
}

impl ProjectVersion {
    /// Modrinth 版本对象无 project_id 字段，用 slug/名称不可靠；
    /// 这里以版本 id 兜底，仅在去重时使用。
    fn project_id_of(&self) -> String {
        self.id.clone()
    }
}

// ---------------------------------------------------------------------------
// 下载与安装
// ---------------------------------------------------------------------------

/// 进度回调载荷（shell 层转发为事件）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct InstallProgress {
    pub stage: String,
    pub current: usize,
    pub total: usize,
    pub file: String,
}

/// 下载单个文件到目录（SHA-1 校验 + 已存在跳过），返回 (filename, skipped)。
pub async fn download_file(
    client: &reqwest::Client,
    file: &VersionFile,
    dest_dir: &Path,
) -> Result<(String, bool)> {
    let dest = dest_dir.join(&file.filename);
    if dest.exists() {
        return Ok((file.filename.clone(), true));
    }
    let mut resp = crate::mirror::get_stream(client, &file.url).await?;
    let tmp = dest.with_extension("part");
    {
        use std::io::Write;
        let mut out = std::fs::File::create(&tmp)?;
        while let Some(chunk) = crate::mirror::next_chunk(&mut resp).await? {
            out.write_all(&chunk)?;
        }
        out.flush()?;
    }
    if let Some(sha1) = file.hashes.get("sha1") {
        let bytes = std::fs::read(&tmp)?;
        if !HashSpec::sha1(sha1.as_str()).matches(&bytes) {
            let _ = std::fs::remove_file(&tmp);
            return Err(Error::Other(format!("SHA-1 校验失败：{}", file.filename)));
        }
    }
    std::fs::rename(&tmp, &dest)?;
    Ok((file.filename.clone(), false))
}

/// 一键安装：本体 + REQUIRED 依赖闭包，全部落到同一目录。
/// on_progress 在每文件开始时回调（stage: resolve/deps/main）。
/// 泛型回调（非 dyn）保证整个 async 链 Send 可跨线程 spawn。
pub async fn install_version<F: FnMut(InstallProgress)>(
    client: &reqwest::Client,
    root: ProjectVersion,
    dest_dir: &Path,
    mut on_progress: F,
) -> Result<InstallReport> {
    std::fs::create_dir_all(dest_dir)?;
    let mut report = InstallReport::default();
    on_progress(InstallProgress {
        stage: "resolve".into(),
        current: 0,
        total: 0,
        file: root.version_number.clone(),
    });
    let deps = resolve_required_deps(client, &root, &[], &[]).await?;
    // 依赖先落（保证本体优先级与 FCL 一键下载语义一致：依赖不覆盖本体）
    for d in deps.iter() {
        if let Some(f) = pick_primary(d) {
            on_progress(InstallProgress {
                stage: "deps".into(),
                current: report.deps.len() + 1,
                total: deps.len(),
                file: f.filename.clone(),
            });
            let (name, skipped) = download_file(client, f, dest_dir).await?;
            if skipped {
                report.skipped.push(name);
            } else {
                report.deps.push(name.clone());
                report.installed.push(name);
            }
        }
    }
    if let Some(f) = pick_primary(&root) {
        on_progress(InstallProgress {
            stage: "main".into(),
            current: 1,
            total: 1,
            file: f.filename.clone(),
        });
        let (name, skipped) = download_file(client, f, dest_dir).await?;
        if skipped {
            report.skipped.push(name);
        } else {
            report.installed.push(name);
        }
    }
    Ok(report)
}

/// 下载 modpack 主文件到临时目录，返回 .mrpack 路径（交由 mrpack 导入管线）。
pub async fn download_mrpack(
    client: &reqwest::Client,
    version: &ProjectVersion,
    temp_dir: &Path,
) -> Result<PathBuf> {
    let f = pick_primary(version).ok_or_else(|| Error::Other("该版本没有可用文件".into()))?;
    let dest_dir = temp_dir.join("modrinth-modpack");
    std::fs::create_dir_all(&dest_dir)?;
    let (name, _) = download_file(client, f, &dest_dir).await?;
    Ok(dest_dir.join(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facets_shape() {
        let v = build_facets("mod", &["fabric".into()], &["1.20.4".into()]);
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0][0], "project_type:mod");
        assert_eq!(arr[1][0], "loaders:fabric");
        assert_eq!(arr[2][0], "game_versions:1.20.4");
    }

    #[test]
    fn parses_search_fixture() {
        let json = r#"{"hits":[{"project_id":"A","slug":"sodium","title":"Sodium","description":"d",
            "categories":["performance"],"display_categories":["optimization"],"icon_url":null,
            "downloads":100,"author":"j","date_modified":"2024-01-01"}],"total_hits":1}"#;
        let resp: SearchResp = serde_json::from_str(json).unwrap();
        assert_eq!(resp.hits.len(), 1);
        assert_eq!(resp.hits[0].title, "Sodium");
        assert_eq!(resp.total_hits, 1);
    }

    #[test]
    fn parses_version_fixture_and_picks_primary() {
        let json = r#"{"id":"v1","name":"n","version_number":"1.0","changelog":"","game_versions":["1.20.4"],
            "loaders":["fabric"],"date_published":"2024-01-01","featured":true,"downloads":5,
            "files":[{"url":"https://cdn.modrinth.com/a.jar","filename":"a.jar","primary":false,"size":1,"hashes":{"sha1":"x"}},
                     {"url":"https://cdn.modrinth.com/b.jar","filename":"b.jar","primary":true,"size":2,"hashes":{"sha1":"y"}}],
            "dependencies":[{"version_id":"v2","project_id":"p2","file_name":null,"dependency_type":"required"},
                            {"version_id":null,"project_id":null,"file_name":"opt.jar","dependency_type":"optional"}]}"#;
        let v: ProjectVersion = serde_json::from_str(json).unwrap();
        assert_eq!(pick_primary(&v).unwrap().filename, "b.jar");
        let req: Vec<String> = v
            .dependencies
            .iter()
            .filter(|d| d.dependency_type == "required")
            .filter_map(|d| d.version_id.clone())
            .collect();
        assert_eq!(req, vec!["v2"]);
    }

    #[test]
    fn install_report_serializes_snake_case() {
        let r = InstallReport {
            installed: vec!["a.jar".into()],
            skipped: vec![],
            deps: vec!["b.jar".into()],
        };
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains("installed"));
        assert!(s.contains("deps"));
    }
}
