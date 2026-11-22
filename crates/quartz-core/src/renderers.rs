//! 渲染层 pack 管理：在线下载 / 解包 / 校验 / 选择。
//!
//! 设计原则（用户硬性要求）：APK 不内置任何第三方渲染器 —— gl4es / MobileGlues /
//! ANGLE / VGPU 等全部运行时在线下载（与 JRE pack、LWJGL pack 同机制）。
//!
//! 安装位置：[`paths::runtime_dir`]/renderers/<id>/（Android 内部私有目录，
//! 可 exec/dlopen；与 JVM pack 同目录策略，不受 FUSE noexec 限制）。
//!
//! 资源源（全部为上游官方发布物，2026-11 核验可下载）：
//! - gl4es       PojavLauncherTeam/PojavLauncher@v3_openjdk jniLibs（libgl4es_114.so）
//! - MobileGlues MobileGL-Dev/MobileGlues-release V2.0.0 官方 APK（提取 arm64 so）
//! - ANGLE       FCL-Team/FoldCraftLauncher@main jniLibs（libEGL_angle / libGLESv2_angle）
//! - VGPU        FCL-Team/FoldCraftLauncher@main jniLibs（libvgpu.so）
//! - Zink / NG-GL4ES（Krypton Wrapper）登记占位：mesa GL 侧尚无稳定的单包直链，
//!   待 M2 接入（FCL 通过其插件市场分发，无公开固定 URL）。

use crate::error::{Error, Result};
use crate::instance::DownloadProgress;
use crate::paths;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// 单个渲染层的来源形态。
#[derive(Debug, Clone)]
pub enum PackSource {
    /// 直接下载若干文件落到安装目录。
    Raw(&'static [(&'static str, &'static str)]),
    /// 下载 APK 后提取 lib/arm64-v8a/*.so。
    ApkExtract(&'static str),
}

/// 渲染器 pack 元数据。
#[derive(Debug, Clone)]
pub struct RendererSpec {
    pub id: &'static str,
    pub name: &'static str,
    /// 对游戏暴露的 EGL 实现库名（供桥接层 dlopen）。
    pub egl_lib: &'static str,
    /// 对游戏暴露的 GL 库名。
    pub gl_lib: &'static str,
    pub note: &'static str,
    pub source: Option<PackSource>,
    /// 粗略体积提示（UI 展示用）。
    pub size_hint: &'static str,
}

const MOBILEGLUES_APK: &str =
    "https://github.com/MobileGL-Dev/MobileGlues-release/releases/download/V2.0.0/MobileGlues_2.0.0.apk";

const URL_GL4ES: &str = "https://raw.githubusercontent.com/PojavLauncherTeam/PojavLauncher/v3_openjdk/app_pojavlauncher/src/main/jniLibs/arm64-v8a/libgl4es_114.so";
const URL_ANGLE_EGL: &str = "https://raw.githubusercontent.com/FCL-Team/FoldCraftLauncher/main/FCL/src/main/jniLibs/arm64-v8a/libEGL_angle.so";
const URL_ANGLE_GLES: &str = "https://raw.githubusercontent.com/FCL-Team/FoldCraftLauncher/main/FCL/src/main/jniLibs/arm64-v8a/libGLESv2_angle.so";
const URL_VGPU: &str = "https://raw.githubusercontent.com/FCL-Team/FoldCraftLauncher/main/FCL/src/main/jniLibs/arm64-v8a/libvgpu.so";

/// 渲染器目录（编排契约：唯一权威）。
pub fn catalog() -> &'static [RendererSpec] {
    &[
        RendererSpec {
            id: "gl4es",
            name: "Holy GL4ES",
            egl_lib: "libgl4es_114.so",
            gl_lib: "libgl4es_114.so",
            note: "GL2.1→GLES2，老版本/低配兜底（Pojav 系默认）",
            source: Some(PackSource::Raw(&[(URL_GL4ES, "libgl4es_114.so")])),
            size_hint: "约 1.5 MB",
        },
        RendererSpec {
            id: "mobileglues",
            name: "MobileGlues",
            egl_lib: "libmobileglues.so",
            gl_lib: "libmobileglues.so",
            note: "GLES3.2 直转，1.20.x 兼容好（官方 Release APK 提取）",
            source: Some(PackSource::ApkExtract(MOBILEGLUES_APK)),
            size_hint: "约 30 MB",
        },
        RendererSpec {
            id: "angle",
            name: "ANGLE",
            egl_lib: "libEGL_angle.so",
            gl_lib: "libGLESv2_angle.so",
            note: "GL→Vulkan（Google 官方转译层，1.17+ 基础档）",
            source: Some(PackSource::Raw(&[
                (URL_ANGLE_EGL, "libEGL_angle.so"),
                (URL_ANGLE_GLES, "libGLESv2_angle.so"),
            ])),
            size_hint: "约 8 MB",
        },
        RendererSpec {
            id: "vgpu",
            name: "VGPU",
            egl_lib: "libvgpu.so",
            gl_lib: "libvgpu.so",
            note: "轻量 GL2.1→GLES 兜底（FCL 同款）",
            source: Some(PackSource::Raw(&[(URL_VGPU, "libvgpu.so")])),
            size_hint: "约 0.6 MB",
        },
        RendererSpec {
            id: "zink",
            name: "Zink",
            egl_lib: "libEGL_mesa.so",
            gl_lib: "libglxshim.so",
            note: "GL4.6→Vulkan（Sodium/光影推荐档）；Mesa GL 侧单包直链待 M2 接入",
            source: None,
            size_hint: "约 60 MB",
        },
        RendererSpec {
            id: "nggl4es",
            name: "Krypton Wrapper",
            egl_lib: "libng_gl4es.so",
            gl_lib: "libng_gl4es.so",
            note: "NG-GL4ES 高性能转译；公开单包直链待 M2 接入",
            source: None,
            size_hint: "约 5 MB",
        },
    ]
}

/// 按 id 查找。
pub fn spec_for(id: &str) -> Option<&'static RendererSpec> {
    catalog().iter().find(|s| s.id == id)
}

/// 渲染层安装目录：runtime_dir()/renderers/<id>。
pub fn install_dir(id: &str) -> PathBuf {
    paths::runtime_dir().join("renderers").join(id)
}

/// 安装完整性：EGL 库与 GL 库均存在。
pub fn is_installed(id: &str) -> bool {
    let Some(spec) = spec_for(id) else {
        return false;
    };
    let dir = install_dir(id);
    dir.join(spec.egl_lib).exists() && dir.join(spec.gl_lib).exists()
}

/// 当前选中的渲染器 id（进程级；落盘于 game_root()/renderer.json）。
static ACTIVE: Mutex<Option<String>> = Mutex::new(None);

/// 持久化路径。
fn active_file() -> PathBuf {
    paths::game_root().join("renderer.json")
}

/// 读取当前选中（落盘优先；默认 gl4es）。
pub fn active() -> String {
    if let Some(a) = ACTIVE.lock().ok().and_then(|g| g.clone()) {
        return a;
    }
    if let Ok(text) = std::fs::read_to_string(active_file()) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(id) = v["id"].as_str() {
                if spec_for(id).is_some() {
                    *ACTIVE.lock().unwrap() = Some(id.to_string());
                    return id.to_string();
                }
            }
        }
    }
    "gl4es".to_string()
}

/// 选中渲染器并落盘（id 必须在目录中）。
pub fn set_active(id: &str) -> Result<()> {
    if spec_for(id).is_none() {
        return Err(Error::Other(format!("未知渲染器：{id}")));
    }
    *ACTIVE.lock().unwrap() = Some(id.to_string());
    let json = serde_json::json!({ "id": id });
    std::fs::write(active_file(), serde_json::to_string(&json)?)?;
    Ok(())
}

/// 启动期环境变量（与 Pojav/FCL 行业默认一致）。
pub fn launch_env(id: &str) -> Vec<(String, String)> {
    match id {
        "gl4es" => vec![
            ("LIBGL_ES".into(), "2".into()),
            ("LIBGL_MIPMAP".into(), "3".into()),
            ("LIBGL_NOINTOVLHACK".into(), "1".into()),
            ("LIBGL_NORMALIZE".into(), "1".into()),
        ],
        // MobileGlues 自带配置文件；这里只给固定语义开关
        "mobileglues" => vec![("MOBILEGLUES_DISABLE_DSA".into(), "1".into())],
        "vgpu" => vec![("VGPU_NOERROR".into(), "1".into())],
        "zink" => vec![
            ("MESA_LOADER_DRIVER_OVERRIDE".into(), "zink".into()),
            ("GALLIUM_DRIVER".into(), "llvmpipe".into()),
        ],
        _ => vec![],
    }
}

/// 下载 + 安装（幂等：重装先清空目录）。进度经 mpsc 上报（复用 DownloadProgress）。
pub async fn download_and_install(
    client: &reqwest::Client,
    id: &str,
    progress: tokio::sync::mpsc::Sender<DownloadProgress>,
) -> Result<PathBuf> {
    let Some(spec) = spec_for(id) else {
        return Err(Error::Other(format!("未知渲染器：{id}")));
    };
    let Some(source) = &spec.source else {
        return Err(Error::Other(format!(
            "「{}」的下载源待接入（见 renderers.rs 说明）",
            spec.name
        )));
    };

    let dir = install_dir(id);
    let staging = paths::runtime_dir()
        .join("renderers")
        .join(format!(".{id}.staging"));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)?;

    let report = |stage: &str, cur: u64, total: u64, file: String, done: bool| {
        let percent = if total > 0 {
            ((cur as f64 / total as f64) * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        };
        let _ = progress.try_send(DownloadProgress {
            stage: stage.into(),
            current: cur,
            total,
            percent,
            file,
            done,
            error: None,
        });
    };

    match source {
        PackSource::Raw(items) => {
            let mut total: u64 = 0;
            for (url, _) in items.iter() {
                total += head_len_async(client, url).await;
            }
            let mut done_bytes: u64 = 0;
            for (i, (url, name)) in items.iter().enumerate() {
                let resp = crate::mirror::get_stream(client, url).await?;
                let mut file_total = resp.content_length().unwrap_or(0);
                if file_total == 0 {
                    file_total = total.saturating_sub(done_bytes);
                }
                let mut stream = resp;
                let mut buf =
                    tokio::io::BufWriter::new(tokio::fs::File::create(staging.join(name)).await?);
                use tokio::io::AsyncWriteExt;
                while let Some(chunk) = crate::mirror::next_chunk(&mut stream).await? {
                    buf.write_all(&chunk).await?;
                    done_bytes += chunk.len() as u64;
                    report(
                        "renderer-download",
                        done_bytes,
                        total.max(done_bytes),
                        format!("[{}/{}] {}", i + 1, items.len(), name),
                        false,
                    );
                }
                buf.flush().await?;
            }
        }
        PackSource::ApkExtract(url) => {
            let resp = crate::mirror::get_stream(client, url).await?;
            let total = resp.content_length().unwrap_or(0);
            let part = staging.parent().unwrap().join(format!(".{id}.apk.part"));
            let mut cur: u64 = 0;
            let mut stream = resp;
            {
                use tokio::io::AsyncWriteExt;
                let mut file =
                    tokio::io::BufWriter::new(tokio::fs::File::create(&part).await?);
                while let Some(chunk) = crate::mirror::next_chunk(&mut stream).await? {
                    file.write_all(&chunk).await?;
                    cur += chunk.len() as u64;
                    report(
                        "renderer-download",
                        cur,
                        total,
                        "MobileGlues APK".into(),
                        false,
                    );
                }
                file.flush().await?;
            }
            // 提取 arm64 原生库（阻塞线程池）
            let part2 = part.clone();
            let staging2 = staging.clone();
            tokio::task::spawn_blocking(move || extract_apk_libs(&part2, &staging2))
                .await
                .map_err(|e| Error::Other(format!("提取线程崩溃：{e}")))??;
            let _ = std::fs::remove_file(&part);
        }
    }

    // 落位（原子改名）+ 校验
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    std::fs::rename(&staging, &dir)?;
    if !is_installed(id) {
        return Err(Error::Other(format!(
            "渲染层安装不完整（缺少 {}）",
            spec.egl_lib
        )));
    }
    report("done", 1, 1, format!("{} 安装完成", spec.name), true);
    Ok(dir)
}

/// HEAD 取 Content-Length（逐候选源尝试；拿不到就 0，进度退化为计数）。
async fn head_len_async(client: &reqwest::Client, url: &str) -> u64 {
    for cand in crate::mirror::candidates(url) {
        let Ok(r) = client.head(&cand).send().await else {
            continue;
        };
        if r.status().is_success() {
            if let Some(len) = r.content_length() {
                return len;
            }
        }
    }
    0
}

/// 从 APK 提取 lib/arm64-v8a/*.so。
fn extract_apk_libs(apk: &Path, dest: &Path) -> Result<usize> {
    let file = std::fs::File::open(apk)?;
    let mut archive = zip::ZipArchive::new(std::io::BufReader::new(file))?;
    let mut count = 0usize;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let Some(name) = entry.enclosed_name() else {
            continue;
        };
        let name = name.to_string_lossy();
        if !name.starts_with("lib/arm64-v8a/") || !name.ends_with(".so") {
            continue;
        }
        let out = dest.join(name.rsplit('/').next().unwrap_or(&name));
        let mut out_file = std::fs::File::create(&out)?;
        std::io::copy(&mut entry, &mut out_file)?;
        count += 1;
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_named_renderers() {
        for id in ["gl4es", "mobileglues", "angle", "vgpu", "zink", "nggl4es"] {
            assert!(spec_for(id).is_some(), "缺 {id}");
        }
        // 命名要求：MobileGlues / Zink / Krypton Wrapper 在列
        assert_eq!(spec_for("mobileglues").unwrap().name, "MobileGlues");
        assert_eq!(spec_for("nggl4es").unwrap().name, "Krypton Wrapper");
    }

    #[test]
    fn default_active_and_toggle() {
        let _guard = crate::runtime::tests::TEST_STATE_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let root = std::env::temp_dir().join(format!("qz-rd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        paths::set_runtime_root(root.clone());
        // 重置进程级缓存
        *ACTIVE.lock().unwrap() = None;

        assert_eq!(active(), "gl4es");
        set_active("mobileglues").unwrap();
        assert_eq!(active(), "mobileglues");
        // 落盘可读回
        *ACTIVE.lock().unwrap() = None;
        assert_eq!(active(), "mobileglues");
        assert!(set_active("nope").is_err());

        *ACTIVE.lock().unwrap() = None;
        std::fs::remove_dir_all(&root).ok();
        paths::set_runtime_root(std::env::temp_dir().join("quartz-test-root"));
    }

    #[test]
    fn launch_env_per_renderer() {
        assert!(launch_env("gl4es").iter().any(|(k, _)| k == "LIBGL_ES"));
        assert!(launch_env("mobileglues").is_empty() == false);
        assert!(launch_env("angle").is_empty());
    }

    #[test]
    fn install_dir_follows_runtime() {
        let _guard = crate::runtime::tests::TEST_STATE_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let root = std::env::temp_dir().join("qz-rd2");
        paths::set_runtime_jres_root(root.clone());
        assert_eq!(install_dir("gl4es"), root.join("renderers/gl4es"));
        paths::set_runtime_jres_root(std::env::temp_dir().join("quartz-test-jres"));
    }
}
