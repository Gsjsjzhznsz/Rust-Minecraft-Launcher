//! ui-pack 清单模型（docs/UI-PACK.md 协议 v0）：解析、校验、灰度命中与任务生成。
//!
//! 这是方案 A「L2 UI 热更」链路在核心侧的落点：
//! - 壳层启动时用 [`validate`] 校验线上 manifest（schema 兼容 / 壳层版本 / 文件安全）；
//! - [`rollout_hit`] 按 `hash(install_id + salt) % 100 < percent` 判定灰度命中；
//! - [`download_tasks`] 把 files 展开为复用 `core::download` 的任务（SHA-256 校验 +
//!   断点跳过 + 并发窗口），壳层下载完成后对 staging 目录做原子 rename 切换；
//! - [`Manifest::previous`] 回滚指针供壳层实现秒级回滚（崩溃 N 次自动切回）。
//!
//! 职责边界：ed25519 签名验签属于壳层/发布系统职责（公钥内嵌于壳层二进制），
//! 核心只负责结构与格式的强校验；路径安全校验防止恶意 manifest 写穿文件系统。

use crate::download::{DownloadTask, HashSpec, Source};
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// 壳层当前支持的 manifest schema；高于此版本将被拒绝（向前兼容红线）。
pub const SUPPORTED_SCHEMA: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub schema: u32,
    /// stable / beta / nightly 等发布通道。
    pub channel: String,
    /// 包版本（如 "2026.11.21-r1"），非 semver，仅要求非空。
    pub version: String,
    #[serde(default)]
    pub created_at: Option<String>,
    /// UI 依赖的壳层 command 最低版本（semver 三段比较）。
    pub min_shell_version: String,
    #[serde(default)]
    pub rollout: Rollout,
    /// 回滚指针：上一稳定版 manifest 的位置。
    #[serde(default)]
    pub previous: Option<PreviousRef>,
    #[serde(default)]
    pub files: Vec<PackFile>,
    /// 形如 "ed25519:<base64>"；由壳层验签，核心仅要求存在即非空字符串。
    #[serde(default)]
    pub signature: Option<String>,
}

/// 灰度发布参数：percent 为命中百分比，salt 参与哈希避免不同批次同结果。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Rollout {
    #[serde(default)]
    pub percent: u32,
    #[serde(default)]
    pub salt: String,
}

/// 回滚指针：指向上一个稳定版本的 manifest 地址。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviousRef {
    pub version: String,
    pub manifest: String,
}

/// ui-pack 内的单个文件（相对 pack 根目录的路径 + SHA-256 + CDN 地址）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackFile {
    pub path: String,
    pub sha256: String,
    pub size: u64,
    pub url: String,
}

/// 解析 manifest JSON。
pub fn parse(text: &str) -> Result<Manifest> {
    Ok(serde_json::from_str(text)?)
}

/// 结构与安全校验。`shell_version` 为当前壳层版本（semver 三段）。
pub fn validate(m: &Manifest, shell_version: &str) -> Result<()> {
    if m.schema > SUPPORTED_SCHEMA {
        return Err(Error::Other(format!(
            "ui-pack schema {} 高于壳层支持的 {}，拒绝加载",
            m.schema, SUPPORTED_SCHEMA
        )));
    }
    if m.version.trim().is_empty() {
        return Err(Error::Other("ui-pack manifest 缺少 version".into()));
    }
    if !shell_meets_min(shell_version, &m.min_shell_version) {
        return Err(Error::Other(format!(
            "壳层 {} 低于 ui-pack 要求的最低版本 {}",
            shell_version, m.min_shell_version
        )));
    }
    if m.rollout.percent > 100 {
        return Err(Error::Other(format!(
            "rollout.percent 越界: {}",
            m.rollout.percent
        )));
    }
    if m.files.is_empty() {
        return Err(Error::Other("ui-pack manifest 至少需要一个文件".into()));
    }
    for f in &m.files {
        if !path_is_safe(&f.path) {
            return Err(Error::Other(format!("不安全的文件路径: {}", f.path)));
        }
        if !is_sha256_hex(&f.sha256) {
            return Err(Error::Other(format!(
                "sha256 格式非法: {} ({})",
                f.path, f.sha256
            )));
        }
        if f.url.trim().is_empty() {
            return Err(Error::Other(format!("缺少下载地址: {}", f.path)));
        }
    }
    Ok(())
}

/// 灰度判定：percent>=100 全量；==0 全关；否则 FNV-1a(install_id + salt) % 100 < percent。
pub fn rollout_hit(m: &Manifest, install_id: &str) -> bool {
    if m.rollout.percent >= 100 {
        return true;
    }
    if m.rollout.percent == 0 {
        return false;
    }
    let h = fnv1a64(format!("{}{}", install_id, m.rollout.salt).as_bytes());
    (h % 100) < u64::from(m.rollout.percent)
}

/// 展开 staging 下载任务（dest = staging_root/<path>，SHA-256 校验）。
/// 壳层在 staging 目录完整后 rename 切换，保证 UI 原子更新。
pub fn download_tasks(m: &Manifest, staging_root: &Path) -> Vec<DownloadTask> {
    m.files
        .iter()
        .map(|f| DownloadTask {
            source: Source::Url(f.url.clone()),
            dest: staging_root.join(&f.path),
            hash: Some(HashSpec::sha256(&f.sha256)),
            size: Some(f.size),
            label: format!("uipack:{}:{}", m.version, f.path),
        })
        .collect()
}

/// 回滚目标（manifest 未提供 previous 指针时为 None）。
pub fn rollback_target(m: &Manifest) -> Option<&PreviousRef> {
    m.previous.as_ref()
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    const PRIME: u64 = 0x0000_0100_0000_01B3;
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(PRIME);
    }
    h
}

fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// 相对路径白名单校验：禁绝对路径、反斜杠、空路径段与任何 ".."。
fn path_is_safe(p: &str) -> bool {
    if p.is_empty() || p.starts_with('/') || p.contains('\\') || p.contains("..") {
        return false;
    }
    p.split('/').all(|seg| !seg.is_empty() && seg != ".")
}

/// semver 三段比较（忽略 prerelease 后缀）：shell >= min。
fn shell_meets_min(shell: &str, min: &str) -> bool {
    let parse = |s: &str| -> Vec<u32> {
        s.split(['-', '+'])
            .next()
            .unwrap_or(s)
            .split('.')
            .take(3)
            .map(|p| p.parse().unwrap_or(0))
            .collect()
    };
    let pad = |v: Vec<u32>| -> [u32; 3] {
        [
            v.first().copied().unwrap_or(0),
            v.get(1).copied().unwrap_or(0),
            v.get(2).copied().unwrap_or(0),
        ]
    };
    let (a, b) = (pad(parse(shell)), pad(parse(min)));
    match a.iter().zip(b.iter()).find(|(x, y)| x != y) {
        Some((x, y)) => x > y,
        None => true,
    }
}

/// 内置演示 manifest（域名使用 RFC2606 保留域，仅离线示例，不可实际下载）。
pub const SAMPLE_MANIFEST: &str = r#"{
  "schema": 1,
  "channel": "stable",
  "version": "2026.11.21-r1",
  "createdAt": "2026-11-21T08:00:00Z",
  "minShellVersion": "0.1.0",
  "rollout": { "percent": 50, "salt": "r1" },
  "previous": { "version": "2026.11.14-r7", "manifest": "https://cdn.example.invalid/ui/2026.11.14-r7/manifest.json" },
  "files": [
    { "path": "pack/index.html", "sha256": "1111111111111111111111111111111111111111111111111111111111111111", "size": 4210, "url": "https://cdn.example.invalid/ui/2026.11.21-r1/pack/index.html" },
    { "path": "pack/assets/app.js", "sha256": "2222222222222222222222222222222222222222222222222222222222222222", "size": 81920, "url": "https://cdn.example.invalid/ui/2026.11.21-r1/pack/assets/app.js" },
    { "path": "pack/i18n/zh-CN.json", "sha256": "3333333333333333333333333333333333333333333333333333333333333333", "size": 20480, "url": "https://cdn.example.invalid/ui/2026.11.21-r1/pack/i18n/zh-CN.json" }
  ],
  "signature": "ed25519:ZGVtbw=="
}"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Manifest {
        parse(SAMPLE_MANIFEST).unwrap()
    }

    #[test]
    fn sample_validates_and_builds_tasks() {
        let m = sample();
        validate(&m, "0.1.0").unwrap();
        assert_eq!(m.files.len(), 3);
        assert_eq!(m.channel, "stable");

        let tasks = download_tasks(&m, Path::new("/staging"));
        assert_eq!(tasks.len(), 3);
        assert_eq!(tasks[0].dest, Path::new("/staging/pack/index.html"));
        let spec = tasks[0].hash.as_ref().unwrap();
        assert_eq!(spec.algo, crate::download::HashAlgo::Sha256);
        assert_eq!(spec.value.len(), 64);
        assert_eq!(tasks[0].size, Some(4210));
        assert!(tasks[0].label.starts_with("uipack:2026.11.21-r1:"));

        let prev = rollback_target(&m).unwrap();
        assert_eq!(prev.version, "2026.11.14-r7");
    }

    #[test]
    fn rejects_schema_version_paths_and_hashes() {
        // schema 高于壳层支持
        let mut m = sample();
        m.schema = 2;
        assert!(validate(&m, "0.1.0").is_err());

        // 路径穿越
        let mut m = sample();
        m.files[0].path = "../evil".into();
        assert!(validate(&m, "0.1.0").is_err());
        // 绝对路径
        let mut m = sample();
        m.files[0].path = "/abs".into();
        assert!(validate(&m, "0.1.0").is_err());
        // 空路径段
        let mut m = sample();
        m.files[0].path = "pack//x.js".into();
        assert!(validate(&m, "0.1.0").is_err());
        // 哈希格式非法
        let mut m = sample();
        m.files[0].sha256 = "zz".into();
        assert!(validate(&m, "0.1.0").is_err());

        // 最低壳层版本不满足
        let mut m2 = sample();
        m2.min_shell_version = "9.9.9".into();
        assert!(validate(&m2, "0.1.0").is_err());
        // 满足（带 prerelease 后缀的壳层版本）
        let mut m3 = sample();
        m3.min_shell_version = "0.1.0".into();
        assert!(validate(&m3, "0.2.0-beta.1").is_ok());

        // 空文件列表
        let mut m4 = sample();
        m4.files.clear();
        assert!(validate(&m4, "0.1.0").is_err());
    }

    #[test]
    fn rollout_bounds_and_determinism() {
        let mut m = sample();
        m.rollout.percent = 100;
        assert!(rollout_hit(&m, "anything"));
        m.rollout.percent = 0;
        assert!(!rollout_hit(&m, "anything"));

        m.rollout.percent = 50;
        assert_eq!(
            rollout_hit(&m, "install-0001"),
            rollout_hit(&m, "install-0001")
        );

        // 50% 灰度在 1000 个样本中的命中率应接近 1/2（FNV-1a 均匀性，确定性断言）
        m.rollout.percent = 50;
        let hits = (0..1000u32)
            .filter(|i| rollout_hit(&m, &i.to_string()))
            .count();
        assert!((400..600).contains(&hits), "50% 灰度命中 {hits}/1000");
    }

    #[test]
    fn semver_comparison() {
        assert!(shell_meets_min("0.1.0", "0.1.0"));
        assert!(shell_meets_min("0.2.0", "0.1.9"));
        assert!(!shell_meets_min("0.0.9", "0.1.0"));
        assert!(shell_meets_min("0.1.0-beta.1", "0.1.0"));
        assert!(!shell_meets_min("0.1", "0.1.1"));
    }
}
