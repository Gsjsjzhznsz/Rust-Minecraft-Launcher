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
    /// —— 支持改名（v0.2.22 起 zink 不再用：落盘名 = ZL2 APK 原名）。
    ApkExtract(&'static str, &'static [(&'static str, &'static str)]),
    /// v0.2.17：下载 zip 后按 basename 提取（.so/.json）—— Turnip 驱动包
    /// （本仓 CI 自建 mesa ICD 的发布资产，zip 根级平铺，非 APK 布局）。
    /// v0.2.20：zink 包同模式 —— CI 从 ZL2 官方 Release APK 字节级提取。
    /// 列表为空 = 提取全部 .so/.json；非空 = 白名单 (zip 内 basename, 落盘名)，
    /// v0.2.21 起为硬契约：包内缺任一条目 = 硬失败（制品↔清单错位当场暴露）。
    ZipExtract(&'static str, &'static [(&'static str, &'static str)]),
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
    /// 包版本号（v0.2.20）：> 0 时安装目录落 .pack-rev 标记，
    /// is_installed 校验不匹配即判未安装（包源/制品变更后自动重装）。
    /// 0 = 不校验（兼容旧装包，不触发无谓重下）。
    pub rev: u32,
}

const MOBILEGLUES_APK: &str =
    "https://github.com/MobileGL-Dev/MobileGlues-release/releases/download/V2.0.0/MobileGlues_2.0.0.apk";

/// Zink（Mesa GL4.6→Vulkan + Kopper）：**ZalithLauncher2 官方内置制品**（v0.2.20
/// 起，用户指令「跟 ZL2 一模一样」）。CI（scripts/build_zinkpack.sh）从
/// ZL2 官方 Release APK（2.8.2，sha256 锁定）字节级提取 Mesa 全套：
/// libEGL_mesa / libglapi / libglxshim / libgallium(zink) / libcutils 兼容层 /
/// libc++_shared（ZL2 官方 NDK r25c 构建）。
/// 2026-11-27 比对存证：IronizedZink v1.1.0（旧源）的 Mesa 五件套与 ZL2 内置
/// 逐字节相同（sha256 一致），仅 libc++_shared.so 不同 —— 本版统一为 ZL2 原件。
/// 设备侧要求：Vulkan 1.1+ 驱动可用（zink 经系统 libvulkan.so 拿设备）。
const ZINK_PACK_ZIP: &str = "https://github.com/Gsjsjzhznsz/Rust-Minecraft-Launcher/releases/latest/download/renderer-zink-aarch64.zip";
/// zink 包制品代数：v0.2.20 = ZL2 2.8.2 提取（rev 2）；v0.2.22 = 落盘名对齐
/// ZL2（libzink_dri.so 不再改名，rev 3 触发老用户重装）。将来随 ZL2 升版递增。
pub const ZINK_PACK_REV: u32 = 3;

/// Zink pack 提取白名单（zip 内 basename → 落盘名）。
/// v0.2.21：zip 条目与 ZL2 2.8.2 APK lib/arm64-v8a **逐文件同名同字节**。
/// v0.2.22（用户指令「跟 ZL2 完全一样」）：落盘名同步对齐 ZL2 —— ZL2 APK
/// 里磁盘名就是 libzink_dri.so，解包不再改名。发现链实锤（本会话反汇编
/// amethyst kopper_zink 构建的 libEGL_mesa.so）：Mesa 23.0.4 补丁版
/// loader_open_driver_lib 按 search_path_vars = {LIBGL_DRIVERS_PATH,
/// LD_LIBRARY_PATH} 逐目录尝试 dir/tls/<name>_dri.so → dir/<name>_dri.so
/// → dir/lib<name>_dri.so（第三式为该构建新增），driver_name=zink
///（MESA_LOADER_DRIVER_OVERRIDE）→ 命中 libzink_dri.so；该库的
/// DT_SONAME=libgallium_dri.so 只是链接器内部标识，与磁盘名无关。
///（历史：v0.2.19 前白名单配着 IronizedZink APK 写、v0.2.20/21 落盘改名
/// libgallium_dri.so —— 均非 ZL2 本尊布局，v0.2.22 纠正。）
const ZINK_ZIP_LIBS: &[(&str, &str)] = &[
    ("libEGL_mesa.so", "libEGL_mesa.so"),
    ("libglapi.so", "libglapi.so"),
    ("libglxshim.so", "libglxshim.so"),
    ("libzink_dri.so", "libzink_dri.so"),
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
            rev: 0,
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
            rev: 0,
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
            rev: 0,
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
            rev: 0,
        },
        RendererSpec {
            id: "zink",
            name: "Zink",
            egl_lib: "libEGL_mesa.so",
            gl_lib: "libglxshim.so",
            note:
                "GL4.6→Vulkan 桌面 GL（ZL2 官方同款 Mesa，Sodium/光影推荐档；需设备 Vulkan 1.1+）",
            source: Some(PackSource::ZipExtract(ZINK_PACK_ZIP, ZINK_ZIP_LIBS)),
            // 预载顺序 = 依赖拓扑：libc++_shared（cutils 的 NEEDED）-> cutils ->
            // glapi -> glxshim（桌面 GL 符号表）-> zink DRI 驱动（ZL2 磁盘名
            // libzink_dri.so；ZL2 本尊等价动作 = Java 侧 dlopen libglxshim +
            // Mesa loader 按 LD_LIBRARY_PATH 搜 libzink_dri.so）。链路任一
            // 缺失都会让 libEGL_mesa dlopen 失败（v9 实机根因）。
            // v0.2.23：libcutils.so 无 DT_SONAME，DT_NEEDED 按名解析不走
            // 本目录预载（:game 的 clns 搜索路径 = nativeLibraryDir，进程
            // 创建时冻结）—— 由 APK jniLibs 携带同款文件命中（ZL2 同款，
            // 见 zink_libcutils_jni_libs_contract 测试）。
            preload: &[
                "libc++_shared.so",
                "libcutils.so",
                "libglapi.so",
                "libglxshim.so",
                "libzink_dri.so",
            ],
            size_hint: "约 15 MB",
            rev: ZINK_PACK_REV,
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
            rev: 0,
        },
    ]
}

/// v0.2.17：Turnip（Mesa freedreno Vulkan ICD，KGSL 内核后端）—— 骁龙 zink
/// 的免 root Vulkan 驱动包。不属于可选渲染层（无 EGL 实现，仅作 zink 的
/// Vulkan 设备供给），因此不进 [`catalog`]（UI 不列出、set_active 拒绝）。
/// 来源：本仓 CI（scripts/build_turnip.sh）从 mesa-26.3.1 自建 —— 社区
/// 预编译（FCL/ZL2）按 mesa 的 vulkan-android.sym 只导 HAL `HMI` 入口，
/// Termux 构建缺 Android WSI，均不能被 VULKAN_PTR 直连消费端使用；
/// 我们的补丁（mesa-turnip-plain-gpa.patch）同时导出
/// vk_icdGetInstanceProcAddr + vkGetInstanceProcAddr（IronizedZink 只
/// dlsym 后者）。
const TURNIP_ZIP: &str = "https://github.com/Gsjsjzhznsz/Rust-Minecraft-Launcher/releases/latest/download/renderer-turnip-aarch64.zip";

/// turnip 伪渲染层 pack id（不进 catalog，见上）。
pub const TURNIP_PACK_ID: &str = "turnip";

static TURNIP_SPEC: RendererSpec = RendererSpec {
    id: TURNIP_PACK_ID,
    name: "Turnip (Adreno)",
    // egl/gl 指向驱动库本体仅为复用 is_installed 完整性检查（egl==gl ==
    // 只检一个文件）；turnip 不走 EGL 加载路径。
    egl_lib: "libvulkan_freedreno.so",
    gl_lib: "libvulkan_freedreno.so",
    note: "Mesa freedreno Vulkan ICD（KGSL）—— zink 的骁龙免 root 驱动（内部包）",
    source: Some(PackSource::ZipExtract(TURNIP_ZIP, &[])),
    preload: &[],
    size_hint: "约 17 MB",
    rev: 0,
};

/// turnip 驱动库落盘路径。
pub fn turnip_driver_lib() -> PathBuf {
    install_dir(TURNIP_PACK_ID).join("libvulkan_freedreno.so")
}

/// 主机侧门控：免 root Adreno 的 KGSL 设备节点存在。
/// Mali 等其它 GPU 无此节点 —— 不下载不探测，行为与 v0.2.16 一致。
pub fn turnip_host_gate() -> bool {
    Path::new("/dev/kgsl-3d0").exists()
}

/// 按 id 查找。
pub fn spec_for(id: &str) -> Option<&'static RendererSpec> {
    if id == TURNIP_PACK_ID {
        return Some(&TURNIP_SPEC);
    }
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
    if dir.join(spec.egl_lib).exists()
        && dir.join(spec.gl_lib).exists()
        && spec.preload.iter().all(|p| dir.join(p).exists())
    {
        // v0.2.20：包代数标记 —— rev > 0 的包要求 .pack-rev 存在且匹配
        //（制品/来源变更后旧装包自动失效，ensure_renderer 启动前自动重装）
        if spec.rev > 0 {
            return std::fs::read_to_string(dir.join(".pack-rev"))
                .map(|t| t.trim() == spec.rev.to_string())
                .unwrap_or(false);
        }
        return true;
    }
    false
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
    // v0.2.17：turnip 是 zink 的内部 Vulkan 驱动包（无 EGL 实现），
    // 选它会走坏 launch 流程 —— 明确拒绝。
    if id == TURNIP_PACK_ID {
        return Err(Error::Other(
            "turnip 是 zink 的内部 Vulkan 驱动包，不能作为渲染层直接使用".into(),
        ));
    }
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
        // Zink/Kopper（v0.2.20 与 ZL2 GameLauncher.kt needsZinkDriverOverride
        // 分支逐项对齐 —— KopperZinkRenderer.mesaBased=true 同源）：
        // - GALLIUM_DRIVER + MESA_LOADER_DRIVER_OVERRIDE=zink 强制 gallium zink 驱动
        //   （ZL2 egl_bridge.c pojavInitOpenGL kopper 分支设 GALLIUM_DRIVER，
        //   Kotlin 侧设 MESA_LOADER_DRIVER_OVERRIDE —— 两边都设，语义一致）
        // - MESA_ANDROID_NO_KMS_SWRAST=1（ZL2 egl_bridge.c 同款）：Mesa android
        //   平台跳过 kms_swrast 探测
        // - MESA_GL_VERSION_OVERRIDE=4.6 / MESA_GLSL_VERSION_OVERRIDE=460：
        //   ZL2 GameLauncher.kt 同款（v0.2.13 曾删除，实锤 ZL2 现行代码就是设，
        //   一模一样原则恢复）
        // - driconf 三项（ZL2 同款环境变量形式）：放宽 GLSL 编译门禁
        // - LIB_MESA_NAME=libglxshim.so：ZL2 mesaBased 渲染器同款（mesa 桥定位）
        // - LIBGL_ES=3：ZL2 KopperZinkRenderer.getRendererEnv 同款
        "zink" => vec![
            ("GALLIUM_DRIVER".into(), "zink".into()),
            ("MESA_LOADER_DRIVER_OVERRIDE".into(), "zink".into()),
            ("MESA_ANDROID_NO_KMS_SWRAST".into(), "1".into()),
            ("MESA_VK_IGNORE_CONFORMANCE_WARNING".into(), "1".into()),
            ("MESA_GL_VERSION_OVERRIDE".into(), "4.6".into()),
            ("MESA_GLSL_VERSION_OVERRIDE".into(), "460".into()),
            ("force_glsl_extensions_warn".into(), "true".into()),
            ("allow_higher_compat_version".into(), "true".into()),
            (
                "allow_glsl_extension_directive_midshader".into(),
                "true".into(),
            ),
            ("LIB_MESA_NAME".into(), "libglxshim.so".into()),
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
        PackSource::ZipExtract(url, whitelist) => {
            // 与 ApkExtract 同款下载 + spawn_blocking 提取（zip 内任意路径
            // 按 basename 匹配，root 级平铺的驱动包无需目录约束）
            let resp = crate::mirror::get_stream(client, url).await?;
            let total = resp.content_length().unwrap_or(0);
            let part = staging.parent().unwrap().join(format!(".{id}.zip.part"));
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
                    report("renderer-download", cur, total, format!("{id} zip"), false);
                }
                file.flush().await?;
            }
            let part2 = part.clone();
            let staging2 = staging.clone();
            tokio::task::spawn_blocking(move || extract_zip_libs(&part2, &staging2, whitelist))
                .await
                .map_err(|e| Error::Other(format!("提取线程崩溃：{e}")))??;
            let _ = std::fs::remove_file(&part);
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
    // v0.2.20：包代数标记（rev > 0 的包才落，is_installed 按此判定重装）
    if spec.rev > 0 {
        std::fs::write(dir.join(".pack-rev"), spec.rev.to_string())?;
    }
    if !is_installed(id) {
        // v0.2.21：缺什么列什么（此前固定报 egl_lib —— v0.2.20 zink 接缝事故
        // 实为 preload 缺当时落盘名的 zink DRI 驱动（libgallium_dri.so，
        // v0.2.22 已改回 ZL2 原名 libzink_dri.so），用户看到的却是「缺少
        // libEGL_mesa.so」）
        let mut missing: Vec<&str> = Vec::new();
        if !dir.join(spec.egl_lib).exists() {
            missing.push(spec.egl_lib);
        }
        if !dir.join(spec.gl_lib).exists() {
            missing.push(spec.gl_lib);
        }
        for p in spec.preload {
            if !dir.join(p).exists() {
                missing.push(p);
            }
        }
        let detail = if missing.is_empty() {
            format!(".pack-rev 标记不符（期望 {}）", spec.rev)
        } else {
            missing.join("、")
        };
        return Err(Error::Other(format!("渲染层安装不完整（缺 {detail}）")));
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

/// v0.2.17：从 zip 提取 .so/.json（basename 匹配，任意目录层级）。
/// whitelist 为空 = 提取全部 .so 与 .json；非空 = 仅 (basename, 落盘名) 列表，
/// v0.2.21 起为硬契约：包内缺任一条目 = Err 点名（制品↔清单错位当场暴露）。
fn extract_zip_libs(zip: &Path, dest: &Path, whitelist: &[(&str, &str)]) -> Result<usize> {
    let file = std::fs::File::open(zip)?;
    let mut archive = zip::ZipArchive::new(std::io::BufReader::new(file))?;
    let mut count = 0usize;
    // v0.2.21：白名单命中簿记 —— 非空白名单存在未命中条目即硬失败。
    // v0.2.20 zink 接缝事故：CI 把 zip 条目改名 libgallium_dri.so，App 清单
    // 仍找 libzink_dri.so，这里静默 continue → gallium 驱动未落盘 → 装完才
    // 报「安装不完整」且误报缺失对象。缺什么当场点名。
    let mut matched: Vec<String> = Vec::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let Some(name) = entry.enclosed_name() else {
            continue;
        };
        let name = name.to_string_lossy();
        if !(name.ends_with(".so") || name.ends_with(".json")) {
            continue;
        }
        let src_name = name.rsplit('/').next().unwrap_or(&name);
        let out_name = if whitelist.is_empty() {
            src_name
        } else {
            match whitelist.iter().find(|(apk_name, _)| *apk_name == src_name) {
                Some((_, disk_name)) => {
                    matched.push(src_name.to_string());
                    *disk_name
                }
                None => continue,
            }
        };
        let out = dest.join(out_name);
        let mut out_file = std::fs::File::create(&out)?;
        std::io::copy(&mut entry, &mut out_file)?;
        count += 1;
    }
    // v0.2.21：非空白名单 = 制品↔清单契约，缺任一条目即硬失败
    //（v0.2.20 zink 接缝事故回归防线：CI 制品改名/裁剪而清单没跟上时，
    // 下载环节当场点名，不再拖到「安装不完整」且误报缺失对象）
    if !whitelist.is_empty() {
        let missing: Vec<&str> = whitelist
            .iter()
            .map(|(s, _)| *s)
            .filter(|s| !matched.iter().any(|m| m.as_str() == *s))
            .collect();
        if !missing.is_empty() {
            return Err(Error::Other(format!(
                "渲染层包与提取清单不符（包内缺：{}）—— 制品与白名单未对齐",
                missing.join("、")
            )));
        }
    }
    Ok(count)
}

/// 从 APK 提取 lib/arm64-v8a/*.so。whitelist 为空 = 全部提取；
/// 非空 = 仅提取 (APK 内文件名, 落盘文件名) 列出的条目（支持改名）。
fn extract_apk_libs(apk: &Path, dest: &Path, whitelist: &[(&str, &str)]) -> Result<usize> {
    let file = std::fs::File::open(apk)?;
    let mut archive = zip::ZipArchive::new(std::io::BufReader::new(file))?;
    let mut count = 0usize;
    // v0.2.21：与 extract_zip_libs 同款命中簿记（非空白名单缺条目硬失败）
    let mut matched: Vec<String> = Vec::new();
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
                Some((_, disk_name)) => {
                    matched.push(src_name.to_string());
                    *disk_name
                }
                None => continue,
            }
        };
        let out = dest.join(out_name);
        let mut out_file = std::fs::File::create(&out)?;
        std::io::copy(&mut entry, &mut out_file)?;
        count += 1;
    }
    // v0.2.21：与 extract_zip_libs 同款严格校验（契约错位当场点名）
    if !whitelist.is_empty() {
        let missing: Vec<&str> = whitelist
            .iter()
            .map(|(s, _)| *s)
            .filter(|s| !matched.iter().any(|m| m.as_str() == *s))
            .collect();
        if !missing.is_empty() {
            return Err(Error::Other(format!(
                "渲染层 APK 与提取清单不符（包内缺：{}）—— 制品与白名单未对齐",
                missing.join("、")
            )));
        }
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
        // zink 注入配方与 ZL2 GameLauncher.kt needsZinkDriverOverride 分支逐项
        // 对齐（v0.2.20 一模一样）
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
        // ZL2 现行代码就是设版本覆盖（v0.2.20 实锤恢复，一模一样）
        assert!(zink
            .iter()
            .any(|(k, v)| k == "MESA_GL_VERSION_OVERRIDE" && v == "4.6"));
        assert!(zink
            .iter()
            .any(|(k, v)| k == "MESA_GLSL_VERSION_OVERRIDE" && v == "460"));
        assert!(zink
            .iter()
            .any(|(k, v)| k == "LIB_MESA_NAME" && v == "libglxshim.so"));
        assert!(zink.iter().any(|(k, _)| k == "LIBGL_ES"));
    }

    #[test]
    fn zink_pack_has_source_and_preload() {
        let spec = spec_for("zink").unwrap();
        // v0.2.20：制品来源 = ZL2 官方提取包（ZipExtract，CI 从 ZL2 Release
        // APK 字节级提取，见 scripts/build_zinkpack.sh）；v0.2.22 rev=3。
        match &spec.source {
            Some(PackSource::ZipExtract(_, whitelist)) => {
                // v0.2.22 对齐 ZL2 本尊布局：libzink_dri.so 落盘名 = 原名
                //（磁盘文件名就是它，DT_SONAME libgallium_dri.so 只是链接器
                // 内部标识；Mesa 补丁版 loader 按 LD_LIBRARY_PATH 搜
                // libzink_dri.so）。
                assert!(whitelist
                    .iter()
                    .any(|(a, b)| *a == "libzink_dri.so" && *b == "libzink_dri.so"));
                assert!(whitelist.iter().any(|(a, _)| *a == "libEGL_mesa.so"));
                assert!(whitelist.iter().any(|(a, _)| *a == "libcutils.so"));
                // v0.2.21：CI 制品 ↔ App 契约逐条钉死 —— zip 条目名 = ZL2 APK
                // lib/arm64-v8a 原名（build_zinkpack.sh 不改名），落盘名见右列；
                // 左列再错位，这里先红（v0.2.20 接缝事故回归防线）。
                let expected: &[(&str, &str)] = &[
                    ("libEGL_mesa.so", "libEGL_mesa.so"),
                    ("libglapi.so", "libglapi.so"),
                    ("libglxshim.so", "libglxshim.so"),
                    ("libzink_dri.so", "libzink_dri.so"),
                    ("libcutils.so", "libcutils.so"),
                    ("libc++_shared.so", "libc++_shared.so"),
                ];
                assert_eq!(whitelist.len(), expected.len());
                for (w, e) in whitelist.iter().zip(expected) {
                    assert_eq!((w.0, w.1), *e);
                }
            }
            _ => panic!("zink 应使用 ZipExtract 白名单提取（ZL2 制品包）"),
        }
        assert_eq!(spec.rev, ZINK_PACK_REV);
        assert!(spec.rev > 0);
        // 预载清单（v9 依赖拓扑）：libc++_shared -> cutils -> glapi ->
        // glxshim -> zink DRI 驱动（ZL2 磁盘名 libzink_dri.so）
        assert_eq!(
            spec.preload,
            [
                "libc++_shared.so",
                "libcutils.so",
                "libglapi.so",
                "libglxshim.so",
                "libzink_dri.so"
            ]
        );
        // 白名单必须包含 libc++_shared.so —— pack 的 libcutils.so 依赖它
        //（v9 实机根因：缺它则 libEGL_mesa 全链 dlopen 失败）
        match &spec.source {
            Some(PackSource::ZipExtract(_, whitelist)) => {
                assert!(whitelist.iter().any(|(a, _)| *a == "libc++_shared.so"));
            }
            _ => unreachable!(),
        }
        assert!(preload_libs("gl4es").is_empty());
        assert_eq!(preload_libs("zink").len(), 5);
    }

    /// v0.2.23：libcutils jniLibs 契约 —— Mesa 栈唯一无 DT_SONAME 的按名
    /// 依赖（libEGL_mesa/libzink_dri 的 DT_NEEDED "libcutils.so"）必须由
    /// APK jniLibs 携带（clns 搜索路径 = nativeLibraryDir，按文件名解析；
    /// :game 是 Zygote 派生进程，数据目录预载对按名解析无效 —— ZL2 的
    /// APK lib/arm64-v8a 原样携带此文件，我们在 finalize_apk.sh 注入同款）。
    /// 本测试钉死仓库内契约件的 sha256 = ZL2 2.8.2 APK 提取件（与
    /// renderer-zink zip 内 libcutils.so 逐字节相同）；文件被替换/损坏时
    /// 本测试先红，CI sha256 -c 再拦一道。
    #[test]
    fn zink_libcutils_jni_libs_contract() {
        const LIBCUTILS_SHA: &str =
            "04258bb1ccef8f5097ddc32ae1fb2d57142ffc5999e96fa36bfab85e02e9e347";
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../apps/shell-tauri/src-tauri/bridge/libcutils.so");
        let data = std::fs::read(&path)
            .unwrap_or_else(|e| panic!("ZL2 jniLibs 契约件缺失 {}：{e}", path.display()));
        assert_eq!(data.len(), 21320, "libcutils.so 大小漂移（ZL2 2.8.2 原版 = 21320 B）");
        let digest = {
            use sha2::Digest as _;
            let mut h = sha2::Sha256::new();
            sha2::Digest::update(&mut h, &data);
            let mut out = String::new();
            use std::fmt::Write as _;
            for b in sha2::Digest::finalize(h) {
                write!(out, "{b:02x}").unwrap();
            }
            out
        };
        assert_eq!(digest, LIBCUTILS_SHA, "libcutils.so sha256 漂移（ZL2 契约破坏）");
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

    /// v0.2.17：turnip 伪渲染层 —— 不进 catalog（UI 不可见），但 spec_for
    /// 可解析（下载/安装/完整性检查复用）；set_active 明确拒绝。
    #[test]
    fn turnip_pack_wiring() {
        assert!(spec_for(TURNIP_PACK_ID).is_some());
        assert!(catalog().iter().all(|s| s.id != TURNIP_PACK_ID));
        assert!(set_active(TURNIP_PACK_ID).is_err());
        // is_installed 复用：驱动库存在即完整（egl==gl==驱动库）
        let _guard = crate::runtime::tests::TEST_STATE_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let root = std::env::temp_dir().join("qz-rd3");
        let _ = std::fs::remove_dir_all(&root);
        paths::set_runtime_jres_root(root.clone());
        let dir = install_dir(TURNIP_PACK_ID);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!is_installed(TURNIP_PACK_ID));
        std::fs::write(turnip_driver_lib(), b"fake").unwrap();
        assert!(is_installed(TURNIP_PACK_ID));
        assert_eq!(
            turnip_driver_lib(),
            root.join("renderers/turnip/libvulkan_freedreno.so")
        );
        paths::set_runtime_jres_root(std::env::temp_dir().join("quartz-test-jres"));
        std::fs::remove_dir_all(&root).ok();
    }

    /// v0.2.17：ZipExtract 按 basename 提取 .so/.json（root 平铺 + 子目录）。
    #[test]
    fn zip_extract_pulls_so_and_json() {
        let work = std::env::temp_dir().join(format!("qz-zipx-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&work);
        std::fs::create_dir_all(&work).unwrap();
        let zip_path = work.join("pack.zip");
        let file = std::fs::File::create(&zip_path).unwrap();
        let mut w = zip::ZipWriter::new(file);
        use std::io::Write as _;
        let opts: zip::write::SimpleFileOptions = Default::default();
        w.start_file("libvulkan_freedreno.so", opts).unwrap();
        std::io::Write::write_all(&mut w, b"so-bytes").unwrap();
        w.start_file("sub/dir/freedreno_icd.json", opts).unwrap();
        std::io::Write::write_all(&mut w, b"{}").unwrap();
        w.start_file("README", opts).unwrap();
        std::io::Write::write_all(&mut w, b"skip-me").unwrap();
        w.finish().unwrap().flush().unwrap();
        let dest = work.join("out");
        std::fs::create_dir_all(&dest).unwrap();
        let n = extract_zip_libs(&zip_path, &dest, &[]).unwrap();
        assert_eq!(n, 2);
        assert!(dest.join("libvulkan_freedreno.so").exists());
        assert!(dest.join("freedreno_icd.json").exists());
        assert!(!dest.join("README").exists());
        // 白名单模式：仅指定项，且支持改名
        let dest2 = work.join("out2");
        std::fs::create_dir_all(&dest2).unwrap();
        let n2 = extract_zip_libs(
            &zip_path,
            &dest2,
            &[("libvulkan_freedreno.so", "libvulkan_freedreno.so")],
        )
        .unwrap();
        assert_eq!(n2, 1);
        assert!(dest2.join("libvulkan_freedreno.so").exists());
        assert!(!dest2.join("freedreno_icd.json").exists());
        std::fs::remove_dir_all(&work).ok();
    }

    /// v0.2.21：白名单严格校验 —— 非空白名单缺条目必须硬失败并点名
    /// （v0.2.20 zink 接缝事故回归防线：CI 制品名与清单错位时，旧逻辑静默
    /// 跳过 → 安装不完整误报；现在下载环节当场报「包内缺 <名字>」）。
    #[test]
    fn zip_extract_strict_whitelist_fails_on_missing_entry() {
        let work = std::env::temp_dir().join(format!("qz-zipw-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&work);
        std::fs::create_dir_all(&work).unwrap();
        let zip_path = work.join("pack.zip");
        let file = std::fs::File::create(&zip_path).unwrap();
        let mut w = zip::ZipWriter::new(file);
        use std::io::Write as _;
        let opts: zip::write::SimpleFileOptions = Default::default();
        w.start_file("libEGL_mesa.so", opts).unwrap();
        std::io::Write::write_all(&mut w, b"x").unwrap();
        w.finish().unwrap().flush().unwrap();
        let dest = work.join("out");
        std::fs::create_dir_all(&dest).unwrap();
        // 清单要 libzink_dri.so 而包内没有 —— 正是 v0.2.20 事故形态
        let err = extract_zip_libs(
            &zip_path,
            &dest,
            &[("libzink_dri.so", "libzink_dri.so")],
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("libzink_dri.so"),
            "报错应点名缺失条目：{err}"
        );
        assert!(!dest.join("libzink_dri.so").exists());
        // 空白名单（turnip 全提取模式）不受严格校验影响
        assert!(extract_zip_libs(&zip_path, &dest, &[]).is_ok());
        std::fs::remove_dir_all(&work).ok();
    }
}
