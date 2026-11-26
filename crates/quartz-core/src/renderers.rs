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
//! - Zink       GoyDevv/IronizedZink v1.1.0 官方插件 APK（Mesa GL4.6→Vulkan，
//!   与 FCL/Zalith2 系同源的 libEGL_mesa + libglxshim + libgallium(zink)）
//! - NG-GL4ES（Krypton Wrapper）登记占位：尚无稳定的单包直链，待后续接入。

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
    /// 列表为空 = 提取全部；非空 = 白名单 (APK 内文件名, 落盘文件名)
    /// —— 支持改名（zink_dri 的 SONAME 是 libgallium_dri.so，Mesa EGL
    /// 按该名 dlopen，文件名必须对齐）。
    ApkExtract(&'static str, &'static [(&'static str, &'static str)]),
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
    /// EGL 库加载前须先 dlopen 的同目录依赖（链接器命名空间按名解析
    /// DT_NEEDED 时不搜索应用数据目录，必须由壳层按绝对路径预载）。
    pub preload: &'static [&'static str],
    /// 粗略体积提示（UI 展示用）。
    pub size_hint: &'static str,
}

const MOBILEGLUES_APK: &str =
    "https://github.com/MobileGL-Dev/MobileGlues-release/releases/download/V2.0.0/MobileGlues_2.0.0.apk";

/// Zink（Mesa GL4.6→Vulkan + Kopper）：IronizedZink 官方发布插件 APK。
/// 单一来源包含 Mesa 全套 bionic 构建（libEGL_mesa / libglapi / libglxshim /
/// libgallium(zink) / libcutils 兼容层），面向 FCL/Zalith2 系（即 Pojav 约定）。
/// 设备侧要求：Vulkan 1.1+ 驱动可用（zink 经系统 libvulkan.so 拿设备）。
const IRONIZED_ZINK_APK: &str =
    "https://github.com/GoyDevv/IronizedZink/releases/download/v1.1.0/Ironized-Zink-v1.1.0.apk";

/// Zink pack 提取白名单（跳过插件 App 自身的支持库；libzink_dri 改名对齐 SONAME）。
///
/// 实机诊断（2026-11 v9）：pack 的 libcutils.so 是依赖 libc++_shared.so 的
/// bionic shim（DT_NEEDED 实测确认）—— 白名单缺 libc++_shared.so 时
/// dlopen(libcutils.so) 必败，连带 libEGL_mesa / libgallium_dri 全链失败，
/// EGL 未初始化 → LWJGL 函数指针 NULL → pc=0 SIGSEGV。
///
/// v0.2.0-e 实机诊断②：pack 的 libcutils.so **无 DT_SONAME** —— bionic
/// dlopen 按完整路径注册 soinfo，libgallium_dri/libEGL_mesa 的 DT_NEEDED
/// "libcutils.so" 按名解析在 clns 命名空间必败。对策：提取原版落位后，
/// game_bridge::ensure_zink_cutils_shim() 在预载前用带 SONAME 的 shim
/// 覆盖（bridge/libcutils_shim.c 交叉编译，符号集与原版对齐）。
const ZINK_APK_LIBS: &[(&str, &str)] = &[
    ("libEGL_mesa.so", "libEGL_mesa.so"),
    ("libglapi.so", "libglapi.so"),
    ("libglxshim.so", "libglxshim.so"),
    ("libzink_dri.so", "libgallium_dri.so"),
    ("libcutils.so", "libcutils.so"),
    ("libc++_shared.so", "libc++_shared.so"),
];

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
            preload: &[],
            size_hint: "约 1.5 MB",
        },
        RendererSpec {
            id: "mobileglues",
            name: "MobileGlues",
            egl_lib: "libmobileglues.so",
            gl_lib: "libmobileglues.so",
            note: "GLES3.2 直转，1.20.x 兼容好（官方 Release APK 提取）",
            source: Some(PackSource::ApkExtract(MOBILEGLUES_APK, &[])),
            preload: &[],
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
            preload: &[],
            size_hint: "约 8 MB",
        },
        RendererSpec {
            id: "vgpu",
            name: "VGPU",
            egl_lib: "libvgpu.so",
            gl_lib: "libvgpu.so",
            note: "轻量 GL2.1→GLES 兜底（FCL 同款）",
            source: Some(PackSource::Raw(&[(URL_VGPU, "libvgpu.so")])),
            preload: &[],
            size_hint: "约 0.6 MB",
        },
        RendererSpec {
            id: "zink",
            name: "Zink",
            egl_lib: "libEGL_mesa.so",
            gl_lib: "libglxshim.so",
            note: "GL4.6→Vulkan 桌面 GL（Mesa Zink+Kopper，Sodium/光影推荐档；需设备 Vulkan 1.1+）",
            source: Some(PackSource::ApkExtract(IRONIZED_ZINK_APK, ZINK_APK_LIBS)),
            // 预载顺序 = 依赖拓扑：libc++_shared（cutils 的 NEEDED）-> cutils ->
            // glapi -> glxshim（桌面 GL 符号表）-> gallium(zink) DRI 驱动。
            // 链路任一缺失都会让 libEGL_mesa dlopen 失败（v9 实机根因）。
            preload: &[
                "libc++_shared.so",
                "libcutils.so",
                "libglapi.so",
                "libglxshim.so",
                "libgallium_dri.so",
            ],
            size_hint: "约 14 MB",
        },
        RendererSpec {
            id: "nggl4es",
            name: "Krypton Wrapper",
            egl_lib: "libng_gl4es.so",
            gl_lib: "libng_gl4es.so",
            note: "NG-GL4ES 高性能转译；公开单包直链待后续接入",
            source: None,
            preload: &[],
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

/// 安装完整性：EGL 库、GL 库与全部预载依赖（preload 清单）均存在。
/// v0.2.4 修复：旧版只查 egl/gl 两库 —— zink 白名单后补的 libc++_shared.so
/// 在旧设备残缺包上探测不到，预载链断裂（EGL 初始化 0x3001）→ 启动即崩 255。
/// 预载清单任一缺失即判未安装，ensure_renderer 会在启动前自动重装。
pub fn is_installed(id: &str) -> bool {
    let Some(spec) = spec_for(id) else {
        return false;
    };
    let dir = install_dir(id);
    dir.join(spec.egl_lib).exists()
        && dir.join(spec.gl_lib).exists()
        && spec.preload.iter().all(|p| dir.join(p).exists())
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
        // MobileGlues 自带配置文件；这里给桥接层需要的固定语义开关：
        // LIBGL_ES=3 -> EGL 上下文 GLES3（桌面 GL 语义转译的底线，MC 1.17+ 需要）
        "mobileglues" => vec![
            ("MOBILEGLUES_DISABLE_DSA".into(), "1".into()),
            ("LIBGL_ES".into(), "3".into()),
        ],
        "vgpu" => vec![("VGPU_NOERROR".into(), "1".into())],
        // Zink/Kopper（v0.2.13 与 FCL/Zalith2 的 zink_kopper 分支逐项对齐）：
        // - GALLIUM_DRIVER + MESA_LOADER_DRIVER_OVERRIDE=zink 强制 gallium zink 驱动
        // - MESA_ANDROID_NO_KMS_SWRAST=1（FCL/ZL2 同款）：Mesa android 平台
        //   跳过 kms_swrast 探测（对当前 23.0.4 包是无效开关 —— 包内无该
        //   字符串；包升级到主线新版本后立即生效）
        // - 不再设 MESA_GL_VERSION_OVERRIDE/MESA_GLSL_VERSION_OVERRIDE：
        //   FCL/ZL2 实测配方不含版本覆盖，强制 4.6 会向 MC 虚报驱动能力
        // - driconf 三项（FCLauncher zink 分支同款）：放宽 GLSL 编译门禁
        // - LIBGL_ES=3 作为桥 GLES 兑底的语义开关（桌面分支失败时使用）
        "zink" => vec![
            ("GALLIUM_DRIVER".into(), "zink".into()),
            ("MESA_LOADER_DRIVER_OVERRIDE".into(), "zink".into()),
            ("MESA_ANDROID_NO_KMS_SWRAST".into(), "1".into()),
            ("MESA_VK_IGNORE_CONFORMANCE_WARNING".into(), "1".into()),
            ("force_glsl_extensions_warn".into(), "true".into()),
            ("allow_higher_compat_version".into(), "true".into()),
            (
                "allow_glsl_extension_directive_midshader".into(),
                "true".into(),
            ),
            ("LIBGL_ES".into(), "3".into()),
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

    // 存储空间前置检查（v8 加固）：下载 + 解压副本，统一按 200MB 下限把关
    crate::space::ensure_free(&dir, 200 * 1024 * 1024, "下载渲染层")
        .map_err(crate::error::Error::Other)?;

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
                let target = staging.join(name);
                let mut buf = tokio::io::BufWriter::new(
                    tokio::fs::File::create(&target)
                        .await
                        .map_err(|e| Error::Other(crate::error::io_context(&e, &target)))?,
                );
                use tokio::io::AsyncWriteExt;
                while let Some(chunk) = crate::mirror::next_chunk(&mut stream).await? {
                    buf.write_all(&chunk)
                        .await
                        .map_err(|e| Error::Other(crate::error::io_context(&e, &target)))?;
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
        PackSource::ApkExtract(url, whitelist) => {
            let resp = crate::mirror::get_stream(client, url).await?;
            let total = resp.content_length().unwrap_or(0);
            let part = staging.parent().unwrap().join(format!(".{id}.apk.part"));
            let mut cur: u64 = 0;
            let mut stream = resp;
            {
                use tokio::io::AsyncWriteExt;
                let mut file = tokio::io::BufWriter::new(
                    tokio::fs::File::create(&part)
                        .await
                        .map_err(|e| Error::Other(crate::error::io_context(&e, &part)))?,
                );
                while let Some(chunk) = crate::mirror::next_chunk(&mut stream).await? {
                    file.write_all(&chunk)
                        .await
                        .map_err(|e| Error::Other(crate::error::io_context(&e, &part)))?;
                    cur += chunk.len() as u64;
                    report("renderer-download", cur, total, format!("{id} APK"), false);
                }
                file.flush().await?;
            }
            // 提取 arm64 原生库（阻塞线程池；白名单/改名随包定义）
            let part2 = part.clone();
            let staging2 = staging.clone();
            tokio::task::spawn_blocking(move || extract_apk_libs(&part2, &staging2, whitelist))
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

/// EGL 库加载前须预载的同目录依赖（空表 = 无）。
pub fn preload_libs(id: &str) -> &'static [&'static str] {
    spec_for(id).map(|s| s.preload).unwrap_or(&[])
}

/// 从 APK 提取 lib/arm64-v8a/*.so。whitelist 为空 = 全部提取；
/// 非空 = 仅提取 (APK 内文件名, 落盘文件名) 列出的条目（支持改名）。
fn extract_apk_libs(apk: &Path, dest: &Path, whitelist: &[(&str, &str)]) -> Result<usize> {
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
        let src_name = name.rsplit('/').next().unwrap_or(&name);
        let out_name = if whitelist.is_empty() {
            src_name
        } else {
            match whitelist.iter().find(|(apk_name, _)| *apk_name == src_name) {
                Some((_, disk_name)) => *disk_name,
                None => continue,
            }
        };
        let out = dest.join(out_name);
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
        assert!(!launch_env("mobileglues").is_empty());
        assert!(launch_env("angle").is_empty());
        // zink 注入配方与 FCL/Zalith2 一致（v0.2.13）
        let zink = launch_env("zink");
        assert!(zink
            .iter()
            .any(|(k, v)| k == "MESA_LOADER_DRIVER_OVERRIDE" && v == "zink"));
        assert!(zink
            .iter()
            .any(|(k, v)| k == "GALLIUM_DRIVER" && v == "zink"));
        assert!(zink
            .iter()
            .any(|(k, v)| k == "MESA_ANDROID_NO_KMS_SWRAST" && v == "1"));
        // FCL/ZL2 实测配方不含版本覆盖（v0.2.13 移除，避免虚报驱动能力）
        assert!(!zink.iter().any(|(k, _)| k == "MESA_GL_VERSION_OVERRIDE"));
        assert!(!zink.iter().any(|(k, _)| k == "MESA_GLSL_VERSION_OVERRIDE"));
        assert!(zink.iter().any(|(k, _)| k == "LIBGL_ES"));
    }

    #[test]
    fn zink_pack_has_source_and_preload() {
        let spec = spec_for("zink").unwrap();
        match &spec.source {
            Some(PackSource::ApkExtract(_, whitelist)) => {
                // SONAME 对齐：libzink_dri.so 落盘为 libgallium_dri.so
                assert!(whitelist
                    .iter()
                    .any(|(a, b)| *a == "libzink_dri.so" && *b == "libgallium_dri.so"));
                assert!(whitelist.iter().any(|(a, _)| *a == "libEGL_mesa.so"));
                assert!(whitelist.iter().any(|(a, _)| *a == "libcutils.so"));
            }
            _ => panic!("zink 应使用 ApkExtract 白名单提取"),
        }
        // 预载清单（v9 依赖拓扑）：libc++_shared -> cutils -> glapi ->
        // glxshim -> gallium（链接器命名空间按名解析 NEEDED）
        assert_eq!(
            spec.preload,
            [
                "libc++_shared.so",
                "libcutils.so",
                "libglapi.so",
                "libglxshim.so",
                "libgallium_dri.so"
            ]
        );
        // 白名单必须包含 libc++_shared.so —— pack 的 libcutils.so 依赖它
        //（v9 实机根因：缺它则 libEGL_mesa 全链 dlopen 失败）
        match &spec.source {
            Some(PackSource::ApkExtract(_, whitelist)) => {
                assert!(whitelist.iter().any(|(a, _)| *a == "libc++_shared.so"));
            }
            _ => unreachable!(),
        }
        assert!(preload_libs("gl4es").is_empty());
        assert_eq!(preload_libs("zink").len(), 5);
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
