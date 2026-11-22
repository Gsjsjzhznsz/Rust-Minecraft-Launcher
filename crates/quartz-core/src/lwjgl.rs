//! LWJGL3-for-Android pack 管理：在线下载官方移植版类与原生库。
//!
//! 来源：PojavLauncherTeam/PojavLauncher「gladiolus」官方 Release APK（单一一致构建）。
//! 从 APK 内提取以下文件（jar 与 so 保证互相配套，版本由同一 APK 决定）：
//! - `assets/components/lwjgl3/lwjgl-glfw-classes.jar`：
//!   官方 LWJGL jar 与安卓 GLFW 桥接类合并产物（org.lwjgl 全量类 +
//!   安卓 GLFW 实现）。其 `org.lwjgl.glfw.GLFW` 静态初始化会
//!   `System.loadLibrary("pojavexec")` 并以 `libpojavexec.so` 作为
//!   GLFW Functions 表载体（pojavInit/pojavCreateContext/... 均由它导出），
//!   启动时整体替换版本 JSON 里的 org.lwjgl:* 类路径（Mojang 分发的
//!   natives-linux 为 glibc 构建，bionic 无法加载）。
//! - 原生库（lib/arm64-v8a，bionic 构建，装入 java.library.path 与
//!   LD_LIBRARY_PATH）：
//!   - `libpojavexec.so`   GLFW 桥接 + 输入/剪贴板 native 实现（**必须**）
//!   - `liblwjgl.so`       LWJGL core
//!   - `liblwjgl_opengl.so` OpenGL bindings
//!   - `liblwjgl_stb.so`   STB bindings（MC 1.19+ 图像处理需要）
//!   - `liblwjgl_tinyfd.so` tinyfiledialogs bindings
//!   - `libopenal.so`      OpenAL 实现（openal 模块默认名 openal）
//!   - `libfreetype.so`    FreeType（freetype 模块默认名 freetype）
//!   - `libjnidispatch.so` JNA 的 bionic 构建（oshi-core → JNA；jar 内自带
//!     的 linux-aarch64 版为 glibc 构建，bionic 加载必败 —— System.loadLibrary
//!     优先级高于 jar 解包，本 so 在 java.library.path 即可拦截）
//!
//! Java 8 桥接类（cacio 等）不在本 pack 范围：1.20.4 直接子进程 JVM 不需要。

use crate::error::{Error, Result};
use crate::instance::DownloadProgress;
use crate::paths;
use std::path::PathBuf;

/// gladiolus 官方发布 APK（jar + 全部 bionic natives 单一来源）。
const POJAV_APK: &str =
    "https://github.com/PojavLauncherTeam/PojavLauncher/releases/download/gladiolus/PojavLauncher.apk";

/// APK 内需提取的文件清单：(APK 条目路径, 落盘相对路径)。
const APK_FILES: &[(&str, &str)] = &[
    (
        "assets/components/lwjgl3/lwjgl-glfw-classes.jar",
        "lwjgl-glfw-classes.jar",
    ),
    ("lib/arm64-v8a/libpojavexec.so", "natives/libpojavexec.so"),
    ("lib/arm64-v8a/liblwjgl.so", "natives/liblwjgl.so"),
    ("lib/arm64-v8a/liblwjgl_opengl.so", "natives/liblwjgl_opengl.so"),
    ("lib/arm64-v8a/liblwjgl_stb.so", "natives/liblwjgl_stb.so"),
    ("lib/arm64-v8a/liblwjgl_tinyfd.so", "natives/liblwjgl_tinyfd.so"),
    ("lib/arm64-v8a/libopenal.so", "natives/libopenal.so"),
    ("lib/arm64-v8a/libfreetype.so", "natives/libfreetype.so"),
    ("lib/arm64-v8a/libjnidispatch.so", "natives/libjnidispatch.so"),
];

/// 安装目录：runtime_dir()/lwjgl3/（classes.jar 与 natives/）。
pub fn install_dir() -> PathBuf {
    paths::runtime_dir().join("lwjgl3")
}

/// 合并类 jar 路径（启动时替换 org.lwjgl:* 类路径）。
pub fn classes_jar() -> PathBuf {
    install_dir().join("lwjgl-glfw-classes.jar")
}

/// 原生库目录（java.library.path / LD_LIBRARY_PATH 注入）。
pub fn natives_dir() -> PathBuf {
    install_dir().join("natives")
}

/// 是否已安装（类 jar + pojavexec/lwjgl/opengl 三核心 so 判定）。
pub fn is_installed() -> bool {
    classes_jar().exists()
        && natives_dir().join("libpojavexec.so").exists()
        && natives_dir().join("liblwjgl.so").exists()
        && natives_dir().join("liblwjgl_opengl.so").exists()
}

/// 下载 + 安装（幂等；重装先清空）。进度经 mpsc 上报。
pub async fn download_and_install(
    client: &reqwest::Client,
    progress: tokio::sync::mpsc::Sender<DownloadProgress>,
) -> Result<PathBuf> {
    let dir = install_dir();
    let staging = paths::runtime_dir().join(".lwjgl3.staging");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(staging.join("natives"))?;

    // ---- ① 下载官方 APK（一次 154MB，换取 jar 与 natives 版本严格配套；镜像回退） ----
    let part = paths::runtime_dir().join(".pojav.apk.part");
    let resp = crate::mirror::get_stream(client, POJAV_APK).await?;
    let total = resp.content_length().unwrap_or(0);
    let mut cur: u64 = 0;
    let mut stream = resp;
    {
        use tokio::io::AsyncWriteExt;
        let mut out = tokio::io::BufWriter::new(tokio::fs::File::create(&part).await?);
        let mut last = tokio::time::Instant::now() - tokio::time::Duration::from_secs(1);
        while let Some(chunk) = crate::mirror::next_chunk(&mut stream).await? {
            out.write_all(&chunk).await?;
            cur += chunk.len() as u64;
            let now = tokio::time::Instant::now();
            if now.duration_since(last) >= tokio::time::Duration::from_millis(120) {
                last = now;
                let percent = if total > 0 {
                    ((cur as f64 / total as f64) * 100.0).clamp(0.0, 100.0)
                } else {
                    0.0
                };
                let _ = progress.try_send(DownloadProgress {
                    stage: "lwjgl-download".into(),
                    current: cur,
                    total,
                    percent,
                    file: "PojavLauncher.apk（LWJGL 组件来源）".into(),
                    done: false,
                    error: None,
                });
            }
        }
        out.flush().await?;
    }
    if total > 0 && cur != total {
        let _ = std::fs::remove_file(&part);
        return Err(Error::Other(format!(
            "LWJGL 组件源下载不完整（{cur}/{total} 字节），请重试"
        )));
    }

    // ---- ② 提取 jar + natives（阻塞线程池） ----
    let part2 = part.clone();
    let staging2 = staging.clone();
    let files2 = APK_FILES.len();
    tokio::task::spawn_blocking(move || extract_from_apk(&part2, &staging2))
        .await
        .map_err(|e| Error::Other(format!("提取线程崩溃：{e}")))??;
    let _ = std::fs::remove_file(&part);

    // ---- ③ 落位（原子改名） ----
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    std::fs::rename(&staging, &dir)?;
    if !is_installed() {
        return Err(Error::Other(
            "LWJGL pack 安装不完整（缺 classes.jar 或核心 so）".into(),
        ));
    }
    let _ = progress
        .send(DownloadProgress {
            stage: "done".into(),
            current: 1,
            total: 1,
            percent: 100.0,
            file: format!("LWJGL3-Android（{files2} 个文件，gladiolus）"),
            done: true,
            error: None,
        })
        .await;
    Ok(dir)
}

/// 从 Pojav APK 提取指定条目到 dest（保持清单相对路径）。
fn extract_from_apk(apk: &std::path::Path, dest: &std::path::Path) -> Result<usize> {
    let file = std::fs::File::open(apk)?;
    let mut archive = zip::ZipArchive::new(std::io::BufReader::new(file))?;
    let mut count = 0usize;
    for (entry_path, rel) in APK_FILES {
        let Ok(mut entry) = archive.by_name(entry_path) else {
            return Err(Error::Other(format!(
                "Pojav APK 内缺少条目：{entry_path}（来源更新？）"
            )));
        };
        let out_path = dest.join(rel);
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out_file = std::fs::File::create(&out_path)?;
        std::io::copy(&mut entry, &mut out_file)?;
        count += 1;
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_layout_follows_runtime() {
        let _guard = crate::runtime::tests::TEST_STATE_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let root = std::env::temp_dir().join(format!("qz-lw-{}", std::process::id()));
        paths::set_runtime_jres_root(root.clone());
        assert_eq!(classes_jar(), root.join("lwjgl3/lwjgl-glfw-classes.jar"));
        assert_eq!(
            natives_dir(),
            root.join("lwjgl3/natives")
        );
        assert!(!is_installed());
        paths::set_runtime_jres_root(std::env::temp_dir().join("quartz-test-jres"));
    }

    #[test]
    fn manifest_requires_pojavexec() {
        // 关键回归：pojavexec 必须在提取清单里（v5 缺它导致游戏启动即 abort）
        assert!(APK_FILES.iter().any(|(p, _)| p.ends_with("libpojavexec.so")));
        assert!(APK_FILES.iter().any(|(p, _)| p.ends_with("libjnidispatch.so")));
        assert!(APK_FILES.iter().any(|(p, _)| p.ends_with("liblwjgl_stb.so")));
        assert!(APK_FILES.iter().any(|(p, _)| p.ends_with("lwjgl-glfw-classes.jar")));
    }
}
