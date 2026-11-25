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

    /// 修改实例字段（渲染器 / 内存 / loader 等），原子写回索引。
    pub fn update(&self, id: &str, patch: impl FnOnce(&mut Instance)) -> Result<Instance> {
        let mut instances = self.load()?;
        let inst = instances
            .iter_mut()
            .find(|i| i.id == id)
            .ok_or_else(|| crate::error::Error::Other(format!("实例不存在：{id}")))?;
        patch(inst);
        let updated = inst.clone();
        self.save(&instances)?;
        Ok(updated)
    }

    /// 按 id 查找实例（不存在返回 None）。
    pub fn get(&self, id: &str) -> Result<Option<Instance>> {
        Ok(self.load()?.into_iter().find(|i| i.id == id))
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

fn download_progress(
    stage: &str,
    file: &str,
    current: u64,
    total: u64,
    done: bool,
    error: Option<String>,
) -> DownloadProgress {
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
        download_progress(
            "version-json",
            &format!("{version_id}.json"),
            0,
            0,
            false,
            None,
        ),
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
        .chain(crate::meta::collect_native_tasks(
            &version, &ctx, &libs_root,
        ))
    {
        if seen_dest.insert(task.dest.clone()) {
            tasks.push(task);
        }
    }

    // ⑤ 资源索引 + 对象（先取索引内容再展开，保证进度百分比的总字节一次性已知）
    if let Some(idx) = &version.asset_index {
        send_progress(
            &progress,
            download_progress(
                "asset-index",
                &format!("{}.json", idx.id),
                0,
                0,
                false,
                None,
            ),
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

    // 存储空间前置检查：只统计真正缺失的任务（重试时已存在文件不计入需求），
    // 外加 64MB 对齐/临时副本余量。空间不足在此给出可行动报错，
    // 而不是下到一半 ENOSPC（os error 28）。
    let pending: u64 = tasks
        .iter()
        .filter(|t| !t.dest.exists())
        .filter_map(|t| t.size)
        .sum();
    if pending > 0 {
        let need = pending + 64 * 1024 * 1024;
        crate::space::ensure_free(
            &crate::paths::minecraft_dir(),
            need,
            &format!(
                "下载 {version_id}（约 {}）",
                crate::space::human_bytes(pending)
            ),
        )
        .map_err(crate::error::Error::Other)?;
    }

    // ⑥ 并行下载（共享执行器，进度换算与失败聚合一致）
    execute_download_plan(tasks, &progress).await
}

/// 共享下载执行器：8 并发、SHA-1 校验、断点跳过、失败聚合与进度上报。
/// install_standard 与 install_loader 共用，保证两类安装的进度语义一致。
pub async fn execute_download_plan(
    tasks: Vec<crate::download::DownloadTask>,
    progress: &tokio::sync::mpsc::Sender<DownloadProgress>,
) -> Result<()> {
    // 任务元数据：label -> (stage, size, dest)（供进度换算与跳过补算）
    let mut meta_by_label = std::collections::BTreeMap::new();
    for t in &tasks {
        meta_by_label.insert(
            t.label.clone(),
            (label_stage(&t.label), t.size.unwrap_or(0), t.dest.clone()),
        );
    }

    // ⑥ 并行下载：8 并发、单文件 3 次重试、SHA-1 校验、.qz-part 原子改名
    let (tx, mut rx) = tokio::sync::mpsc::channel::<crate::download::ProgressEvent>(64);
    let plan = crate::download::DownloadPlan::new(tasks);
    let exec_client = crate::meta::http_client();
    let exec = tokio::spawn(async move {
        crate::download::execute(&plan, &exec_client, DOWNLOAD_CONCURRENCY, tx).await
    });

    let total: u64 = {
        // plan 已被 move，从元数据聚合总字节
        meta_by_label.values().map(|(_, sz, _)| *sz).sum::<u64>()
    };

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
        send_progress(
            tx,
            download_progress(stage, file, done_bytes, total, false, None),
        )
        .await;
    }

    while let Some(ev) = rx.recv().await {
        match ev {
            crate::download::ProgressEvent::TaskStart { label } => {
                if let Some((s, _, _)) = meta_by_label.get(&label) {
                    stage = s.to_string();
                }
                file = label;
                push_progress(
                    progress,
                    true,
                    &stage,
                    &file,
                    done_bytes,
                    total,
                    &mut last_emit,
                )
                .await;
            }
            crate::download::ProgressEvent::TaskDone { bytes, .. } => {
                done_bytes += bytes;
                push_progress(
                    progress,
                    false,
                    &stage,
                    &file,
                    done_bytes,
                    total,
                    &mut last_emit,
                )
                .await;
            }
            crate::download::ProgressEvent::TaskSkipped { label } => {
                let size = match meta_by_label.get(&label) {
                    Some((_, sz, dest)) if *sz > 0 => *sz,
                    Some((_, _, dest)) => std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0),
                    None => 0,
                };
                done_bytes += size;
                push_progress(
                    progress,
                    false,
                    &stage,
                    &file,
                    done_bytes,
                    total,
                    &mut last_emit,
                )
                .await;
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
        progress,
        download_progress("done", "", total, total, true, None),
    )
    .await;
    Ok(())
}

// ---------------------------------------------------------------------------
// Modloader 安装（Fabric / Quilt / Forge / NeoForge / OptiFine）
//
// 对标 FCL / ZL2 的安装器编排：
// - Fabric / Quilt：官方 meta profile JSON（自带 inheritsFrom）直接落位为
//   版本 JSON，再下载其 libraries —— 纯 Rust，不需要跑 JVM 安装器；
// - Forge / NeoForge：下载官方 installer，用已装运行时 java 以
//   `--installClient` 无头模式安装（官方支持，写 versions/<id>/ 与 libraries）；
// - OptiFine：BMCLAPI 直装包 + 自生成 tweaker 版本 JSON（1.12- 完整支持；
//   1.13+ OptiFine 官方安装器需 GUI patch，暂不承诺）。
//
// 基版本缺失时自动先安装原版（install_standard 串联，进度同一通道）。
// ---------------------------------------------------------------------------

/// 从 profile JSON 提取版本 id（fabric meta 的 id 形如 fabric-loader-0.15.11-1.20.4）。
fn profile_version_id(text: &str) -> Result<String> {
    let v: serde_json::Value = serde_json::from_str(text)?;
    Ok(v.get("id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string())
}

/// loader 版本安装的统一入口。返回安装后的版本 id。
pub async fn install_loader(
    client: &reqwest::Client,
    mc_version: &str,
    spec: &LoaderSpec,
    progress: tokio::sync::mpsc::Sender<DownloadProgress>,
) -> Result<String> {
    crate::paths::ensure_layout()?;

    match spec {
        LoaderSpec::Fabric { loader_version } | LoaderSpec::Quilt { loader_version } => {
            install_meta_loader(client, mc_version, spec, loader_version, progress).await
        }
        LoaderSpec::Forge { version } => {
            install_forge_like(
                client,
                mc_version,
                &crate::modkit::loaders::forge_installer_url(mc_version, version),
                progress,
            )
            .await
        }
        LoaderSpec::NeoForge { version } => {
            install_forge_like(
                client,
                mc_version,
                &crate::modkit::loaders::neoforge_installer_url(version),
                progress,
            )
            .await
        }
        LoaderSpec::OptiFine { version } => {
            install_optifine(client, mc_version, version, progress).await
        }
    }
}

/// Fabric / Quilt：profile JSON 落位 + 库下载。
async fn install_meta_loader(
    client: &reqwest::Client,
    mc_version: &str,
    spec: &LoaderSpec,
    loader_version: &str,
    progress: tokio::sync::mpsc::Sender<DownloadProgress>,
) -> Result<String> {
    // ① 基版本必须就绪（缺则串联安装原版，进度同一通道）
    if !crate::paths::version_json(mc_version).exists() {
        send_progress(
            &progress,
            download_progress(
                "prepare",
                &format!("基版本 {mc_version} 未安装，自动先安装"),
                0,
                0,
                false,
                None,
            ),
        )
        .await;
        install_standard(client, mc_version, progress.clone()).await?;
    }

    // ② profile JSON
    let text = match spec {
        LoaderSpec::Fabric { .. } => {
            crate::modkit::loaders::fabric_profile_json(mc_version, loader_version).await?
        }
        LoaderSpec::Quilt { .. } => {
            crate::modkit::loaders::quilt_profile_json(mc_version, loader_version).await?
        }
        _ => unreachable!("install_meta_loader 只处理 Fabric/Quilt"),
    };
    let vid = profile_version_id(&text)?;
    if vid.is_empty() {
        return Err(crate::error::Error::Other(
            "loader profile 缺少 id，无法确定版本目录".into(),
        ));
    }
    send_progress(
        &progress,
        download_progress("loader-json", &format!("{vid}.json"), 0, 0, false, None),
    )
    .await;
    let profile: crate::model::VersionJson = serde_json::from_str(&text)?;
    let vdir = crate::paths::version_dir(&vid);
    tokio::fs::create_dir_all(&vdir).await?;
    tokio::fs::write(crate::paths::version_json(&vid), &text).await?;

    // ③ 库任务（继承版本只装自身库；client jar / assets 由基版本提供）
    let ctx = standard_rules_ctx();
    let libs_root = crate::paths::libraries_dir();
    let tasks = meta_tasks(&profile, &ctx, &libs_root);
    // 空任务集也要走执行器（保持 done 事件一致性）
    execute_download_plan(tasks, &progress).await?;
    Ok(vid)
}

/// 汇集一个版本定义的库 + natives 下载任务（去重）。
fn meta_tasks(
    version: &crate::model::VersionJson,
    ctx: &crate::rules::RuntimeCtx,
    libs_root: &std::path::Path,
) -> Vec<crate::download::DownloadTask> {
    let mut tasks: Vec<crate::download::DownloadTask> = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for task in crate::meta::collect_library_tasks(version, ctx, libs_root)
        .into_iter()
        .chain(crate::meta::collect_native_tasks(version, ctx, libs_root))
    {
        if seen.insert(task.dest.clone()) {
            tasks.push(task);
        }
    }
    tasks
}

/// Forge / NeoForge：下载官方 installer，用已装运行时 java 无头安装。
/// installer 会写 versions/<id>/ 与 libraries/（与标准布局一致）。
async fn install_forge_like(
    client: &reqwest::Client,
    mc_version: &str,
    installer_url: &str,
    progress: tokio::sync::mpsc::Sender<DownloadProgress>,
) -> Result<String> {
    // ① 基版本必须就绪
    if !crate::paths::version_json(mc_version).exists() {
        send_progress(
            &progress,
            download_progress(
                "prepare",
                &format!("基版本 {mc_version} 未安装，自动先安装"),
                0,
                0,
                false,
                None,
            ),
        )
        .await;
        install_standard(client, mc_version, progress.clone()).await?;
    }

    // ② 安装 installer 运行所需的 java 运行时（Forge 1.17+ 要 16+；按基版本 javaVersion）
    let base = crate::meta::load_version(mc_version)?;
    let major = base
        .java_version
        .as_ref()
        .map(|j| j.major_version)
        .unwrap_or(8);
    if !crate::runtime::is_installed(major) {
        return Err(crate::error::Error::MissingConfig(format!(
            "安装 Forge/NeoForge 需要运行时 JRE {major}，请先在「设置 → 运行时」下载"
        )));
    }

    // ③ 下载 installer jar
    send_progress(
        &progress,
        download_progress("loader-installer", "installer.jar", 0, 0, false, None),
    )
    .await;
    let mut resp = crate::mirror::get_stream(client, installer_url).await?;
    let mut data: Vec<u8> = Vec::new();
    while let Some(chunk) = crate::mirror::next_chunk(&mut resp).await? {
        data.extend_from_slice(&chunk);
    }
    if data.is_empty() {
        return Err(crate::error::Error::Other(format!(
            "installer 下载为空：{installer_url}"
        )));
    }
    let tmp = std::env::temp_dir().join(format!("qz-loader-installer-{}.jar", std::process::id()));
    std::fs::write(&tmp, &data)?;

    // ④ 版本目录快照（安装后 diff 得到 installer 写入的版本 id）
    let versions_before = list_dir_names(&crate::paths::versions_dir());

    // ⑤ 无头安装：java -jar installer --installClient <minecraft_dir>
    let java = crate::runtime::java_bin(major);
    let mc_dir = crate::paths::minecraft_dir();
    let tmp2 = tmp.clone();
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new(java)
            .arg("-jar")
            .arg(&tmp2)
            .arg("--installClient")
            .arg(&mc_dir)
            .output()
    })
    .await
    .map_err(|e| crate::error::Error::Other(format!("installer 任务失败：{e}")))??;

    let _ = std::fs::remove_file(&tmp);
    if !output.status.success() {
        let tail: String = String::from_utf8_lossy(&output.stderr)
            .lines()
            .rev()
            .take(12)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n");
        return Err(crate::error::Error::Other(format!(
            "installer 退出码 {}：{}",
            output.status.code().unwrap_or(-1),
            tail
        )));
    }

    // ⑥ diff 得到新版本 id（installer 命名规则随版本变化，以实盘结果为准）
    let versions_after = list_dir_names(&crate::paths::versions_dir());
    let added: Vec<String> = versions_after
        .iter()
        .filter(|v| !versions_before.contains(v))
        .cloned()
        .collect();
    let vid = match added.len() {
        1 => added[0].clone(),
        _ => {
            return Err(crate::error::Error::Other(
                "installer 未写入新版本目录（退出成功但无新增版本），可能是不兼容的 Forge/NeoForge 组合".to_string(),
            ))
        }
    };
    send_progress(&progress, download_progress("done", &vid, 1, 1, true, None)).await;
    Ok(vid)
}

/// OptiFine：BMCLAPI 直装 jar 落到本地库 + 生成 tweaker 版本 JSON。
/// 1.8-1.12.2 走 LaunchWrapper tweaker（minecraftArguments 追加 --tweakClass）；
/// 1.13+ OptiFine 官方改为 installer patch 模型，本路径不承诺，报可行动错误。
async fn install_optifine(
    client: &reqwest::Client,
    mc_version: &str,
    of_version: &str,
    progress: tokio::sync::mpsc::Sender<DownloadProgress>,
) -> Result<String> {
    // 解析 of_version 形如 "HD U I6"（3 段）或 "HD I6"（2 段）。
    // BMCLAPI 条目 type / patch 由 list_optifine_versions 提供。

    // 基版本必须就绪
    if !crate::paths::version_json(mc_version).exists() {
        install_standard(client, mc_version, progress.clone()).await?;
    }
    // 1.13+ OptiFine 官方安装器需要 GUI patch 流程（修改 client jar 本体），
    // 无法在无头环境完成 —— 提前给出可行动错误，不浪费下载。
    let major = {
        let base = crate::meta::load_version(mc_version)?;
        base.java_version
            .as_ref()
            .map(|j| j.major_version)
            .unwrap_or(8)
    };
    if major > 12 {
        return Err(crate::error::Error::Other(
            "OptiFine 1.13+ 官方安装器需要 GUI patch 流程，暂不支持；1.8-1.12.2 完整可用".into(),
        ));
    }

    // 版本号约定："HD U I6"（3 段）或 "HD I6"（2 段）；下载 URL 只需 type + patch。
    let parts: Vec<&str> = of_version.split_whitespace().collect();
    if parts.len() < 2 || parts.len() > 3 {
        return Err(crate::error::Error::Other(format!(
            "OptiFine 版本格式需为 \"<type> [kind] <patch>\"（如 \"HD U I6\"），收到：{of_version}"
        )));
    }
    let of_type = parts[0];
    let of_patch = parts[parts.len() - 1];

    let url = crate::modkit::loaders::optifine_download_url(mc_version, of_type, of_patch);
    send_progress(
        &progress,
        download_progress("loader-libs", "OptiFine.jar", 0, 0, false, None),
    )
    .await;
    let mut resp = crate::mirror::get_stream(client, &url).await?;
    let mut data: Vec<u8> = Vec::new();
    while let Some(chunk) = crate::mirror::next_chunk(&mut resp).await? {
        data.extend_from_slice(&chunk);
    }
    if data.is_empty() {
        return Err(crate::error::Error::Other(format!(
            "OptiFine 下载为空：{url}"
        )));
    }

    // jar 落位：本地 maven 布局 optifine:OptiFine:{ver}，供 classpath 引用
    let coord_ver = format!("{mc_version}_{of_type}_{of_patch}");
    let rel = format!("optifine/OptiFine/{coord_ver}/OptiFine-{coord_ver}.jar");
    let dest = crate::paths::libraries_dir().join(&rel);
    tokio::fs::create_dir_all(dest.parent().unwrap()).await?;
    tokio::fs::write(&dest, &data).await?;

    // 版本 JSON：继承基版本 + OptiFine 本地库 + tweaker（1.8-1.12.2 LaunchWrapper 模型）。
    // url 为空字符串 = 本地安装库（无下载任务，见 meta::library_jar_url 约定）。
    let vid = format!("{mc_version}-OptiFine_{of_type}_{of_patch}");
    let json = serde_json::json!({
        "id": vid,
        "inheritsFrom": mc_version,
        "releaseTime": iso_now(),
        "time": iso_now(),
        "type": "release",
        "mainClass": "net.minecraft.launchwrapper.Launch",
        "minecraftArguments": "--tweakClass optifine.OptiFineTweaker",
        "libraries": [
            { "name": format!("optifine:OptiFine:{coord_ver}"), "url": "" }
        ]
    });
    let vdir = crate::paths::version_dir(&vid);
    tokio::fs::create_dir_all(&vdir).await?;
    tokio::fs::write(
        crate::paths::version_json(&vid),
        serde_json::to_string_pretty(&json)?,
    )
    .await?;
    send_progress(&progress, download_progress("done", &vid, 1, 1, true, None)).await;
    Ok(vid)
}

fn list_dir_names(dir: &std::path::Path) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            if e.path().is_dir() {
                out.push(e.file_name().to_string_lossy().into_owned());
            }
        }
    }
    out.sort();
    out
}

// ---------------------------------------------------------------------------
// 资产完整性检查与修复（v9「资源自动化」）
//
// 实机反馈：游戏内满屏 NoSuchFileException（assets/objects/**）—— 用户从旧
// 版本升级后 assets 只剩索引、对象文件从未落全；且下载中断可重试，但没有
// 主动触发入口。此模块提供：
// 1. count_missing_assets：stat-only 扫描（不读文件内容、不校验哈希），
//    一次扫描 <1s，供启动前自动检查与设置页展示；
// 2. repair_assets：只下载缺失对象（幂等，可反复调用），进度经 mpsc 上报。
// ---------------------------------------------------------------------------

/// 资产完整性状态（UI 展示）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetStatus {
    pub index_id: String,
    pub total: u64,
    pub missing: u64,
    pub missing_bytes: u64,
}

/// 读版本的资产索引（未安装/无资产索引返回 None）。
/// 走 inheritsFrom 链解析：loader 版本（Fabric/Forge 等）继承基版本的资产索引。
pub fn asset_index_for_version(version_id: &str) -> Option<(String, String)> {
    let resolved = crate::meta::resolve_version_chain(version_id).ok()?;
    let idx = resolved.version.asset_index.as_ref()?;
    let id = idx.id.clone();
    let index_text = std::fs::read_to_string(crate::paths::asset_index_file(&id)).ok()?;
    Some((id, index_text))
}

/// 资产完整性扫描（stat-only；无索引文件 = 尚未安装资产，missing=0 视为「无」，
/// 由调用方用 indexPresent 区分）。损坏的索引 JSON 返回错误（提示重下版本）。
pub fn count_missing_assets(version_id: &str) -> Result<AssetStatus> {
    let Some((id, index_text)) = asset_index_for_version(version_id) else {
        return Ok(AssetStatus {
            index_id: String::new(),
            total: 0,
            missing: 0,
            missing_bytes: 0,
        });
    };
    let tasks = crate::meta::expand_asset_index(&index_text, &crate::paths::assets_dir())?;
    let mut missing: u64 = 0;
    let mut missing_bytes: u64 = 0;
    for t in &tasks {
        if !t.dest.exists() {
            missing += 1;
            missing_bytes += t.size.unwrap_or(0);
        }
    }
    Ok(AssetStatus {
        index_id: id,
        total: tasks.len() as u64,
        missing,
        missing_bytes,
    })
}

/// 修复资产：只下载缺失对象（已存在的文件不动；stat 过滤保证秒级重入）。
/// 返回本次实际下载的文件数。进度经 mpsc 上报（stage=assets，与 install_standard 同构）。
pub async fn repair_assets(
    client: &reqwest::Client,
    version_id: &str,
    progress: tokio::sync::mpsc::Sender<DownloadProgress>,
) -> Result<usize> {
    let Some((_id, index_text)) = asset_index_for_version(version_id) else {
        return Err(crate::error::Error::Other(format!(
            "版本 {version_id} 缺少资产索引，请先重新下载该版本"
        )));
    };
    let all = crate::meta::expand_asset_index(&index_text, &crate::paths::assets_dir())?;
    let tasks: Vec<crate::download::DownloadTask> =
        all.into_iter().filter(|t| !t.dest.exists()).collect();
    let missing = tasks.len();
    if missing == 0 {
        send_progress(&progress, download_progress("done", "", 1, 1, true, None)).await;
        return Ok(0);
    }
    let total: u64 = tasks.iter().filter_map(|t| t.size).sum();
    crate::space::ensure_free(
        &crate::paths::assets_dir(),
        total + 32 * 1024 * 1024,
        &format!("修复资产（{} 个文件）", missing),
    )
    .map_err(crate::error::Error::Other)?;

    let (tx, mut rx) = tokio::sync::mpsc::channel::<crate::download::ProgressEvent>(64);
    let plan = crate::download::DownloadPlan::new(tasks);
    let exec_client = client.clone();
    let exec = tokio::spawn(async move {
        crate::download::execute(&plan, &exec_client, DOWNLOAD_CONCURRENCY, tx).await
    });
    let mut done_bytes: u64 = 0;
    while let Some(ev) = rx.recv().await {
        match ev {
            crate::download::ProgressEvent::TaskDone { bytes, .. } => done_bytes += bytes,
            crate::download::ProgressEvent::TaskSkipped { .. } => done_bytes += 0,
            crate::download::ProgressEvent::TaskFailed { label, error } => {
                let _ = label;
                return Err(crate::error::Error::Other(format!(
                    "资产修复失败（已自动重试 3 次）：{error}"
                )));
            }
            crate::download::ProgressEvent::TaskStart { .. } => {}
        }
        send_progress(
            &progress,
            download_progress("assets", "", done_bytes, total, false, None),
        )
        .await;
    }
    let summary = exec
        .await
        .map_err(|e| crate::error::Error::Other(format!("下载执行器崩溃：{e}")))??;
    if summary.failed > 0 {
        return Err(crate::error::Error::Other(
            "部分资产修复失败（已自动重试 3 次），可再次执行".into(),
        ));
    }
    send_progress(&progress, download_progress("done", "", 1, 1, true, None)).await;
    Ok(missing)
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
