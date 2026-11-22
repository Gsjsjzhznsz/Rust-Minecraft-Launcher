//! 实例体系：隔离的游戏目录 + 共享的全局仓库（assets / libraries / runtimes）。
//!
//! 目录布局（全局根 root/）：
//! ```text
//! root/
//! ├── instances/<id>/{saves,mods,resourcepacks,shaderpacks,config,logs,crash-reports}
//! ├── assets/{indexes,objects}
//! ├── libraries/
//! ├── versions/<id>/
//! ├── runtimes/<os>/<arch>/<jvm-major>/
//! └── renderpacks/
//! ```
//! 游戏以 `--gameDir instances/<id>` 运行；类路径与资产目录指向全局仓库，
//! 避免多实例重复占盘。instances.json 与 root 元数据由本模块读写。

use crate::error::Result;
use crate::rules::RuntimeCtx;
use serde::{Deserialize, Serialize};
use sha1::Digest as _;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LoaderSpec {
    Fabric { loader_version: String },
    Quilt { loader_version: String },
    NeoForge { version: String },
    Forge { version: String },
    OptiFine { version: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
    pub id: String,
    pub name: String,
    pub mc_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loader: Option<LoaderSpec>,
    pub java_major: u32,
    /// 为空表示按全局 runtimes 目录解析。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub java_path: Option<PathBuf>,
    pub memory_mb: u32,
    /// 渲染器 pack id（见 renderbus），空表示自动选择。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub renderer: Option<String>,
    pub created_at: String,
}

/// 新建实例的输入。
#[derive(Debug, Clone)]
pub struct CreateInstance {
    pub id: String,
    pub name: String,
    pub mc_version: String,
    pub loader: Option<LoaderSpec>,
    pub java_major: u32,
    pub memory_mb: u32,
}

pub struct InstanceStore {
    /// 启动器全局根目录。
    root: PathBuf,
}

impl InstanceStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn instances_dir(&self) -> PathBuf {
        self.root.join("instances")
    }

    pub fn index_file(&self) -> PathBuf {
        self.instances_dir().join("instances.json")
    }

    pub fn instance_dir(&self, id: &str) -> PathBuf {
        self.instances_dir().join(id)
    }

    pub fn assets_root(&self) -> PathBuf {
        self.root.join("assets")
    }

    pub fn libraries_root(&self) -> PathBuf {
        self.root.join("libraries")
    }

    pub fn versions_root(&self) -> PathBuf {
        self.root.join("versions")
    }

    pub fn runtimes_root(&self) -> PathBuf {
        self.root.join("runtimes")
    }

    pub fn renderpacks_root(&self) -> PathBuf {
        self.root.join("renderpacks")
    }

    /// 读取实例列表；文件不存在视为空列表。
    pub fn load(&self) -> Result<Vec<Instance>> {
        let path = self.index_file();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let text = std::fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&text)?)
    }

    fn save(&self, instances: &[Instance]) -> Result<()> {
        std::fs::create_dir_all(self.instances_dir())?;
        let text = serde_json::to_string_pretty(instances)?;
        let tmp = self.index_file().with_extension("json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, self.index_file())?;
        Ok(())
    }

    /// 创建实例目录并登记。目录不存在则创建；id 重复报错。
    pub fn create(&self, input: CreateInstance) -> Result<Instance> {
        let mut instances = self.load()?;
        if instances.iter().any(|i| i.id == input.id) {
            return Err(crate::error::Error::Other(format!(
                "instance id already exists: {}",
                input.id
            )));
        }
        let dir = self.instance_dir(&input.id);
        for sub in [
            "saves",
            "mods",
            "resourcepacks",
            "shaderpacks",
            "config",
            "logs",
            "crash-reports",
        ] {
            std::fs::create_dir_all(dir.join(sub))?;
        }
        let inst = Instance {
            id: input.id,
            name: input.name,
            mc_version: input.mc_version,
            loader: input.loader,
            java_major: input.java_major,
            java_path: None,
            memory_mb: input.memory_mb,
            renderer: None,
            created_at: iso_now(),
        };
        instances.push(inst.clone());
        self.save(&instances)?;
        Ok(inst)
    }

    /// 删除实例目录与登记。
    pub fn delete(&self, id: &str) -> Result<bool> {
        let mut instances = self.load()?;
        let before = instances.len();
        instances.retain(|i| i.id != id);
        if instances.len() == before {
            return Ok(false);
        }
        self.save(&instances)?;
        let dir = self.instance_dir(id);
        if dir.exists() {
            std::fs::remove_dir_all(dir)?;
        }
        Ok(true)
    }
}

/// 返回 ISO8601/UTC 风格时间戳（秒精度）。
/// 不引入时间库：UNIX epoch + 手工格式化（YYYY-MM-DDTHH:MM:SSZ）。
pub fn iso_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // civil_from_days（Howard Hinnant 算法）
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

// ---------------------------------------------------------------------------
// 标准目录布局安装（蓝图第三节下载链路：与官方 .minecraft 互通）
//
// 链路：① version_manifest_v2 -> ② 版本 JSON（SHA-1 校验并落盘）
//      -> ③ client jar -> ④ libraries（rules 过滤 os=linux + natives 分类器）
//      -> ⑤ assetIndex + objects（resources.download.minecraft.net）
// 全部经 download::execute：8 并发、每文件 SHA-1 校验、失败重试 3 次、
// 已存在且校验通过自动跳过（可安全重试）。
// ---------------------------------------------------------------------------

/// 下载进度事件（壳层直接转发到 WebView `download://progress`）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    /// manifest | version-json | client | libraries | asset-index | assets | done | error
    pub stage: String,
    /// 已完成字节（含跳过）。
    pub current: u64,
    /// 计划总字节。
    pub total: u64,
    pub percent: f64,
    /// 当前文件（任务标签）。
    pub file: String,
    pub done: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 下载并发数（蓝图规定 8）。
pub const DOWNLOAD_CONCURRENCY: usize = 8;

/// 标准布局安装的规则上下文：os 按 linux 处理（Mojang 清单没有 android 条目），
/// arch 沿用 host（arm64 / x86_64）。
pub fn standard_rules_ctx() -> RuntimeCtx {
    RuntimeCtx::new("linux", crate::host_arch())
}

fn download_progress(stage: &str, file: &str, current: u64, total: u64, done: bool, error: Option<String>) -> DownloadProgress {
    let percent = if total > 0 && !done {
        ((current as f64 / total as f64) * 100.0).clamp(0.0, 100.0)
    } else if done {
        100.0
    } else {
        0.0
    };
    DownloadProgress {
        stage: stage.to_string(),
        current,
        total,
        percent,
        file: file.to_string(),
        done,
        error,
    }
}

fn label_stage(label: &str) -> &'static str {
    if label.starts_with("client:") {
        "client"
    } else if label.starts_with("lib:") || label.starts_with("native:") {
        "libraries"
    } else if label.starts_with("asset-index:") {
        "asset-index"
    } else if label.starts_with("asset:") {
        "assets"
    } else {
        "prepare"
    }
}

async fn send_progress(tx: &tokio::sync::mpsc::Sender<DownloadProgress>, ev: DownloadProgress) {
    let _ = tx.send(ev).await;
}

/// 拉取文本并做可选 SHA-1 校验（镜像回退由 mirror 模块负责）。
async fn fetch_text_verified(
    client: &reqwest::Client,
    url: &str,
    sha1: Option<&str>,
) -> Result<String> {
    let bytes = crate::mirror::get_bytes(client, url).await?;
    if let Some(expect) = sha1 {
        let spec = crate::download::HashSpec::sha1(expect);
        if !spec.matches(&bytes) {
            let got = hex::encode(sha1::Sha1::digest(&bytes));
            return Err(crate::error::Error::ShaMismatch(
                url.to_string(),
                expect.to_string(),
                got,
            ));
        }
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// 按标准目录布局完整安装一个版本。进度经 mpsc 通道上报。
///
/// 幂等：重复调用会跳过已存在且哈希正确的文件，可直接用于失败重试。
pub async fn install_standard(
    client: &reqwest::Client,
    version_id: &str,
    progress: tokio::sync::mpsc::Sender<DownloadProgress>,
) -> Result<()> {
    crate::paths::ensure_layout()?;

    // ① 版本清单
    send_progress(
        &progress,
        download_progress("manifest", "version_manifest_v2.json", 0, 0, false, None),
    )
    .await;
    let manifest = crate::meta::fetch_version_manifest(client).await?;
    let entry = manifest
        .find(version_id)
        .cloned()
        .ok_or_else(|| crate::error::Error::Other(format!("版本清单中未找到版本：{version_id}")))?;

    // ② 版本 JSON（SHA-1 校验） -> versions/<v>/<v>.json
    send_progress(
        &progress,
        download_progress("version-json", &format!("{version_id}.json"), 0, 0, false, None),
    )
    .await;
    let json_text = fetch_text_verified(client, &entry.url, Some(&entry.sha1)).await?;
    let version: crate::model::VersionJson = serde_json::from_str(&json_text)?;
    let vdir = crate::paths::version_dir(version_id);
    tokio::fs::create_dir_all(&vdir).await?;
    tokio::fs::write(crate::paths::version_json(version_id), &json_text).await?;

    // ③④ 任务收集：client jar / libraries / natives（os=linux 规则过滤）
    let ctx = standard_rules_ctx();
    let libs_root = crate::paths::libraries_dir();
    let mut tasks: Vec<crate::download::DownloadTask> = Vec::new();
    let mut seen_dest = std::collections::BTreeSet::new();

    let client_task = crate::meta::client_jar_task(&version, &crate::paths::versions_dir())
        .ok_or_else(|| {
            crate::error::Error::Other(format!("版本 {version_id} 缺少 client 下载信息"))
        })?;
    seen_dest.insert(client_task.dest.clone());
    tasks.push(client_task);

    for task in crate::meta::collect_library_tasks(&version, &ctx, &libs_root)
        .into_iter()
        .chain(crate::meta::collect_native_tasks(&version, &ctx, &libs_root))
    {
        if seen_dest.insert(task.dest.clone()) {
            tasks.push(task);
        }
    }

    // ⑤ 资源索引 + 对象（先取索引内容再展开，保证进度百分比的总字节一次性已知）
    if let Some(idx) = &version.asset_index {
        send_progress(
            &progress,
            download_progress("asset-index", &format!("{}.json", idx.id), 0, 0, false, None),
        )
        .await;
        let index_text = fetch_text_verified(client, &idx.url, idx.sha1.as_deref()).await?;
        tokio::fs::create_dir_all(crate::paths::asset_indexes_dir()).await?;
        tokio::fs::write(crate::paths::asset_index_file(&idx.id), &index_text).await?;
        for task in crate::meta::expand_asset_index(&index_text, &crate::paths::minecraft_dir())? {
            if seen_dest.insert(task.dest.clone()) {
                tasks.push(task);
            }
        }
    }

    let total: u64 = tasks.iter().filter_map(|t| t.size).sum();
    // 任务元数据：label -> (stage, size, dest)（供进度换算与跳过补算）
    let mut meta_by_label = std::collections::BTreeMap::new();
    for t in &tasks {
        meta_by_label.insert(
            t.label.clone(),
            (
                label_stage(&t.label),
                t.size.unwrap_or(0),
                t.dest.clone(),
            ),
        );
    }

    // ⑥ 并行下载：8 并发、单文件 3 次重试、SHA-1 校验、.qz-part 原子改名
    let (tx, mut rx) = tokio::sync::mpsc::channel::<crate::download::ProgressEvent>(64);
    let plan = crate::download::DownloadPlan::new(tasks);
    let exec_client = client.clone();
    let exec = tokio::spawn(async move {
        crate::download::execute(&plan, &exec_client, DOWNLOAD_CONCURRENCY, tx).await
    });

    let mut done_bytes: u64 = 0;
    let mut last_emit = std::time::Instant::now() - std::time::Duration::from_secs(1);
    let mut stage = "prepare".to_string();
    let mut file = String::new();
    let mut failure: Option<String> = None;

    async fn push_progress(
        tx: &tokio::sync::mpsc::Sender<DownloadProgress>,
        force: bool,
        stage: &str,
        file: &str,
        done_bytes: u64,
        total: u64,
        last_emit: &mut std::time::Instant,
    ) {
        let now = std::time::Instant::now();
        if !force && now.duration_since(*last_emit) < std::time::Duration::from_millis(150) {
            return;
        }
        *last_emit = now;
        send_progress(tx, download_progress(stage, file, done_bytes, total, false, None)).await;
    }

    while let Some(ev) = rx.recv().await {
        match ev {
            crate::download::ProgressEvent::TaskStart { label } => {
                if let Some((s, _, _)) = meta_by_label.get(&label) {
                    stage = s.to_string();
                }
                file = label;
                push_progress(&progress, true, &stage, &file, done_bytes, total, &mut last_emit).await;
            }
            crate::download::ProgressEvent::TaskDone { bytes, .. } => {
                done_bytes += bytes;
                push_progress(&progress, false, &stage, &file, done_bytes, total, &mut last_emit).await;
            }
            crate::download::ProgressEvent::TaskSkipped { label } => {
                let size = match meta_by_label.get(&label) {
                    Some((_, sz, dest)) if *sz > 0 => *sz,
                    Some((_, _, dest)) => {
                        std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0)
                    }
                    None => 0,
                };
                done_bytes += size;
                push_progress(&progress, false, &stage, &file, done_bytes, total, &mut last_emit).await;
            }
            crate::download::ProgressEvent::TaskFailed { label, error } => {
                failure = Some(format!("{label}: {error}"));
            }
        }
    }

    let summary = exec
        .await
        .map_err(|e| crate::error::Error::Other(format!("下载执行器崩溃：{e}")))??;
    if summary.failed > 0 || failure.is_some() {
        let detail = failure.unwrap_or_default();
        return Err(crate::error::Error::Other(format!(
            "部分文件下载失败（已自动重试 3 次）：{detail}"
        )));
    }

    send_progress(
        &progress,
        download_progress("done", "", total, total, true, None),
    )
    .await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_load_delete_roundtrip() {
        let root = test_root("instance-roundtrip");
        let store = InstanceStore::new(&root);
        assert!(store.load().unwrap().is_empty());

        let inst = store
            .create(CreateInstance {
                id: "dev-1".into(),
                name: "开发服整合".into(),
                mc_version: "1.20.4".into(),
                loader: Some(LoaderSpec::Fabric {
                    loader_version: "0.15.11".into(),
                }),
                java_major: 17,
                memory_mb: 2048,
            })
            .unwrap();

        assert!(store.instance_dir("dev-1").join("saves").exists());
        assert!(store.instance_dir("dev-1").join("shaderpacks").exists());

        let loaded = store.load().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].mc_version, "1.20.4");

        // 重复 id 报错
        assert!(store
            .create(CreateInstance {
                id: "dev-1".into(),
                name: "dup".into(),
                mc_version: "1.20.4".into(),
                loader: None,
                java_major: 17,
                memory_mb: 2048,
            })
            .is_err());

        assert!(store.delete("dev-1").unwrap());
        assert!(!store.instance_dir("dev-1").exists());
        assert!(store.load().unwrap().is_empty());
        assert_eq!(inst.java_major, 17);
    }

    #[test]
    fn iso_now_shape() {
        let t = iso_now();
        assert_eq!(t.len(), 20);
        assert!(t.ends_with('Z'));
        assert_eq!(&t[4..5], "-");
    }

    fn test_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("qz-inst-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
