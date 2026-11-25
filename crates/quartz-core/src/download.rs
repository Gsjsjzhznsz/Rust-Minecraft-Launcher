//! 下载编排：任务模型、哈希校验（SHA-1/SHA-256/SHA-512）、断点跳过、并发执行与进度事件。
//!
//! 设计要点：
//! - `Source::Url` 走 HTTP（reqwest，rustls）；`Source::Local` 支持从本地缓存/已解压
//!   运行时包复制，方便打包与离线分发，也让执行器可以完全离线测试。
//! - Mojang 元数据用 SHA-1（piston-meta），Modrinth mrpack 可选 SHA-512，
//!   ui-pack 协议用 SHA-256 —— 统一由 [`HashSpec`] 表达。
//! - 每个任务写入前先校验「已存在且哈希正确」则跳过，天然实现断点续传。
//! - 哈希不匹配报 `Error::ShaMismatch`，交由上层决定重试或清理。

use crate::error::{Error, Result};
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha512};
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;

/// 单任务执行结果：(label, bytes, skipped) 或 (label, 错误)。
/// JoinSet 泛型参数较长，收敛为别名（clippy::type_complexity）。
type TaskExecOutcome = std::result::Result<(String, u64, bool), (String, Error)>;

/// 下载校验哈希算法。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashAlgo {
    Sha1,
    Sha256,
    Sha512,
}

/// 期望哈希（算法 + hex 编码值），任务校验的唯一表达。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HashSpec {
    pub algo: HashAlgo,
    pub value: String,
}

impl HashSpec {
    pub fn sha1(value: impl Into<String>) -> Self {
        Self {
            algo: HashAlgo::Sha1,
            value: value.into(),
        }
    }

    pub fn sha256(value: impl Into<String>) -> Self {
        Self {
            algo: HashAlgo::Sha256,
            value: value.into(),
        }
    }

    pub fn sha512(value: impl Into<String>) -> Self {
        Self {
            algo: HashAlgo::Sha512,
            value: value.into(),
        }
    }

    /// 校验一段数据是否与期望哈希一致（大小写不敏感）。
    pub fn matches(&self, data: &[u8]) -> bool {
        hash_hex(self.algo, data).eq_ignore_ascii_case(&self.value)
    }
}

fn hash_hex(algo: HashAlgo, data: &[u8]) -> String {
    match algo {
        // sha1 与 sha2 共享 RustCrypto 的 digest::Digest trait，统一用 sha2::Digest。
        HashAlgo::Sha1 => hex::encode(Sha1::digest(data)),
        HashAlgo::Sha256 => hex::encode(Sha256::digest(data)),
        HashAlgo::Sha512 => hex::encode(Sha512::digest(data)),
    }
}

#[derive(Debug, Clone)]
pub enum Source {
    Url(String),
    Local(PathBuf),
}

impl Source {
    pub fn source_url(&self) -> String {
        match self {
            Source::Url(u) => u.clone(),
            Source::Local(p) => format!("file://{}", p.display()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DownloadTask {
    pub source: Source,
    pub dest: PathBuf,
    /// 期望哈希；None 表示不校验（仅离线演示路径）。
    pub hash: Option<HashSpec>,
    pub size: Option<u64>,
    /// 人类可读的任务标签，如 "lib:org.lwjgl:lwjgl:3.3.1"。
    pub label: String,
}

#[derive(Debug, Clone, Default)]
pub struct DownloadPlan {
    pub tasks: Vec<DownloadTask>,
}

impl DownloadPlan {
    pub fn new(tasks: Vec<DownloadTask>) -> Self {
        Self { tasks }
    }
    pub fn total_bytes(&self) -> u64 {
        self.tasks.iter().filter_map(|t| t.size).sum()
    }
}

#[derive(Debug, Clone)]
pub enum ProgressEvent {
    TaskStart { label: String },
    TaskDone { label: String, bytes: u64 },
    TaskSkipped { label: String },
    TaskFailed { label: String, error: String },
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ExecSummary {
    pub done: u32,
    pub skipped: u32,
    pub failed: u32,
    pub bytes: u64,
}

fn spec_matches(hash: &Option<HashSpec>, data: &[u8]) -> bool {
    match hash {
        None => true,
        Some(spec) => spec.matches(data),
    }
}

/// 从单个 URL 拉全量字节（流式读 + 停滞超时；镜像选择交由 [`crate::mirror`]）。
async fn fetch_url_bytes(client: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    crate::mirror::get_bytes(client, url).await
}

async fn fetch_bytes(source: &Source, client: &reqwest::Client) -> Result<Vec<u8>> {
    match source {
        Source::Local(p) => tokio::fs::read(p).await.map_err(Into::into),
        Source::Url(url) => fetch_url_bytes(client, url).await,
    }
}

/// 执行单个任务：已存在且校验通过 -> 跳过；否则拉取 -> 校验 -> 原子落盘。
async fn run_task(task: DownloadTask, client: reqwest::Client) -> Result<(u64, bool)> {
    // 已存在且哈希匹配则跳过（断点续传）。
    if tokio::fs::metadata(&task.dest).await.is_ok() {
        let existing = tokio::fs::read(&task.dest).await?;
        if spec_matches(&task.hash, &existing) {
            return Ok((existing.len() as u64, true));
        }
    }

    // 镜像回退重试策略：候选源（镜像优先 + 直连兑底）逐个尝试为第一轮，
    // 全部失败后整体重试一轮（应对瞬时网络抖动）。任一候选成功即返回。
    let urls = match &task.source {
        Source::Local(_) => Vec::new(),
        Source::Url(u) => crate::mirror::candidates(u),
    };

    let mut last_err: Option<Error> = None;
    for round in 0..2u8 {
        if round > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(800)).await;
        }
        let sources: Vec<Source> = match &task.source {
            Source::Local(_) => vec![task.source.clone()],
            Source::Url(_) => urls.iter().map(|u| Source::Url(u.clone())).collect(),
        };
        for src in sources {
            match fetch_bytes(&src, &client).await {
                Ok(data) => {
                    if let Some(size) = task.size {
                        if data.len() as u64 != size {
                            let e =
                                Error::SizeMismatch(task.label.clone(), size, data.len() as u64);
                            last_err = Some(e);
                            continue;
                        }
                    }
                    if !spec_matches(&task.hash, &data) {
                        let (expected, got) = match &task.hash {
                            Some(h) => (h.value.clone(), hash_hex(h.algo, &data)),
                            None => (String::new(), String::new()),
                        };
                        let e = Error::ShaMismatch(task.label.clone(), expected, got);
                        last_err = Some(e);
                        continue;
                    }
                    if let Some(parent) = task.dest.parent() {
                        tokio::fs::create_dir_all(parent)
                            .await
                            .map_err(|e| Error::Other(crate::error::io_context(&e, parent)))?;
                    }
                    let tmp = task.dest.with_extension("qz-part");
                    tokio::fs::write(&tmp, &data)
                        .await
                        .map_err(|e| Error::Other(crate::error::io_context(&e, &tmp)))?;
                    tokio::fs::rename(&tmp, &task.dest)
                        .await
                        .map_err(|e| Error::Other(crate::error::io_context(&e, &task.dest)))?;
                    return Ok((data.len() as u64, false));
                }
                Err(e) => last_err = Some(e),
            }
        }
    }
    Err(last_err.unwrap_or_else(|| Error::Other(format!("task failed: {}", task.label))))
}

/// 并发执行下载计划，事件通过 mpsc 上报。返回汇总。
pub async fn execute(
    plan: &DownloadPlan,
    client: &reqwest::Client,
    concurrency: usize,
    events: mpsc::Sender<ProgressEvent>,
) -> Result<ExecSummary> {
    use tokio::task::JoinSet;

    let concurrency = concurrency.max(1);
    let mut set: JoinSet<TaskExecOutcome> = JoinSet::new();
    let mut inflight = 0usize;
    let mut iter = plan.tasks.iter().cloned();
    let mut summary = ExecSummary::default();

    // 简单窗口并发：最多同时 inflight 个任务。
    loop {
        while inflight < concurrency {
            let Some(task) = iter.next() else { break };
            let label = task.label.clone();
            let _ = events
                .send(ProgressEvent::TaskStart {
                    label: label.clone(),
                })
                .await;
            let client = client.clone();
            set.spawn(async move {
                match run_task(task, client).await {
                    Ok((bytes, skipped)) => Ok((label, bytes, skipped)),
                    Err(e) => Err((label, e)),
                }
            });
            inflight += 1;
        }
        let Some(next) = set.join_next().await else {
            break;
        };
        inflight -= 1;
        match next {
            Ok(Ok((label, bytes, skipped))) => {
                if skipped {
                    summary.skipped += 1;
                    let _ = events.send(ProgressEvent::TaskSkipped { label }).await;
                } else {
                    summary.done += 1;
                    summary.bytes += bytes;
                    let _ = events.send(ProgressEvent::TaskDone { label, bytes }).await;
                }
            }
            // 失败必须携带真实任务标签（v5 前丢标签导致 UI 只能显示 "task"）。
            Ok(Err((label, e))) => {
                summary.failed += 1;
                let _ = events
                    .send(ProgressEvent::TaskFailed {
                        label,
                        error: e.to_string(),
                    })
                    .await;
            }
            Err(join_err) => {
                summary.failed += 1;
                let _ = events
                    .send(ProgressEvent::TaskFailed {
                        label: "task(panic)".to_string(),
                        error: join_err.to_string(),
                    })
                    .await;
            }
        }
        // 任何失败即中止（可重试语义交给上层）。
        if summary.failed > 0 {
            break;
        }
    }
    Ok(summary)
}

/// 同步工具：校验一个本地文件哈希。
pub fn verify_file(path: &Path, expected: Option<&HashSpec>) -> Result<()> {
    let data = std::fs::read(path)?;
    let Some(spec) = expected else {
        return Ok(());
    };
    let actual = hash_hex(spec.algo, &data);
    if actual.eq_ignore_ascii_case(&spec.value) {
        Ok(())
    } else {
        Err(Error::ShaMismatch(
            path.display().to_string(),
            spec.value.clone(),
            actual,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn sha1_of(b: &[u8]) -> String {
        hash_hex(HashAlgo::Sha1, b)
    }

    #[test]
    fn hash_spec_nist_vectors_and_case_insensitive() {
        // NIST 标准测试向量：SHA-1("abc") 与 SHA-256("abc")
        let sha1_vec = "a9993e364706816aba3e25717850c26c9cd0d89d";
        let sha256_vec = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert!(HashSpec::sha1(sha1_vec).matches(b"abc"));
        assert!(HashSpec::sha256(sha256_vec).matches(b"abc"));
        // 大写期望值同样匹配（eq_ignore_ascii_case）
        assert!(HashSpec::sha256(sha256_vec.to_ascii_uppercase()).matches(b"abc"));
        assert!(!HashSpec::sha1(sha1_vec).matches(b"abcd"));
        // SHA-512 自一致性
        let sha512 = hash_hex(HashAlgo::Sha512, b"abc");
        assert!(HashSpec::sha512(sha512).matches(b"abc"));
    }

    #[tokio::test]
    async fn local_tasks_copy_skip_and_fail_on_bad_sha() {
        let dir = tempfile_dir("local-tasks");
        let src = dir.join("src.bin");
        let good = dir.join("good.bin");
        let good256 = dir.join("good256.bin");
        let bad = dir.join("bad.bin");
        std::fs::File::create(&src)
            .unwrap()
            .write_all(b"hello quartz")
            .unwrap();

        let plan = DownloadPlan::new(vec![
            DownloadTask {
                source: Source::Local(src.clone()),
                dest: good.clone(),
                hash: Some(HashSpec::sha1(sha1_of(b"hello quartz"))),
                size: None,
                label: "good".into(),
            },
            DownloadTask {
                source: Source::Local(src.clone()),
                dest: good256.clone(),
                hash: Some(HashSpec::sha256(hash_hex(
                    HashAlgo::Sha256,
                    b"hello quartz",
                ))),
                size: None,
                label: "good-sha256".into(),
            },
            DownloadTask {
                source: Source::Local(src.clone()),
                dest: bad.clone(),
                hash: Some(HashSpec::sha1("0000000000000000000000000000000000000000")),
                size: None,
                label: "bad".into(),
            },
        ]);

        let (tx, mut rx) = mpsc::channel(64);
        let client = reqwest::Client::new();
        let summary = execute(&plan, &client, 2, tx).await.unwrap();
        assert_eq!(summary.done, 2);
        assert_eq!(summary.failed, 1);
        assert_eq!(std::fs::read(&good).unwrap(), b"hello quartz");
        assert_eq!(std::fs::read(&good256).unwrap(), b"hello quartz");
        assert!(!bad.exists());

        // 事件流里应有一次 Done 一次 Failed。
        let mut done = false;
        let mut failed = false;
        while let Ok(ev) = rx.try_recv() {
            match ev {
                ProgressEvent::TaskDone { .. } => done = true,
                ProgressEvent::TaskFailed { .. } => failed = true,
                _ => {}
            }
        }
        assert!(done && failed);
    }

    #[tokio::test]
    async fn skips_existing_verified_file() {
        let dir = tempfile_dir("skip-existing");
        let dest = dir.join("cache.bin");
        std::fs::File::create(&dest)
            .unwrap()
            .write_all(b"stable")
            .unwrap();
        let plan = DownloadPlan::new(vec![DownloadTask {
            source: Source::Local(dir.join("whatever.bin")),
            dest: dest.clone(),
            hash: Some(HashSpec::sha1(sha1_of(b"stable"))),
            size: None,
            label: "cached".into(),
        }]);
        let (tx, _rx) = mpsc::channel(64);
        let summary = execute(&plan, &reqwest::Client::new(), 1, tx)
            .await
            .unwrap();
        assert_eq!(summary.skipped, 1);
        assert_eq!(summary.done, 0);
        assert_eq!(std::fs::read(&dest).unwrap(), b"stable");
    }

    fn tempfile_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("qz-test-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
