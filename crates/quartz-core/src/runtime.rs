//! JVM 运行时 pack 管理（M0 启动链路）：下载 / 解包 / 校验 Pojav android-openjdk 产物。
//!
//! 平台硬性约束（为什么这样设计）：
//! 1. Android 10（API 29）起，`targetSdk >= 29` 的应用禁止对应用主目录中的文件
//!    调用 `exec()`（W^X 限制）；同时 `/storage/emulated/0`（FUSE）挂载为 noexec。
//!    因此壳层 APK 固定 `targetSdk 28`（Pojav/Termux 同款方案），且 JVM pack
//!    必须安装在 [`paths::runtime_dir`]（壳层注入的内部私有目录，可 exec）。
//! 2. 游戏文件（.minecraft）仍在公共标准目录（授权后 `/storage/emulated/0/Quartz/files`），
//!    与 JVM pack 分离 —— 数据可浏览/可迁移，运行时可执行。
//! 3. pack 来源：PojavLauncherTeam/android-openjdk-build-multiarch
//!    「JRE17 for Android」（OpenJDK GPL-2 with Classpath Exception），
//!    tarball 顶层无包装目录（`./bin/java` ...），bin/java 为 Android 原生 ELF
//!    （interpreter /system/bin/linker64，API 21+，NDK r21 构建）。
//!
//! Java 版本覆盖：JRE17 覆盖 Minecraft 1.13–1.20.4；1.20.5+ 需要 Java 21、
//! 1.12- 需要 Java 8 —— 下载源待 M1 接入（见 [`spec_for`] 与 ROADMAP）。

use crate::error::{Error, Result};
use crate::instance::DownloadProgress;
use crate::paths;
use std::path::{Path, PathBuf};

/// pack 下载源：「JRE17 for Android」（tag jre17-ec28559，官方 GitHub Release）。
const RELEASE_BASE: &str =
    "https://github.com/PojavLauncherTeam/android-openjdk-build-multiarch/releases/download/jre17-ec28559";

/// 单个运行时 pack 的下载清单条目（size 为预校验的精确字节数，进度条确定性）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeSpec {
    pub java_major: u32,
    pub arch: &'static str,
    pub file: &'static str,
    pub size: u64,
}

impl RuntimeSpec {
    pub fn url(&self) -> String {
        format!("{RELEASE_BASE}/{}", self.file)
    }
}

/// 按目标 Java 主版本 + 设备架构返回 pack 清单；无收录来源时返回 None。
pub fn spec_for(java_major: u32, arch: &str) -> Option<RuntimeSpec> {
    if java_major != 17 {
        // Java 21（MC 1.20.5+）/ Java 8（MC <= 1.12）来源待 M1 接入
        return None;
    }
    let (arch, file, size): (&'static str, &'static str, u64) = match arch {
        "arm64" => ("arm64", "jre17-arm64-20210825-release.tar.xz", 38_065_004),
        "arm" => ("arm", "jre17-arm-20210914-release.tar.xz", 37_378_360),
        "x86_64" => ("x86_64", "jre17-x86_64-20210825-release.tar.xz", 39_230_848),
        _ => return None,
    };
    Some(RuntimeSpec {
        java_major,
        arch,
        file,
        size,
    })
}

/// JVM pack 安装目录：runtime_dir()/jre<主版本>。
pub fn install_dir(java_major: u32) -> PathBuf {
    paths::runtime_dir().join(format!("jre{java_major}"))
}

/// JVM 可执行文件：install_dir()/bin/java。
pub fn java_bin(java_major: u32) -> PathBuf {
    install_dir(java_major).join("bin/java")
}

/// 是否已安装（bin/java + libjvm.so 双重判定，防半截解包）。
pub fn is_installed(java_major: u32) -> bool {
    java_bin(java_major).exists()
        && install_dir(java_major)
            .join("lib/server/libjvm.so")
            .exists()
}

/// 从 release 元文件读 JAVA_VERSION（不 spawn 进程，纯文件读取）。
pub fn installed_version(java_major: u32) -> Option<String> {
    let text = std::fs::read_to_string(install_dir(java_major).join("release")).ok()?;
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("JAVA_VERSION=\"") {
            return Some(v.trim_end_matches('"').to_string());
        }
    }
    None
}

/// 安装完整性校验：bin/java / libjvm.so / release 三要素。
pub fn verify_install(java_major: u32) -> Result<String> {
    let dir = install_dir(java_major);
    if !java_bin(java_major).exists() {
        return Err(Error::Other(format!(
            "JRE pack 不完整（缺少 {}）",
            dir.join("bin/java").display()
        )));
    }
    if !dir.join("lib/server/libjvm.so").exists() {
        return Err(Error::Other(
            "JRE pack 不完整（缺少 lib/server/libjvm.so）".to_string(),
        ));
    }
    Ok(installed_version(java_major).unwrap_or_else(|| format!("{java_major}")))
}

/// 下载 + 解包 + 权限修正 + 校验，一步到位（幂等：重装会先清空目标目录）。
///
/// 进度经 mpsc 上报（复用 [`DownloadProgress`]）：
/// - stage `runtime-download`：字节级进度（current/total/percent 有效）；
/// - stage `runtime-extract`：条目计数进度（total=0，UI 显示计数不显示百分比）；
/// - stage `done`：完成。
pub async fn download_and_install(
    client: &reqwest::Client,
    java_major: u32,
    arch: &str,
    progress: tokio::sync::mpsc::Sender<DownloadProgress>,
) -> Result<PathBuf> {
    let spec = spec_for(java_major, arch).ok_or_else(|| {
        Error::Other(format!(
            "暂无 Java {java_major}（{arch}）的运行时 pack 下载源（Java 21/8 来源待 M1 接入）"
        ))
    })?;

    let jres = paths::runtime_dir();
    std::fs::create_dir_all(&jres)?;

    // ---- ① 下载（.qz-part 临时名，完成后改名） ----
    let part = jres.join(format!("{}.qz-part", spec.file));
    let resp = client.get(spec.url()).send().await?.error_for_status()?;
    let total = resp.content_length().unwrap_or(spec.size);
    let mut downloaded: u64 = 0;
    let mut file = std::fs::File::create(&part)?;
    let mut last = tokio::time::Instant::now() - tokio::time::Duration::from_secs(1);
    let mut stream = resp;
    while let Some(chunk) = stream
        .chunk()
        .await
        .map_err(|e| Error::Other(format!("运行时下载失败：{e}")))?
    {
        std::io::Write::write_all(&mut file, &chunk)?;
        downloaded += chunk.len() as u64;
        let now = tokio::time::Instant::now();
        if now.duration_since(last) >= tokio::time::Duration::from_millis(120) {
            last = now;
            let percent = if total > 0 {
                ((downloaded as f64 / total as f64) * 100.0).clamp(0.0, 100.0)
            } else {
                0.0
            };
            let _ = progress
                .try_send(DownloadProgress {
                    stage: "runtime-download".into(),
                    current: downloaded,
                    total,
                    percent,
                    file: spec.file.into(),
                    done: false,
                    error: None,
                });
        }
    }
    std::io::Write::flush(&mut file)?;
    drop(file);
    if total > 0 && downloaded != total {
        return Err(Error::Other(format!(
            "运行时包下载不完整（{downloaded}/{total} 字节），请重试"
        )));
    }

    // ---- ② 解包（CPU/IO 密集，放阻塞线程池） ----
    let staging = jres.join(format!(".jre{java_major}.staging"));
    let _ = std::fs::remove_dir_all(&staging);
    let progress2 = progress.clone();
    let part2 = part.clone();
    let staging2 = staging.clone();
    tokio::task::spawn_blocking(move || extract_tar_xz(&part2, &staging2, progress2))
        .await
        .map_err(|e| Error::Other(format!("解包线程崩溃：{e}")))??;

    // ---- ③ 落位（原子改名）+ 清理下载缓存 ----
    let final_dir = install_dir(java_major);
    if final_dir.exists() {
        std::fs::remove_dir_all(&final_dir)?;
    }
    std::fs::rename(&staging, &final_dir)?;
    let _ = std::fs::remove_file(&part);

    // ---- ④ 校验 ----
    let version = verify_install(java_major)?;
    let _ = progress
        .send(DownloadProgress {
            stage: "done".into(),
            current: total,
            total,
            percent: 100.0,
            file: format!("jre{java_major}（{version}）"),
            done: true,
            error: None,
        })
        .await;
    Ok(final_dir)
}

/// 解包 tar.xz 到目标目录（unpack_in 阻断路径穿越；解包后修正 bin/* 执行位）。
fn extract_tar_xz(
    archive: &Path,
    dest: &Path,
    progress: tokio::sync::mpsc::Sender<DownloadProgress>,
) -> Result<()> {
    let file = std::fs::File::open(archive)?;
    let xz = xz2::read::XzDecoder::new(std::io::BufReader::new(file));
    let mut tar = tar::Archive::new(xz);
    tar.set_preserve_permissions(true);
    std::fs::create_dir_all(dest)?;
    let mut entries: u64 = 0;
    let mut last = std::time::Instant::now() - std::time::Duration::from_secs(1);
    for entry in tar.entries()? {
        let mut entry = entry?;
        // unpack_in 拒绝绝对路径与 .. 穿越；非法条目直接跳过
        if entry.unpack_in(dest).is_err() {
            continue;
        }
        entries += 1;
        let now = std::time::Instant::now();
        if now.duration_since(last) >= std::time::Duration::from_millis(150) {
            last = now;
            let _ = progress.try_send(DownloadProgress {
                stage: "runtime-extract".into(),
                current: entries,
                total: 0,
                percent: 0.0,
                file: format!("已解包 {entries} 个文件"),
                done: false,
                error: None,
            });
        }
    }
    // 双保险：bin/ 下全部可执行（NDK 构建产物本身带执行位，个别 ROM 解包后丢失）
    ensure_exec_dir(&dest.join("bin"));
    Ok(())
}

/// 递归把目录下普通文件权限设为 rwxr-xr-x（仅 bin/ 一层）。
fn ensure_exec_dir(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(_meta) = entry.metadata() {
                    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_coverage_and_urls() {
        let arm64 = spec_for(17, "arm64").expect("arm64 JRE17 必须有源");
        assert_eq!(arm64.file, "jre17-arm64-20210825-release.tar.xz");
        assert_eq!(
            arm64.url(),
            "https://github.com/PojavLauncherTeam/android-openjdk-build-multiarch/releases/download/jre17-ec28559/jre17-arm64-20210825-release.tar.xz"
        );
        assert!(arm64.size > 30_000_000);

        // arm32 / x86_64 也有源
        assert!(spec_for(17, "arm").is_some());
        assert!(spec_for(17, "x86_64").is_some());
        // 未收录：Java 21 / 8 / 未知架构
        assert!(spec_for(21, "arm64").is_none());
        assert!(spec_for(8, "arm64").is_none());
        assert!(spec_for(17, "riscv").is_none());
    }

    #[test]
    fn install_dir_follows_runtime_override() {
        let root = std::env::temp_dir().join(format!("qz-rt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        paths::set_runtime_jres_root(root.clone());
        assert_eq!(install_dir(17), root.join("jre17"));
        assert_eq!(java_bin(17), root.join("jre17/bin/java"));
        assert!(!is_installed(17));

        // 伪造半截 pack：只有 bin/java，没有 libjvm -> 未安装
        std::fs::create_dir_all(java_bin(17).parent().unwrap()).unwrap();
        std::fs::write(java_bin(17), b"fake").unwrap();
        assert!(!is_installed(17));
        // 补齐 libjvm -> 已安装，release 文件可读版本
        std::fs::create_dir_all(install_dir(17).join("lib/server")).unwrap();
        std::fs::write(install_dir(17).join("lib/server/libjvm.so"), b"fake").unwrap();
        std::fs::write(install_dir(17).join("release"), "JAVA_VERSION=\"17.0.5\"\n").unwrap();
        assert!(is_installed(17));
        assert_eq!(installed_version(17).as_deref(), Some("17.0.5"));
        assert_eq!(verify_install(17).unwrap(), "17.0.5");

        // 缺 release -> 校验兜底返回主版本
        std::fs::remove_file(install_dir(17).join("release")).unwrap();
        assert_eq!(verify_install(17).unwrap(), "17");

        std::fs::remove_dir_all(&root).ok();
        paths::set_runtime_jres_root(std::env::temp_dir().join("quartz-test-jres"));
    }
}
