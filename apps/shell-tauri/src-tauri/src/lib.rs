//! Quartz Tauri 壳层。
//!
//! 架构契约（方案 A）：UI（WebView/JS）只调用本文件注册的 command；
//! command 是 quartz-core 能力的薄封装，不含业务逻辑。
//!
//! 命令契约（v3 蓝图 + M0 启动接入）：
//! - `offline_login(name)`        离线登录：MD5("OfflinePlayer:"+name) -> accounts.json
//! - `get_account()`              当前选中账号
//! - `list_accounts()`            全部账号 + 选中标记
//! - `select_account(uuid)`       切换选中账号
//! - `remove_account(uuid)`       删除账号
//! - `list_versions()`            真实 manifest 版本列表（release 优先）
//! - `list_installed_versions()`  本地 versions/ 已安装版本
//! - `download_game(version_id)`  后台 spawn 完整下载管线，
//!                                经 `download://progress` 事件上报 {stage,current,total,percent,file}
//! - `get_game_dir()`             标准游戏根目录绝对路径（UI 常显）
//! - `get_env_info()`             存储诊断：根目录 + 可写性 + 存储模式 + 完整文件权限
//! - `get_permission_state()`     存储权限状态 {granted}
//! - `request_storage_permission()` 触发系统权限弹窗（授权后经 focus/visibility 重检）
//! - `reinit_storage()`           权限变化后重解析存储根 + 后台迁移旧数据
//!                                （`quartz://migrate` 事件上报迁移进度）
//! - `launch_preflight(version_id)` 启动前自检（版本/账号/Java 运行时）
//! - `get_runtime_status(java_major)` JVM pack 状态（installed/version/path）
//! - `download_runtime(java_major)` 下载 JVM 运行时 pack（`runtime://progress` 事件）
//! - `launch_game(version_id)`    M0 已接入：JRE pack + LaunchPlan 子进程启动，
//!                                `launch://log` / `launch://exit` 事件实时上报
//!                                （v0.1.8：exit 事件附日志尾部直达 UI）
//! - `stop_game()`                终止游戏进程（SIGKILL）

use quartz_core::instance::DownloadProgress;
use quartz_core::renderbus::{self, DeviceCaps};
use quartz_core::{
    auth, content, host_arch, instance, jvmops, lwjgl, meta, model, modkit, paths, platform,
    renderers, rules, runtime, settings, RuntimeCtx,
};
use serde::Serialize;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, State};

#[cfg(target_os = "android")]
mod game_bridge;

/// 下载并发互斥：同一时刻只允许一个下载任务。
#[derive(Default)]
pub struct AppState {
    downloading: Arc<std::sync::Mutex<bool>>,
    /// 运行时 pack 下载互斥。
    runtime_busy: Arc<std::sync::atomic::AtomicBool>,
    /// 渲染层 pack 下载互斥。
    renderer_busy: Arc<std::sync::atomic::AtomicBool>,
    /// LWJGL pack 下载互斥。
    lwjgl_busy: Arc<std::sync::atomic::AtomicBool>,
    /// 运行中的游戏进程 pid（None = 空闲）。
    game_pid: Arc<Mutex<Option<u32>>>,
}

// ---------------------------------------------------------------------------
// Android JNI 基础（存储解析 / 权限检测共用）
// ---------------------------------------------------------------------------

#[cfg(target_os = "android")]
fn with_jni<T>(f: impl FnOnce(&mut jni::JNIEnv, &jni::objects::JObject) -> Option<T>) -> Option<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let ctx = ndk_context::android_context();
        let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }.ok()?;
        let mut env = vm.attach_current_thread().ok()?;
        let context = unsafe { jni::objects::JObject::from_raw(ctx.context().cast()) };
        // 防御：清掉上游遗留的挂起异常。JNIEnv 按线程复用，任何未清理的异常都会让
        // 本闭包内的下一个 JNI 调用（甚至 Tauri 内部调用）在 CheckJNI 下直接 abort。
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_clear();
        }
        let result = f(&mut env, &context);
        // 关键：闭包内任何 JNI Err（如 NoSuchMethodError）都会把异常留在挂起态，
        // 若不清掉，下一次 NewStringUTF 等调用即触发
        // "JNI NewStringUTF called with pending exception" abort 闪退。
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_clear();
        }
        result
    }))
    .ok()
    .flatten()
}

#[cfg(target_os = "android")]
fn jni_file_path(env: &mut jni::JNIEnv, file: &jni::objects::JObject) -> Option<PathBuf> {
    let path = env
        .call_method(file, "getAbsolutePath", "()Ljava/lang/String;", &[])
        .ok()?
        .l()
        .ok()?;
    let s: String = env.get_string((&path).into()).ok()?.into();
    if s.is_empty() {
        None
    } else {
        Some(PathBuf::from(s))
    }
}

#[cfg(target_os = "android")]
fn jni_external_files_dir(
    env: &mut jni::JNIEnv,
    context: &jni::objects::JObject,
) -> Option<PathBuf> {
    use jni::objects::JValue;
    let file = env
        .call_method(
            context,
            "getExternalFilesDir",
            "(Ljava/lang/String;)Ljava/io/File;",
            &[JValue::Object(&jni::objects::JObject::null())],
        )
        .ok()?
        .l()
        .ok()?;
    if file.is_null() {
        return None;
    }
    jni_file_path(env, &file)
}

#[cfg(target_os = "android")]
fn jni_internal_files_dir(
    env: &mut jni::JNIEnv,
    context: &jni::objects::JObject,
) -> Option<PathBuf> {
    let file = env
        .call_method(context, "getFilesDir", "()Ljava/io/File;", &[])
        .ok()?
        .l()
        .ok()?;
    if file.is_null() {
        return None;
    }
    jni_file_path(env, &file)
}

// ---------------------------------------------------------------------------
// 存储权限（完整文件权限）
// ---------------------------------------------------------------------------
//
// 设计：APK 固定 targetSdk 28（Android 10 前的存储模型 + 允许 exec() 应用目录
// 内的 JVM —— Pojav/Termux 同款方案）。
//
// 授权通道按系统版本分流（MainActivity.requestStorageAccess）:
// - Android 11+（API 30+）：MANAGE_EXTERNAL_STORAGE「所有文件访问」系统设置页。
//   不走 WRITE 弹窗的原因：targetSdk<=29 应用在 Android 11+ 经弹窗授予存储权限时，
//   系统会杀掉应用进程刷新 FUSE 挂载视图 —— 用户视角即「授权后闪退」。
//   所有文件访问页授权不杀进程，返回后 focus 重检即可切换标准目录。
// - Android 10 及以下（API <= 29）：READ/WRITE_EXTERNAL_STORAGE 运行时弹窗
//   （旧存储模型，授权即时生效不杀进程）。
// 游戏文件即可放在文件管理器可见的标准目录 /storage/emulated/0/Quartz/files。

/// Android 端存储权限是否已授权（纯系统 API，不依赖 Activity 实例）。
///
/// 注意：ndk-context 给到的 context 是 **Application** 对象（不是 MainActivity），
/// 其上不存在任何自定义方法 —— 在它上面 GetMethodID("isStorageAccessGranted")
/// 必抛 NoSuchMethodError。故全部改用静态方法 / Context 自有方法：
/// - Android 11+（API 30+）：静态 Environment.isExternalStorageManager()
///   （MANAGE_EXTERNAL_STORAGE「所有文件访问」）
/// - Android 10 及以下：Context.checkSelfPermission(WRITE_EXTERNAL_STORAGE)
///   （targetSdk 28 旧存储模型，Application 即 Context，可直接调）
#[cfg(target_os = "android")]
fn android_storage_permission_granted() -> bool {
    use jni::objects::JValue;
    with_jni(|env, ctx| {
        let sdk = env
            .get_static_field("android/os/Build$VERSION", "SDK_INT", "I")
            .ok()?
            .i()
            .ok()?;
        if sdk >= 30 {
            let granted = env
                .call_static_method(
                    "android/os/Environment",
                    "isExternalStorageManager",
                    "()Z",
                    &[],
                )
                .ok()?
                .z()
                .ok()?;
            Some(granted)
        } else {
            let perm = env
                .new_string("android.permission.WRITE_EXTERNAL_STORAGE")
                .ok()?;
            let granted = env
                .call_method(
                    ctx,
                    "checkSelfPermission",
                    "(Ljava/lang/String;)I",
                    &[JValue::Object(&perm.into())],
                )
                .ok()?
                .i()
                .ok()?;
            // PackageManager.PERMISSION_GRANTED == 0
            Some(granted == 0)
        }
    })
    .unwrap_or(false)
}

/// 存储权限是否已授权（桌面端恒真）。
fn storage_permission_granted() -> bool {
    #[cfg(target_os = "android")]
    {
        android_storage_permission_granted()
    }
    #[cfg(not(target_os = "android"))]
    {
        true
    }
}

/// 权限状态查询。
#[tauri::command]
fn get_permission_state() -> serde_json::Value {
    json!({ "granted": storage_permission_granted() })
}

/// 申请完整文件权限：Android 11+ 跳系统「所有文件访问」设置页（授权不杀进程），
/// Android 10- 触发运行时弹窗。结果不通过本命令返回：UI 在 focus/visibilitychange
/// 时重新查询状态（回到应用后自动重检）。
#[tauri::command]
fn request_storage_permission() -> Result<bool, String> {
    #[cfg(target_os = "android")]
    {
        // MainActivity 伴生对象静态方法（GetStaticMethodID，不依赖接收对象类型）：
        // 内部持有当前 Activity 实例，按系统版本分流（11+「所有文件访问」设置页 /
        // 10- 运行时弹窗）。此前直接在 ndk-context 的 Application 对象上调实例方法
        // requestStorageAccess —— 其上查无此方法，NoSuchMethodError 未清理即 abort。
        let via_kotlin = with_jni(|env, ctx| {
            use jni::objects::JClass;
            use jni::objects::JValue;
            // 注意：FindClass 在 AttachCurrentThread 的原生线程上走系统类加载器，
            // 解析不到 APK 内的应用类；必须经 context.getClassLoader().loadClass()
            // 显式解析壳层 MainActivity（Application 即 Context，链路稳定）。
            let loader = env
                .call_method(ctx, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])
                .ok()?
                .l()
                .ok()?;
            let name = env.new_string("com.quartz.launcher.MainActivity").ok()?;
            let cls_obj = env
                .call_method(
                    &loader,
                    "loadClass",
                    "(Ljava/lang/String;)Ljava/lang/Class;",
                    &[JValue::Object(&name.into())],
                )
                .ok()?
                .l()
                .ok()?;
            let cls = JClass::from(cls_obj);
            env.call_static_method(&cls, "requestStorageAccessStatic", "()V", &[])
                .ok()?;
            Some(())
        });
        if via_kotlin.is_some() {
            return Ok(true);
        }
        // 到这里说明 JNI 链路本身异常（with_jni 已清异常）；给用户明确的手动指引。
        Err(
            "无法唤起系统权限界面。请手动授予：系统设置 → 应用 → Quartz → 权限 → 所有文件访问权限"
                .to_string(),
        )
    }
    #[cfg(not(target_os = "android"))]
    {
        Ok(true)
    }
}

/// Android 方向切换：进入游戏 → 横屏，游戏退出 → 竖屏。
/// 与权限申请同链路（MainActivity 伴生对象静态方法转发到当前 Activity 实例；
/// ndk-context 的 context 是 Application，其上无实例方法）。失败静默：
/// 方向切换绝不阻塞/中止游戏启动。
/// v0.1.5：主链路改由 showGameSurfaceStatic/hideGameSurfaceStatic 承担，保留备用。
#[cfg_attr(target_os = "android", allow(dead_code))]
#[cfg(target_os = "android")]
fn android_set_orientation(landscape: bool) {
    let ok = with_jni(|env, ctx| {
        use jni::objects::{JClass, JValue};
        let loader = env
            .call_method(ctx, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])
            .ok()?
            .l()
            .ok()?;
        let name = env.new_string("com.quartz.launcher.MainActivity").ok()?;
        let cls_obj = env
            .call_method(
                &loader,
                "loadClass",
                "(Ljava/lang/String;)Ljava/lang/Class;",
                &[JValue::Object(&name.into())],
            )
            .ok()?
            .l()
            .ok()?;
        let cls = JClass::from(cls_obj);
        env.call_static_method(
            &cls,
            "setGameOrientationStatic",
            "(Z)V",
            &[JValue::Bool(u8::from(landscape))],
        )
        .ok()?;
        Some(())
    });
    if ok.is_none() {
        eprintln!("[quartz] 方向切换 JNI 调用失败（忽略，不影响启动）");
    }
}

// ---------------------------------------------------------------------------
// Android 存储解析（error13 / os error 13 = EACCES 根因修复 + 权限分级）
// ---------------------------------------------------------------------------

/// 存储诊断信息（启动时确定，UI 经 get_env_info 展示）。
#[derive(Debug, Clone)]
pub struct StorageDiag {
    pub mode: &'static str,
    pub root: PathBuf,
    pub layout_ok: bool,
    pub layout_error: Option<String>,
}

static STORAGE_DIAG: Mutex<Option<StorageDiag>> = Mutex::new(None);

/// 迁移互斥：焦点/可见性事件可能连续触发多次 reinit_storage，防止并发重复拷贝。
static MIGRATING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Android 平台解析真实可写目录；ndk-context 未初始化等异常一律回退硬编码路径。
///
/// 优先级（与授权状态联动）：
/// 1. 已授权完整文件权限 -> 公共标准目录 /storage/emulated/0/Quartz/files（可见、卸载不丢）
/// 2. 应用外部私有目录（getExternalFilesDir，可浏览，卸载丢失）
/// 3. 应用内部私有目录（getFilesDir，必定可写）
#[cfg(target_os = "android")]
fn resolve_android_root() -> (PathBuf, &'static str) {
    if storage_permission_granted() {
        let public = PathBuf::from("/storage/emulated/0/Quartz/files");
        if std::fs::create_dir_all(&public).is_ok() {
            return (public, "public");
        }
        eprintln!("[quartz] 公共标准目录创建失败，回退应用私有目录");
    }
    let result = with_jni(|env, context| {
        // 外部私有目录（用户可用文件管理器查看 .minecraft）
        if let Some(p) = jni_external_files_dir(env, context) {
            return Some((p, "external"));
        }
        // 内部私有目录（必定可写）
        jni_internal_files_dir(env, context).map(|p| (p, "internal"))
    });
    result.unwrap_or_else(|| {
        (
            PathBuf::from(format!(
                "/storage/emulated/0/Android/data/{}/files",
                paths::ANDROID_PACKAGE_ID
            )),
            "fallback",
        )
    })
}

/// Android 内部私有目录（Context.getFilesDir()，任何设备必定可写；exec 可用）。
#[cfg(target_os = "android")]
fn resolve_android_internal() -> Option<PathBuf> {
    with_jni(|env, context| jni_internal_files_dir(env, context))
}

/// 初始化游戏根目录与 JVM pack 目录并创建标准布局；返回诊断信息。
fn init_storage() -> StorageDiag {
    #[cfg(target_os = "android")]
    let (root, mode) = resolve_android_root();
    #[cfg(not(target_os = "android"))]
    let (root, mode) = (paths::game_root(), "desktop");

    let _ = paths::set_runtime_root(root.clone());
    let mut layout = paths::ensure_layout();

    // 外部目录不可写（EACCES / os error 13）时回退内部私有目录，
    // 保证离线登录与下载在任何设备上永远可用。
    #[cfg(target_os = "android")]
    let mut mode = mode;
    #[cfg(target_os = "android")]
    if layout.is_err() && mode != "internal" {
        eprintln!(
            "[quartz] 当前根目录不可写（{:?}），回退内部存储",
            layout.as_ref().err()
        );
        if let Some(internal) = resolve_android_internal() {
            let _ = paths::set_runtime_root(internal.clone());
            layout = paths::ensure_layout();
            mode = "internal";
        }
    }

    // JVM pack 必须放在可 exec 的内部目录（FUSE noexec + targetSdk>=29 禁 exec 的约束）
    #[cfg(target_os = "android")]
    if let Some(internal) = resolve_android_internal() {
        paths::set_runtime_jres_root(internal.join("jres"));
    }

    let diag = StorageDiag {
        mode,
        root: paths::game_root(),
        layout_ok: layout.is_ok(),
        layout_error: layout.err().map(|e| format!("{e}")),
    };
    eprintln!(
        "[quartz] 存储初始化：mode={} root={} jres={}",
        diag.mode,
        diag.root.display(),
        paths::runtime_dir().display()
    );
    diag
}

fn storage_json(diag: &StorageDiag) -> serde_json::Value {
    json!({
        "gameDir": diag.root.to_string_lossy(),
        "storageMode": diag.mode,
        "layoutOk": diag.layout_ok,
        "layoutError": diag.layout_error,
        "fullAccess": storage_permission_granted(),
        "freeBytes": quartz_core::space::free_space(&diag.root),
    })
}

// ---------------------------------------------------------------------------
// 崩溃证据落盘 + 启动自愈（v8「一打开就闪退」加固）
// ---------------------------------------------------------------------------
//
// 「闪退」的三类根因都在这里落证据 / 自愈：
// 1. Rust panic（任意线程）：panic hook 追加标记到唯一崩溃日志，下次启动 UI 横幅
//    直接展示原因 —— 不再是黑盒闪退；
// 2. Java/Kotlin 未捕获异常：QuartzApp（gen/android）同一文件追加 JAVA 标记；
// 3. WebView 状态损坏导致的打开即崩：QuartzApp 检测 2 分钟内 ≥2 次崩溃，
//    在 WebView 创建前清理 app_webview（Chromium profile）自愈。
//
// 另：失败下载每次重试都留下大文件（.qz-part/.part/.staging，PoJav APK 154MB /
// JRE pack 38MB），反复失败会把 userdata 分区塞满 —— FUSE 与应用私有目录同分区，
// 塞满后 WebView 无法初始化即「打开就闪退」。启动时自动清理残留 + 下载前空间
// 门槛（space 模块）从根上折断这个循环。

/// 崩溃日志唯一权威路径（run() 最早阶段解析；panic hook 与 get_diagnostics 共用）。
/// Android 用内部私有目录（必定可写，无需任何权限）。
static CRASH_LOG: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

fn crash_log_path() -> PathBuf {
    #[cfg(target_os = "android")]
    {
        if let Some(p) = resolve_android_internal() {
            return p.join("crash").join("quartz-crash.log");
        }
        // JNI 不可用时的壳层回退（应用私有目录固定路径）
        PathBuf::from("/data/data/com.quartz.launcher/files/crash/quartz-crash.log")
    }
    #[cfg(not(target_os = "android"))]
    {
        paths::game_root().join("logs").join("quartz-crash.log")
    }
}

/// 追加一条崩溃标记。格式（Rust/Java 两端约定，QuartzApp.kt 同款解析）：
/// `==== CRASH @<epoch秒> <KIND> ====` 换行正文。失败静默（panic hook 内不能报错）。
fn append_crash_marker(kind: &str, body: &str) {
    let Some(path) = CRASH_LOG.get() else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    use std::io::Write as _;
    let text = format!("==== CRASH @{epoch} {kind} ====\n{body}\n");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(f, "{text}");
        let _ = f.flush();
    }
    // 授权时同步一份到公共目录：用户可用文件管理器直接取出日志（不依赖 UI）
    #[cfg(target_os = "android")]
    {
        if storage_permission_granted() {
            let public = paths::game_root().join("logs").join("quartz-crash.log");
            if let Some(parent) = public.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(public)
            {
                let _ = writeln!(f, "{text}");
            }
        }
    }
}

/// 安装 panic hook：Rust panic（任意线程）先落盘再交回默认打印（logcat 保留）。
/// 必须在 tauri::Builder 构建前调用 —— setup 钩子/主线程 panic 由此才有证据。
fn install_panic_hook() {
    let _ = CRASH_LOG.set(crash_log_path());
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "未知位置".into());
        let msg = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "非字符串 panic 载荷".into());
        let thread = std::thread::current()
            .name()
            .unwrap_or("<unnamed>")
            .to_string();
        let bt = std::backtrace::Backtrace::force_capture().to_string();
        let bt_tail = bt
            .lines()
            .filter(|l| !l.trim().is_empty())
            .take(40)
            .collect::<Vec<_>>()
            .join("\n");
        append_crash_marker(
            "RUST-PANIC",
            &format!(
                "thread: {thread}\nlocation: {location}\nmessage: {msg}\nbacktrace(40):\n{bt_tail}"
            ),
        );
        eprintln!("[quartz-panic] {msg} @ {location}");
        default_hook(info);
    }));
}

/// 清理失败下载残留的临时文件（*.qz-part / *.part / *.tmp 与 .*.staging 目录）。
/// 只动顶层明确的临时名，绝不碰正常数据。返回回收字节数（eprintln 汇报）。
fn cleanup_stale_partials() {
    let roots = [paths::runtime_dir(), paths::game_root()];
    let mut reclaimed: u64 = 0;
    for root in roots {
        for entry in std::fs::read_dir(&root).into_iter().flatten().flatten() {
            let p = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let is_dir = p.is_dir();
            let stale_part = !is_dir
                && (name.ends_with(".qz-part")
                    || name.ends_with(".part")
                    || name.ends_with(".tmp"));
            let stale_staging = is_dir && name.starts_with('.') && name.ends_with(".staging");
            if stale_part || stale_staging {
                let size = entry
                    .metadata()
                    .map(|m| if m.is_dir() { 0 } else { m.len() })
                    .unwrap_or(0);
                let ok = if is_dir {
                    std::fs::remove_dir_all(&p).is_ok()
                } else {
                    std::fs::remove_file(&p).is_ok()
                };
                if ok {
                    reclaimed += size;
                    eprintln!(
                        "[quartz] 已清理残留下载缓存：{}（{} 字节）",
                        p.display(),
                        size
                    );
                }
            }
        }
    }
    if reclaimed > 0 {
        eprintln!("[quartz] 共回收残留下载缓存 {} 字节", reclaimed);
    }
}

/// 下载类命令的可用空间下限（精确需求在 core 各下载函数内按实际字节数校验）。
const DOWNLOAD_FLOOR: u64 = 200 * 1024 * 1024;

/// 下载前底线检查：可用空间 < 200MB 直接拦（ENOSPC 在 WebView/系统层面后果严重）。
fn check_download_floor(target: &Path) -> Result<(), String> {
    match quartz_core::space::free_space(target) {
        Some(free) if free < DOWNLOAD_FLOOR => Err(quartz_core::space::insufficient(
            DOWNLOAD_FLOOR,
            free,
            "执行下载",
        )),
        _ => Ok(()),
    }
}

/// 字符安全截断（按 char 而非字节，避免切断 UTF-8）。
fn clamp_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    s.chars().take(max).collect()
}

/// 从崩溃日志提取最后一个标记块（含标记行，最多 60 行）。
fn last_crash_summary() -> Option<String> {
    let path = CRASH_LOG.get()?;
    let text = std::fs::read_to_string(path).ok()?;
    let mut blocks: Vec<Vec<&str>> = Vec::new();
    for line in text.lines() {
        if line.starts_with("==== CRASH @") {
            blocks.push(vec![line]);
        } else if let Some(last) = blocks.last_mut() {
            last.push(line);
        }
    }
    let last = blocks.last()?;
    let body = last
        .iter()
        .take(60)
        .copied()
        .collect::<Vec<&str>>()
        .join("\n");
    Some(clamp_chars(&body, 3000))
}

/// 最近一次游戏启动日志尾部（SIGABRT/退出码诊断在 UI 上直达，无需文件管理器）。
fn last_launch_tail() -> Option<String> {
    let dir = paths::game_root().join("logs");
    let mut best: Option<(PathBuf, std::time::SystemTime)> = None;
    for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with("launch-") && name.ends_with(".log") {
            let t = e
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            if best.as_ref().map(|(_, bt)| t > *bt).unwrap_or(true) {
                best = Some((e.path(), t));
            }
        }
    }
    let (p, _) = best?;
    let text = std::fs::read_to_string(&p).ok()?;
    let lines: Vec<&str> = text.lines().collect();
    let tail = lines.len().saturating_sub(60);
    Some(clamp_chars(&lines[tail..].join("\n"), 3000))
}

/// 启动诊断：上次崩溃原因 + 存储剩余 + 最近启动日志尾部（UI 开屏横幅展示）。
#[tauri::command]
fn get_diagnostics() -> serde_json::Value {
    json!({
        "lastCrash": last_crash_summary(),
        "lastLaunchTail": last_launch_tail(),
        "freeBytes": quartz_core::space::free_space(&paths::game_root()),
        "crashLogPath": CRASH_LOG.get().map(|p| p.display().to_string()),
    })
}

/// 递归收集「源存在且目标缺失/不一致」的文件（不覆盖目标已有的同名同尺寸文件）。
fn collect_migrate_files(src: &Path, dst: &Path, out: &mut Vec<(PathBuf, PathBuf, u64)>) {
    let Ok(entries) = std::fs::read_dir(src) else {
        return;
    };
    for entry in entries.flatten() {
        let sp = entry.path();
        let dp = dst.join(entry.file_name());
        if sp.is_dir() {
            collect_migrate_files(&sp, &dp, out);
        } else if sp.is_file() {
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            let dst_size = std::fs::metadata(&dp).map(|m| m.len()).unwrap_or(u64::MAX);
            if dst_size != size {
                out.push((sp, dp, size));
            }
        }
    }
}

/// 后台迁移：把旧根（私有目录）的数据搬到新根（公共标准目录），
/// 经 `quartz://migrate` 事件上报 {percent,done,error}。
fn migrate_root(from: PathBuf, to: PathBuf, app: AppHandle) {
    let mut plan: Vec<(PathBuf, PathBuf, u64)> = Vec::new();
    collect_migrate_files(&from.join(".minecraft"), &to.join(".minecraft"), &mut plan);
    let old_accounts = from.join("accounts.json");
    let new_accounts = to.join("accounts.json");
    let migrate_accounts = old_accounts.exists() && !new_accounts.exists();
    if migrate_accounts {
        let size = std::fs::metadata(&old_accounts)
            .map(|m| m.len())
            .unwrap_or(0);
        plan.push((old_accounts, new_accounts, size));
    }

    if plan.is_empty() {
        let _ = app.emit(
            "quartz://migrate",
            &json!({ "percent": 100.0, "done": true, "error": null }),
        );
        return;
    }

    let total: u64 = plan.iter().map(|(_, _, s)| *s).sum::<u64>().max(1);
    eprintln!(
        "[quartz] 开始迁移存储：{} 个文件（{} 字节）",
        plan.len(),
        total
    );
    let mut copied: u64 = 0;
    let mut last = std::time::Instant::now() - std::time::Duration::from_secs(1);
    for (src, dst, size) in plan {
        if let Some(parent) = dst.parent() {
            if std::fs::create_dir_all(parent).is_err() {
                let _ = app.emit(
                    "quartz://migrate",
                    &json!({ "percent": (copied as f64 / total as f64) * 100.0, "done": false,
                             "error": format!("无法创建目录 {}", parent.display()) }),
                );
                return;
            }
        }
        if std::fs::copy(&src, &dst).is_err() {
            let _ = app.emit(
                "quartz://migrate",
                &json!({ "percent": (copied as f64 / total as f64) * 100.0, "done": false,
                         "error": format!("复制失败：{} -> {}", src.display(), dst.display()) }),
            );
            return;
        }
        copied += size;
        let now = std::time::Instant::now();
        if now.duration_since(last) >= std::time::Duration::from_millis(150) {
            last = now;
            let _ = app.emit(
                "quartz://migrate",
                &json!({ "percent": (copied as f64 / total as f64) * 100.0, "done": false, "error": null }),
            );
        }
    }
    eprintln!("[quartz] 存储迁移完成");
    let _ = app.emit(
        "quartz://migrate",
        &json!({ "percent": 100.0, "done": true, "error": null }),
    );
}

/// 权限变化后重解析存储根；根变化时后台迁移旧数据（幂等，跳过目标已有文件）。
#[tauri::command]
fn reinit_storage(app: AppHandle, state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let old_root = paths::game_root();
    let diag = init_storage();
    *STORAGE_DIAG.lock().map_err(|_| "内部状态锁异常")? = Some(diag.clone());
    let new_root = paths::game_root();
    // 游戏运行中不迁移（避免拷贝正在被 JVM 读写的文件）；同时防并发迁移。
    let game_running = state
        .game_pid
        .lock()
        .map_err(|_| "内部状态锁异常")?
        .is_some();
    let mut do_migrate = old_root != new_root && old_root.exists() && !game_running;
    if do_migrate {
        do_migrate = !MIGRATING.swap(true, std::sync::atomic::Ordering::SeqCst);
    }
    if do_migrate {
        let app2 = app.clone();
        std::thread::spawn(move || {
            migrate_root(old_root, new_root, app2);
            MIGRATING.store(false, std::sync::atomic::Ordering::SeqCst);
        });
    }
    Ok(storage_json(&diag))
}

// ---------------------------------------------------------------------------
// 启动器设置（内存分配等；game_root/quartz-settings.json）
// ---------------------------------------------------------------------------

/// 设置文件路径（游戏根目录，不与 .minecraft 冲突）。
fn settings_file() -> PathBuf {
    paths::game_root().join("quartz-settings.json")
}

/// 读取内存分配（默认 1024MB；越界值回退默认）。
fn get_memory_mb_inner() -> u32 {
    let Ok(text) = std::fs::read_to_string(settings_file()) else {
        return 1024;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return 1024;
    };
    let mb = v["memoryMb"].as_u64().unwrap_or(1024) as u32;
    if (512..=8192).contains(&mb) {
        mb
    } else {
        1024
    }
}

/// 读取内存分配（UI 设置页）。
#[tauri::command]
fn get_memory_mb() -> u32 {
    get_memory_mb_inner()
}

/// 保存内存分配（512–8192MB，256MB 步进由 UI 控制；越界拒绝）。
#[tauri::command]
fn set_memory_mb(mb: u32) -> Result<(), String> {
    if !(512..=8192).contains(&mb) {
        return Err("内存范围应为 512–8192 MB".into());
    }
    let path = settings_file();
    let mut v = serde_json::from_str::<serde_json::Value>(
        &std::fs::read_to_string(&path).unwrap_or_else(|_| "{}".into()),
    )
    .unwrap_or_else(|_| serde_json::json!({}));
    v["memoryMb"] = serde_json::json!(mb);
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// 资产完整性（v9「资源自动化」）：状态查询 + 一键修复 + 启动前自动补全
// ---------------------------------------------------------------------------

/// 资产完整性状态（stat-only 秒级扫描；设置页展示）。
#[tauri::command]
fn assets_status(version_id: String) -> Result<serde_json::Value, String> {
    match instance::count_missing_assets(&version_id) {
        Ok(s) => Ok(json!({
            "indexPresent": !s.index_id.is_empty(),
            "indexId": s.index_id,
            "total": s.total,
            "missing": s.missing,
            "missingBytes": s.missing_bytes,
        })),
        Err(e) => Err(e.to_string()),
    }
}

/// 一键修复资产（只补缺失对象；进度走 download://progress 事件，与游戏下载同通道）。
#[tauri::command]
async fn repair_assets(
    version_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<usize, String> {
    {
        let mut busy = state.downloading.lock().map_err(|_| "内部状态锁异常")?;
        if *busy {
            return Err("已有下载任务正在进行，请等待完成后再试".to_string());
        }
        *busy = true;
    }
    check_download_floor(&paths::game_root())?;
    let busy = state.downloading.clone();
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<instance::DownloadProgress>(128);
        let forwarder_app = app2.clone();
        let forwarder = tokio::spawn(async move {
            while let Some(ev) = rx.recv().await {
                let _ = forwarder_app.emit("download://progress", &ev);
            }
        });
        let client = meta::http_client();
        let result = instance::repair_assets(&client, &version_id, tx).await;
        if let Err(e) = &result {
            let _ = app2.emit(
                "download://progress",
                &instance::DownloadProgress {
                    stage: "error".to_string(),
                    current: 0,
                    total: 0,
                    percent: 0.0,
                    file: String::new(),
                    done: false,
                    error: Some(e.to_string()),
                },
            );
        }
        drop(result);
        if let Ok(mut busy) = busy.lock() {
            *busy = false;
        }
        let _ = forwarder.await;
    });
    Ok(0)
}

/// 启动前自动补全缺失资产（阻塞，进度写启动控制台；缺失为 0 时瞬时返回）。
fn ensure_assets_ready(version_id: &str, app: &AppHandle) -> Result<(), String> {
    let status = instance::count_missing_assets(version_id).map_err(|e| e.to_string())?;
    if status.index_id.is_empty() {
        return Err(format!(
            "版本 {version_id} 缺少资产索引（{version_id}.json 无 assetIndex 或索引文件未落盘），请重新下载该版本"
        ));
    }
    if status.missing == 0 {
        return Ok(());
    }
    app.emit(
        "launch://log",
        &LaunchLog {
            source: "quartz".into(),
            line: format!(
                "[quartz] 检测到 {} 个缺失资产文件（约 {}），启动前自动补全…",
                status.missing,
                quartz_core::space::human_bytes(status.missing_bytes)
            ),
        },
    )
    .ok();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<instance::DownloadProgress>(128);
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(ev) = rx.recv().await {
            let line = if ev.stage == "error" {
                format!("[quartz] 资产修复错误：{}", ev.error.unwrap_or_default())
            } else if ev.stage == "done" {
                "[quartz] 资产补全完成".to_string()
            } else {
                format!(
                    "[quartz] 资产修复 {}%（{}/{}）",
                    ev.percent as u32,
                    quartz_core::space::human_bytes(ev.current),
                    quartz_core::space::human_bytes(ev.total)
                )
            };
            let _ = app2.emit(
                "launch://log",
                &LaunchLog {
                    source: "quartz".into(),
                    line,
                },
            );
        }
    });
    let client = meta::http_client();
    let vid = version_id.to_string();
    tauri::async_runtime::block_on(async move { instance::repair_assets(&client, &vid, tx).await })
        .map(|n| {
            let _ = n;
        })
        .map_err(|e| format!("资产自动补全失败（可在设置页手动重试）：{e}"))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// 启动日志查看（设置页：最近日志列表 + 尾部读取）
// ---------------------------------------------------------------------------

/// 日志文件条目。
#[derive(Serialize)]
pub struct LogEntry {
    pub name: String,
    pub size: u64,
    pub modified_ms: u64,
}

/// 列出启动日志（launch-*.log / game-*.log / quartz-crash.log，按修改时间倒序，最多 12 条）。
#[tauri::command]
fn list_logs() -> Vec<LogEntry> {
    let mut out: Vec<LogEntry> = Vec::new();
    let dirs = [paths::game_root().join("logs")];
    for dir in dirs {
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if !(name.starts_with("launch-") && name.ends_with(".log"))
                    && !(name.starts_with("game-") && name.ends_with(".log"))
                    && name != "quartz-crash.log"
                {
                    continue;
                }
                let meta = e.metadata().ok();
                let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
                let modified_ms = meta
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);
                out.push(LogEntry {
                    name,
                    size,
                    modified_ms,
                });
            }
        }
    }
    // 崩溃日志（内部目录）也纳入展示
    if let Some(p) = CRASH_LOG.get() {
        if let Ok(meta) = std::fs::metadata(p) {
            out.push(LogEntry {
                name: "quartz-crash.log".into(),
                size: meta.len(),
                modified_ms: meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0),
            });
        }
    }
    out.sort_by_key(|e| std::cmp::Reverse(e.modified_ms));
    out.truncate(12);
    out
}

/// 读取日志尾部（最多 300 行，单行截断 500 字符；读 quartz-crash.log 时取内部目录）。
#[tauri::command]
fn read_log_tail(name: String) -> Result<String, String> {
    if name.contains("..") || name.contains('/') || name.contains('\\') {
        return Err("非法日志名".into());
    }
    let path = if name == "quartz-crash.log" {
        CRASH_LOG.get().cloned().ok_or("崩溃日志不可用")?
    } else {
        paths::game_root().join("logs").join(&name)
    };
    let text = std::fs::read_to_string(&path).map_err(|e| format!("读取失败：{e}"))?;
    let lines: Vec<String> = text
        .lines()
        .rev()
        .take(300)
        .map(|l| clamp_chars(l, 500))
        .collect();
    Ok(lines.into_iter().rev().collect::<Vec<_>>().join("\n"))
}

// ---------------------------------------------------------------------------
// 应用信息
// ---------------------------------------------------------------------------

/// 应用基础信息。
#[tauri::command]
fn app_meta() -> serde_json::Value {
    let hal = platform::default_hal();
    json!({
        "name": "Quartz Launcher",
        "version": env!("CARGO_PKG_VERSION"),
        "os": hal.os().rule_name(),
        "arch": host_arch(),
    })
}

/// 存储诊断：根目录绝对路径、是否可写、存储模式、完整文件权限状态。
#[tauri::command]
fn get_env_info() -> serde_json::Value {
    let diag = STORAGE_DIAG
        .lock()
        .map_err(|_| "内部状态锁异常")
        .ok()
        .and_then(|g| g.clone())
        .unwrap_or_else(init_storage);
    storage_json(&diag)
}

/// 渲染器建议：按平台默认能力 + 手动覆盖。
#[tauri::command]
fn renderer_advice(mc: String, caps: Option<serde_json::Value>) -> serde_json::Value {
    let hal = platform::default_hal();
    let os = hal.os().bus_name();
    let caps = caps
        .and_then(|v| serde_json::from_value::<DeviceCaps>(v).ok())
        .unwrap_or_else(|| DeviceCaps::for_os(os));
    let required = renderbus::required_profile(&mc);
    match renderbus::select(os, host_arch(), required, &caps) {
        Some(p) => json!({
            "id": p.id, "backend": p.backend, "provides": p.provides,
            "note": p.note, "required": required,
        }),
        None => json!({ "id": null, "required": required,
            "reason": "设备无满足 GL 语义的加速路径（性能红线）" }),
    }
}

/// 生成启动计划（离线演示：内嵌 1.20.4 样例 + 离线账号）。
#[tauri::command]
fn plan_offline(player: String, memory_mb: u32) -> Result<serde_json::Value, String> {
    let version: model::VersionJson =
        serde_json::from_str(quartz_core::fixtures::VERSION_1_20_4).map_err(|e| e.to_string())?;
    let ctx = RuntimeCtx::new("linux", host_arch());
    let allowed: Vec<&model::Library> = version
        .libraries
        .iter()
        .filter(|l| meta::library_allowed(l, &ctx))
        .collect();
    let account = auth::offline_account(&player);
    let root = paths::minecraft_dir();

    let plan = jvmops::build_launch_plan(&jvmops::PlanInputs {
        ctx: &ctx,
        version: &version,
        allowed_libs: &allowed,
        libraries_root: &root.join("libraries"),
        natives_root: &root.join("natives"),
        assets_root: &root.join("assets"),
        versions_root: &root.join("versions"),
        instance_dir: &paths::game_root().join("instances").join("demo"),
        account: &account,
        memory_mb,
        java_major: version
            .java_version
            .as_ref()
            .map(|j| j.major_version)
            .unwrap_or(17),
        java_path: None,
        window: None,
        client_jar_path: None,
        lwjgl_classes_jar: None,
    })
    .map_err(|e| e.to_string())?;

    serde_json::to_value(&plan).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// 离线账号
// ---------------------------------------------------------------------------

/// 离线登录：校验用户名 -> MD5 离线 UUID -> 写 files/accounts.json -> 直接进主界面。
#[tauri::command]
fn offline_login(name: String) -> Result<auth::Account, String> {
    auth::add_offline_account(&name).map_err(|e| e.to_string())
}

/// 当前选中账号；未登录返回 null。
#[tauri::command]
fn get_account() -> Option<auth::Account> {
    auth::active_account().unwrap_or(None)
}

/// 账号列表条目（附选中标记）。
#[derive(Serialize)]
pub struct AccountEntry {
    #[serde(flatten)]
    pub account: auth::Account,
    pub selected: bool,
}

/// 全部账号（供账户页管理）。
#[tauri::command]
fn list_accounts() -> Result<Vec<AccountEntry>, String> {
    let store = auth::load_store().map_err(|e| e.to_string())?;
    Ok(store
        .accounts
        .into_iter()
        .map(|a| {
            let selected = store.selected.as_deref() == Some(a.uuid.as_str());
            AccountEntry {
                account: a,
                selected,
            }
        })
        .collect())
}

/// 切换选中账号。
#[tauri::command]
fn select_account(uuid: String) -> Result<auth::Account, String> {
    let mut store = auth::load_store().map_err(|e| e.to_string())?;
    let account = store
        .accounts
        .iter()
        .find(|a| a.uuid == uuid)
        .cloned()
        .ok_or_else(|| "账号不存在".to_string())?;
    store.selected = Some(uuid);
    auth::save_store(&store).map_err(|e| e.to_string())?;
    Ok(account)
}

/// 删除账号；若删除的是选中账号，则自动选中剩余的最后一个。
#[tauri::command]
fn remove_account(uuid: String) -> Result<bool, String> {
    let mut store = auth::load_store().map_err(|e| e.to_string())?;
    let before = store.accounts.len();
    store.accounts.retain(|a| a.uuid != uuid);
    let removed = store.accounts.len() != before;
    if !removed {
        return Ok(false);
    }
    if store.selected.as_deref() == Some(uuid.as_str()) {
        store.selected = store.accounts.last().map(|a| a.uuid.clone());
    }
    auth::save_store(&store).map_err(|e| e.to_string())?;
    Ok(true)
}

// ---------------------------------------------------------------------------
// 版本列表与下载
// ---------------------------------------------------------------------------

/// 版本列表条目（release 优先排序）。
#[derive(Debug, Clone, Serialize)]
pub struct VersionInfo {
    pub id: String,
    #[serde(rename = "type")]
    pub version_type: String,
    pub url: String,
}

/// 拉取真实 version_manifest_v2 并排序：release -> snapshot -> old_beta -> old_alpha。
#[tauri::command]
async fn list_versions() -> Result<Vec<VersionInfo>, String> {
    let client = meta::http_client();
    let manifest = meta::fetch_version_manifest(&client)
        .await
        .map_err(|e| format!("版本清单拉取失败：{e}"))?;
    const KIND_ORDER: [&str; 4] = ["release", "snapshot", "old_beta", "old_alpha"];
    let mut out = Vec::with_capacity(manifest.versions.len());
    for kind in KIND_ORDER {
        for v in manifest.versions.iter().filter(|v| v.kind == kind) {
            out.push(VersionInfo {
                id: v.id.clone(),
                version_type: v.kind.clone(),
                url: v.url.clone(),
            });
        }
    }
    for v in manifest
        .versions
        .iter()
        .filter(|v| !KIND_ORDER.contains(&v.kind.as_str()))
    {
        out.push(VersionInfo {
            id: v.id.clone(),
            version_type: v.kind.clone(),
            url: v.url.clone(),
        });
    }
    Ok(out)
}

/// 本地已安装版本（versions/<v>/<v>.jar 存在即为已安装）。
#[tauri::command]
fn list_installed_versions() -> Vec<String> {
    let dir = paths::versions_dir();
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            if paths::version_jar(&name).exists() {
                out.push(name);
            }
        }
    }
    out.sort();
    out.reverse();
    out
}

/// 后台启动完整下载管线（8 并发 + SHA-1 校验 + 3 次重试 + 原子落盘），
/// 进度经 `download://progress` 事件上报 {stage,current,total,percent,file,done,error}。
#[tauri::command]
async fn download_game(
    version_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    {
        let mut busy = state.downloading.lock().map_err(|_| "内部状态锁异常")?;
        if *busy {
            return Err("已有下载任务正在进行，请等待完成后再试".to_string());
        }
        *busy = true;
    }
    // 空间底线检查（精确需求在 instance::install_standard 内按缺失任务字节数校验）
    check_download_floor(&paths::game_root())?;
    let busy = state.downloading.clone();
    let app = app.clone();

    tauri::async_runtime::spawn(async move {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<instance::DownloadProgress>(128);
        let forwarder_app = app.clone();
        let forwarder = tokio::spawn(async move {
            while let Some(ev) = rx.recv().await {
                let _ = forwarder_app.emit("download://progress", &ev);
            }
        });

        let client = meta::http_client();
        let result = instance::install_standard(&client, &version_id, tx).await;
        if let Err(e) = &result {
            // 失败也走事件通道（UI 红字报错 + 可重试），绝不静默。
            let _ = app.emit(
                "download://progress",
                &instance::DownloadProgress {
                    stage: "error".to_string(),
                    current: 0,
                    total: 0,
                    percent: 0.0,
                    file: String::new(),
                    done: false,
                    error: Some(e.to_string()),
                },
            );
        }
        drop(result);
        if let Ok(mut busy) = busy.lock() {
            *busy = false;
        }
        // forwarder 在 tx 关闭后自然退出
        let _ = forwarder.await;
    });

    Ok(())
}

/// 标准游戏根目录绝对路径（UI 常显）。
#[tauri::command]
fn get_game_dir() -> String {
    paths::game_root().to_string_lossy().into_owned()
}

// ---------------------------------------------------------------------------
// JVM 运行时 pack（M0 启动链路）
// ---------------------------------------------------------------------------

/// JVM pack 状态：installed / version / path / 是否有下载源。
#[tauri::command]
fn get_runtime_status(java_major: u32) -> serde_json::Value {
    json!({
        "installed": runtime::is_installed(java_major),
        "version": runtime::installed_version(java_major),
        "path": runtime::install_dir(java_major).to_string_lossy(),
        "specAvailable": runtime::spec_for(java_major, host_arch()).is_some(),
    })
}

/// 后台下载 JVM 运行时 pack，进度经 `runtime://progress` 上报（与下载游戏事件同构）。
#[tauri::command]
async fn download_runtime(
    java_major: u32,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    if state
        .runtime_busy
        .swap(true, std::sync::atomic::Ordering::SeqCst)
    {
        return Err("已有运行时下载任务正在进行".to_string());
    }
    check_download_floor(&paths::runtime_dir())?;
    let busy = state.runtime_busy.clone();
    let app = app.clone();

    tauri::async_runtime::spawn(async move {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<DownloadProgress>(64);
        let forwarder_app = app.clone();
        let forwarder = tokio::spawn(async move {
            while let Some(ev) = rx.recv().await {
                let _ = forwarder_app.emit("runtime://progress", &ev);
            }
        });

        let client = meta::http_client();
        let result = runtime::download_and_install(&client, java_major, host_arch(), tx).await;
        if let Err(e) = &result {
            let _ = app.emit(
                "runtime://progress",
                &DownloadProgress {
                    stage: "error".to_string(),
                    current: 0,
                    total: 0,
                    percent: 0.0,
                    file: String::new(),
                    done: false,
                    error: Some(e.to_string()),
                },
            );
        }
        drop(result);
        busy.store(false, std::sync::atomic::Ordering::SeqCst);
        let _ = forwarder.await;
    });

    Ok(())
}

// ---------------------------------------------------------------------------
// 渲染层 pack + LWJGL 安卓 pack（全部在线下载，APK 不内置）
// ---------------------------------------------------------------------------

/// 渲染器目录 + 安装状态 + 当前选中。
#[tauri::command]
fn list_renderers() -> serde_json::Value {
    let active = renderers::active();
    json!(renderers::catalog()
        .iter()
        .map(|s| {
            json!({
                "id": s.id,
                "name": s.name,
                "note": s.note,
                "eglLib": s.egl_lib,
                "glLib": s.gl_lib,
                "sizeHint": s.size_hint,
                "specAvailable": s.source.is_some(),
                "installed": renderers::is_installed(s.id),
                "active": s.id == active,
            })
        })
        .collect::<Vec<_>>())
}

/// 后台下载渲染层 pack，进度经 `renderer://progress` 上报。
#[tauri::command]
async fn download_renderer(
    id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    if state
        .renderer_busy
        .swap(true, std::sync::atomic::Ordering::SeqCst)
    {
        return Err("已有渲染层下载任务正在进行".to_string());
    }
    check_download_floor(&paths::runtime_dir())?;
    let busy = state.renderer_busy.clone();
    let app = app.clone();

    tauri::async_runtime::spawn(async move {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<DownloadProgress>(64);
        let forwarder_app = app.clone();
        let forwarder = tokio::spawn(async move {
            while let Some(ev) = rx.recv().await {
                let _ = forwarder_app.emit("renderer://progress", &ev);
            }
        });

        let client = meta::http_client();
        let result = renderers::download_and_install(&client, &id, tx).await;
        if let Err(e) = &result {
            let _ = app.emit(
                "renderer://progress",
                &DownloadProgress {
                    stage: "error".to_string(),
                    current: 0,
                    total: 0,
                    percent: 0.0,
                    file: String::new(),
                    done: false,
                    error: Some(e.to_string()),
                },
            );
        }
        drop(result);
        busy.store(false, std::sync::atomic::Ordering::SeqCst);
        let _ = forwarder.await;
    });

    Ok(())
}

/// 选择当前渲染器（落盘 renderer.json）。
#[tauri::command]
fn set_active_renderer(id: String) -> Result<(), String> {
    renderers::set_active(&id).map_err(|e| e.to_string())
}

/// LWJGL 安卓 pack 状态。
#[tauri::command]
fn get_lwjgl_status() -> serde_json::Value {
    json!({
        "installed": lwjgl::is_installed(),
        "path": lwjgl::install_dir().to_string_lossy(),
    })
}

/// 后台下载 LWJGL 安卓 pack，进度经 `lwjgl://progress` 上报。
#[tauri::command]
async fn download_lwjgl(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    if state
        .lwjgl_busy
        .swap(true, std::sync::atomic::Ordering::SeqCst)
    {
        return Err("已有 LWJGL 下载任务正在进行".to_string());
    }
    check_download_floor(&paths::runtime_dir())?;
    let busy = state.lwjgl_busy.clone();
    let app = app.clone();

    tauri::async_runtime::spawn(async move {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<DownloadProgress>(64);
        let forwarder_app = app.clone();
        let forwarder = tokio::spawn(async move {
            while let Some(ev) = rx.recv().await {
                let _ = forwarder_app.emit("lwjgl://progress", &ev);
            }
        });

        let client = meta::http_client();
        let result = lwjgl::download_and_install(&client, tx).await;
        if let Err(e) = &result {
            let _ = app.emit(
                "lwjgl://progress",
                &DownloadProgress {
                    stage: "error".to_string(),
                    current: 0,
                    total: 0,
                    percent: 0.0,
                    file: String::new(),
                    done: false,
                    error: Some(e.to_string()),
                },
            );
        }
        drop(result);
        busy.store(false, std::sync::atomic::Ordering::SeqCst);
        let _ = forwarder.await;
    });

    Ok(())
}

// ---------------------------------------------------------------------------
// Modloader 安装 / 实例体系 / 微软与 authlib 登录 / 整合包 / 内容管理 / 设置
// （对标 FCL / ZL2 的完整启动器功能面；编排全部在 quartz-core，命令层只做接线）
// ---------------------------------------------------------------------------

/// loader 类型校验与映射：fabric / quilt / forge / neoforge / optifine。
fn parse_loader_spec(loader: &str, loader_version: &str) -> Result<instance::LoaderSpec, String> {
    let lv = loader_version.to_string();
    match loader {
        "fabric" => Ok(instance::LoaderSpec::Fabric { loader_version: lv }),
        "quilt" => Ok(instance::LoaderSpec::Quilt { loader_version: lv }),
        "forge" => Ok(instance::LoaderSpec::Forge { version: lv }),
        "neoforge" => Ok(instance::LoaderSpec::NeoForge { version: lv }),
        "optifine" => Ok(instance::LoaderSpec::OptiFine { version: lv }),
        _ => Err(format!(
            "未知 loader 类型：{loader}（支持 fabric/quilt/forge/neoforge/optifine）"
        )),
    }
}

/// loader 版本列表条目。
#[derive(Serialize)]
pub struct LoaderEntry {
    pub version: String,
    pub stable: bool,
}

/// 某 MC 版本可用的 loader 版本列表。
#[tauri::command]
async fn list_loader_versions(loader: String, mc: String) -> Result<Vec<LoaderEntry>, String> {
    let out = match loader.as_str() {
        "fabric" => modkit::loaders::list_fabric_loaders(&mc)
            .await
            .map_err(|e| format!("Fabric 版本列表拉取失败：{e}"))?
            .into_iter()
            .map(|v| LoaderEntry {
                version: v.version,
                stable: v.stable,
            })
            .collect(),
        "quilt" => modkit::loaders::list_quilt_loaders(&mc)
            .await
            .map_err(|e| format!("Quilt 版本列表拉取失败：{e}"))?
            .into_iter()
            .map(|v| LoaderEntry {
                version: v.version,
                stable: v.stable,
            })
            .collect(),
        "forge" => modkit::loaders::list_forge_versions(&mc)
            .await
            .map_err(|e| format!("Forge 版本列表拉取失败：{e}"))?
            .into_iter()
            .map(|v| LoaderEntry {
                version: v,
                stable: true,
            })
            .collect(),
        "neoforge" => modkit::loaders::list_neoforge_versions(&mc)
            .await
            .map_err(|e| format!("NeoForge 版本列表拉取失败：{e}"))?
            .into_iter()
            .map(|v| LoaderEntry {
                version: v,
                stable: true,
            })
            .collect(),
        "optifine" => modkit::loaders::list_optifine_versions(&mc)
            .await
            .map_err(|e| format!("OptiFine 版本列表拉取失败：{e}"))?
            .into_iter()
            .map(|v| LoaderEntry {
                version: v.name.trim_start_matches("OptiFine ").to_string(),
                stable: true,
            })
            .collect(),
        _ => return Err(format!("未知 loader 类型：{loader}")),
    };
    Ok(out)
}

/// 安装 modloader（基版本缺失自动先装原版；进度经 `loader://progress` 上报）。
#[tauri::command]
async fn install_loader_cmd(
    app: AppHandle,
    state: State<'_, AppState>,
    mc: String,
    loader: String,
    loader_version: String,
) -> Result<(), String> {
    if state
        .downloading
        .lock()
        .map_err(|_| "内部状态锁异常")?
        .clone()
    {
        return Err("已有下载任务正在进行，请等待完成后再试".to_string());
    }
    let spec = parse_loader_spec(&loader, &loader_version)?;
    check_download_floor(&paths::game_root())?;
    *state.downloading.lock().map_err(|_| "内部状态锁异常")? = true;
    let busy = state.downloading.clone();
    let app = app.clone();

    tauri::async_runtime::spawn(async move {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<instance::DownloadProgress>(128);
        let forwarder_app = app.clone();
        let forwarder = tokio::spawn(async move {
            while let Some(ev) = rx.recv().await {
                let _ = forwarder_app.emit("loader://progress", &ev);
            }
        });
        let client = meta::http_client();
        let result = instance::install_loader(&client, &mc, &spec, tx).await;
        if let Err(e) = &result {
            let _ = app.emit(
                "loader://progress",
                &instance::DownloadProgress {
                    stage: "error".to_string(),
                    current: 0,
                    total: 0,
                    percent: 0.0,
                    file: String::new(),
                    done: false,
                    error: Some(e.to_string()),
                },
            );
        }
        drop(result);
        if let Ok(mut busy) = busy.lock() {
            *busy = false;
        }
        let _ = forwarder.await;
    });
    Ok(())
}

// ---- 实例体系（FCL 版本隔离语义） ----

fn instances_store() -> instance::InstanceStore {
    instance::InstanceStore::new(paths::minecraft_dir().join("instances"))
}

/// 实例列表（附选中标记与 loader 描述）。
#[tauri::command]
fn list_instances() -> Result<Vec<serde_json::Value>, String> {
    let store = instances_store();
    let list = store.load().map_err(|e| e.to_string())?;
    let selected = settings::load().selected_instance;
    Ok(list
        .into_iter()
        .map(|i| {
            let loader_desc = i
                .loader
                .as_ref()
                .map(|l| match l {
                    instance::LoaderSpec::Fabric { loader_version } => {
                        format!("Fabric {loader_version}")
                    }
                    instance::LoaderSpec::Quilt { loader_version } => {
                        format!("Quilt {loader_version}")
                    }
                    instance::LoaderSpec::Forge { version } => format!("Forge {version}"),
                    instance::LoaderSpec::NeoForge { version } => format!("NeoForge {version}"),
                    instance::LoaderSpec::OptiFine { version } => format!("OptiFine {version}"),
                })
                .unwrap_or_else(|| "原版".to_string());
            json!({
                "id": i.id,
                "name": i.name,
                "mcVersion": i.mc_version,
                "loaderDesc": loader_desc,
                "javaMajor": i.java_major,
                "memoryMb": i.memory_mb,
                "renderer": i.renderer,
                "createdAt": i.created_at,
                "selected": selected.as_deref() == Some(i.id.as_str()),
            })
        })
        .collect())
}

/// 新建实例（不触发下载；首次启动按需安装版本与 loader）。
#[tauri::command]
fn create_instance_cmd(
    name: String,
    mc_version: String,
    loader: Option<String>,
    loader_version: Option<String>,
) -> Result<serde_json::Value, String> {
    let spec = match (&loader, &loader_version) {
        (Some(l), Some(v)) => Some(parse_loader_spec(l, v)?),
        (Some(_), None) => return Err("指定了 loader 但缺少 loader 版本".to_string()),
        _ => None,
    };
    // java 主版本：优先 loader 要求，其次基版本（安装时由版本 JSON 决定）
    let java_major = spec
        .as_ref()
        .map(|_| modkit::loader_requires_jvm_major(loader.as_ref().unwrap()))
        .unwrap_or(17);
    let input = instance::CreateInstance {
        id: format!("{}-{}", mc_version.replace('.', "_"), chrono_like_id()),
        name: name.trim().to_string(),
        mc_version: mc_version.clone(),
        loader: spec,
        java_major,
        memory_mb: get_memory_mb_inner(),
    };
    let inst = instances_store().create(input).map_err(|e| e.to_string())?;
    settings::save(&settings::Settings {
        selected_instance: Some(inst.id.clone()),
        ..settings::load()
    })
    .map_err(|e| e.to_string())?;
    Ok(json!({ "id": inst.id, "name": inst.name }))
}

fn chrono_like_id() -> String {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("{n:x}")
}

/// 删除实例（含目录）。
#[tauri::command]
fn delete_instance_cmd(id: String) -> Result<bool, String> {
    let ok = instances_store().delete(&id).map_err(|e| e.to_string())?;
    let mut s = settings::load();
    if s.selected_instance.as_deref() == Some(id.as_str()) {
        s.selected_instance = None;
        settings::save(&s).map_err(|e| e.to_string())?;
    }
    Ok(ok)
}

/// 选中实例（None = 回到全局 .minecraft）。
#[tauri::command]
fn select_instance(id: Option<String>) -> Result<(), String> {
    if let Some(i) = &id {
        let _ = instances_store()
            .get(i)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("实例不存在：{i}"))?;
    }
    settings::save(&settings::Settings {
        selected_instance: id,
        ..settings::load()
    })
    .map_err(|e| e.to_string())
}

/// 修改实例内存分配。
#[tauri::command]
fn set_instance_memory(id: String, memory_mb: u32) -> Result<(), String> {
    instances_store()
        .update(&id, |i| i.memory_mb = memory_mb)
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 修改实例渲染器（None = 自动）。
#[tauri::command]
fn set_instance_renderer(id: String, renderer: Option<String>) -> Result<(), String> {
    instances_store()
        .update(&id, |i| i.renderer = renderer)
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ---- 微软账号（设备码） ----

/// 微软设备码登录：后台轮询直到成功/失败。
/// 事件：`ms://prompt`（DeviceCodeResp，展示 user_code + verification_uri）、
/// `ms://result`（{ ok, account?, error? }）。
/// client_id 必须在设置页配置项目自有 Azure Public Client ID。
#[tauri::command]
fn ms_login(app: AppHandle) -> Result<(), String> {
    let client_id = settings::load()
        .azure_client_id
        .unwrap_or_else(|| auth::microsoft::DEFAULT_CLIENT_ID.to_string());
    if client_id.is_empty() {
        return Err(
            "未配置 Azure client_id：请在「设置」页填入项目自有 Public Client ID 后重试"
                .to_string(),
        );
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let client = meta::http_client();
        let result = async {
            let dc = auth::microsoft::device_code_request(&client, &client_id)
                .await
                .map_err(|e| format!("设备码申请失败：{e}"))?;
            let _ = app.emit("ms://prompt", &dc);
            // 按 interval 轮询（与 device_code_login 内部一致的语义，但事件化）
            let mut interval = dc.interval.max(1);
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(interval)).await;
                match auth::microsoft::poll_token(&client, &client_id, &dc.device_code)
                    .await
                    .map_err(|e| e.to_string())?
                {
                    auth::microsoft::TokenOutcome::Success(msa) => {
                        let xbl = auth::microsoft::xbl_user_token(&client, &msa.access_token)
                            .await
                            .map_err(|e| format!("XBL 认证失败：{e}"))?;
                        let xsts = auth::microsoft::xsts_authorize(&client, &xbl.token)
                            .await
                            .map_err(|e| format!("XSTS 认证失败：{e}"))?;
                        let uhs = xsts
                            .display_claims
                            .xui
                            .first()
                            .map(|c| c.uhs.clone())
                            .ok_or("XSTS 响应缺少 uhs")?;
                        let mc = auth::microsoft::mc_login(&client, &uhs, &xsts.token)
                            .await
                            .map_err(|e| format!("Minecraft 会话获取失败：{e}"))?;
                        let prof = auth::microsoft::profile(&client, &mc.access_token)
                            .await
                            .map_err(|e| format!("档案获取失败：{e}"))?;
                        let account = auth::add_account(auth::Account {
                            kind: auth::AccountKind::Microsoft,
                            username: prof.name,
                            uuid: auth::microsoft::dashed_uuid(&prof.id),
                            access_token: mc.access_token,
                            refresh_token: msa.refresh_token,
                            user_type: Some("msa".into()),
                            authlib_server: None,
                        })
                        .map_err(|e| e.to_string())?;
                        break Ok(account);
                    }
                    auth::microsoft::TokenOutcome::Pending { interval: i } => {
                        interval = i;
                        continue;
                    }
                    auth::microsoft::TokenOutcome::Failed { error } => {
                        break Err(format!("MSA 授权失败：{error}"));
                    }
                }
            }
        }
        .await;
        let payload = match result {
            Ok(account) => {
                json!({ "ok": true, "account": account, "error": serde_json::Value::Null })
            }
            Err(e) => json!({ "ok": false, "account": serde_json::Value::Null, "error": e }),
        };
        let _ = app.emit("ms://result", &payload);
    });
    Ok(())
}

// ---- authlib-injector 外置登录 ----

/// authlib 外置登录（Yggdrasil 协议）：成功后写入 accounts.json 并选中。
#[tauri::command]
async fn authlib_login(
    server: String,
    username: String,
    password: String,
) -> Result<auth::Account, String> {
    let client = meta::http_client();
    let account = auth::authlib::login(&client, &server, &username, &password)
        .await
        .map_err(|e| format!("外置登录失败：{e}"))?;
    // injector jar 未就绪只警告不阻塞（启动时再检查）
    auth::add_account(account.clone()).map_err(|e| e.to_string())?;
    Ok(account)
}

/// authlib-injector agent 安装状态。
#[tauri::command]
fn authlib_status() -> serde_json::Value {
    json!({
        "installed": auth::authlib::injector_jar().exists(),
        "version": auth::authlib::installed_version(),
    })
}

/// 下载 authlib-injector agent jar（后台；`authlib://progress` 上报）。
#[tauri::command]
async fn download_authlib(app: AppHandle) -> Result<(), String> {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let client = meta::http_client();
        let result = auth::authlib::download_injector(&client).await;
        let payload = match &result {
            Ok(v) => json!({ "done": true, "version": v, "error": serde_json::Value::Null }),
            Err(e) => {
                json!({ "done": false, "version": serde_json::Value::Null, "error": e.to_string() })
            }
        };
        let _ = app.emit("authlib://progress", &payload);
    });
    Ok(())
}

// ---- Modrinth 整合包导入 ----

/// 导入 .mrpack 整合包：建实例 -> 装 loader -> 下载内容文件。
/// 进度：`mrpack://progress`（与 download://progress 同构）。
#[tauri::command]
async fn import_mrpack(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    instance_name: Option<String>,
) -> Result<(), String> {
    if state
        .downloading
        .lock()
        .map_err(|_| "内部状态锁异常")?
        .clone()
    {
        return Err("已有下载任务正在进行，请等待完成后再试".to_string());
    }
    let src = std::path::PathBuf::from(&path);
    if !src.exists() {
        return Err(format!("整合包文件不存在：{path}"));
    }
    // ① 先读索引（同步、快）确定 loader 与版本要求
    let index = modkit::mrpack::read_index(&src).map_err(|e| format!("整合包解析失败：{e}"))?;
    if index.game != "minecraft" {
        return Err(format!("不支持的整合包游戏：{}", index.game));
    }
    let mc = index
        .dependencies
        .get("minecraft")
        .cloned()
        .ok_or("整合包缺少 minecraft 版本依赖")?;
    // ② 建实例（loader 稍后按依赖安装）
    let loader_kind: Option<(&str, String)> = index
        .dependencies
        .get("fabric-loader")
        .map(|v| ("fabric", v.clone()))
        .or_else(|| {
            index
                .dependencies
                .get("quilt-loader")
                .map(|v| ("quilt", v.clone()))
        })
        .or_else(|| {
            index
                .dependencies
                .get("forge")
                .map(|v| ("forge", v.clone()))
        })
        .or_else(|| {
            index
                .dependencies
                .get("neoforge")
                .map(|v| ("neoforge", v.clone()))
        });
    let name = instance_name.unwrap_or_else(|| index.name.clone());
    let (spec, java_major) = match &loader_kind {
        Some((k, v)) => (
            Some(parse_loader_spec(k, v)?),
            modkit::loader_requires_jvm_major(k),
        ),
        None => (None, 17),
    };
    let input = instance::CreateInstance {
        id: format!("mrpack-{}", chrono_like_id()),
        name: name.trim().to_string(),
        mc_version: mc.clone(),
        loader: spec,
        java_major,
        memory_mb: get_memory_mb_inner(),
    };
    let inst = instances_store().create(input).map_err(|e| e.to_string())?;
    let inst_dir = instances_store().instance_dir(&inst.id);

    *state.downloading.lock().map_err(|_| "内部状态锁异常")? = true;
    let busy = state.downloading.clone();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<instance::DownloadProgress>(128);
        let forwarder_app = app.clone();
        let forwarder = tokio::spawn(async move {
            while let Some(ev) = rx.recv().await {
                let _ = forwarder_app.emit("mrpack://progress", &ev);
            }
        });
        let client = meta::http_client();
        let result: Result<(), String> = async {
            // ③ loader 安装（链式先装基版本）
            if let Some(spec) = &inst.loader {
                instance::install_loader(&client, &mc, spec, tx.clone())
                    .await
                    .map_err(|e| format!("loader 安装失败：{e}"))?;
            } else {
                instance::install_standard(&client, &mc, tx.clone())
                    .await
                    .map_err(|e| format!("基版本安装失败：{e}"))?;
            }
            // ④ 内容文件（mods/shaderpacks/config 等，按 env.client 过滤）
            let tasks = modkit::mrpack::client_download_tasks(&index, &inst_dir);
            instance::execute_download_plan(tasks, &tx)
                .await
                .map_err(|e| format!("整合包文件下载失败：{e}"))?;
            Ok(())
        }
        .await;
        if let Err(e) = &result {
            let _ = app.emit(
                "mrpack://progress",
                &instance::DownloadProgress {
                    stage: "error".to_string(),
                    current: 0,
                    total: 0,
                    percent: 0.0,
                    file: String::new(),
                    done: false,
                    error: Some(e.clone()),
                },
            );
        }
        if let Ok(mut busy) = busy.lock() {
            *busy = false;
        }
        let _ = forwarder.await;
    });
    Ok(())
}

// ---- 内容管理 ----

/// 列出某类内容（mods/resourcepacks/shaderpacks/saves/screenshots/logs）。
/// instance_id 有值时列出实例目录下的内容。
#[tauri::command]
fn list_content_cmd(
    kind: String,
    instance_id: Option<String>,
) -> Result<Vec<content::ContentEntry>, String> {
    let k = content::ContentKind::parse(&kind).ok_or_else(|| format!("未知内容类型：{kind}"))?;
    let dir = instance_dir_for(&instance_id);
    content::list_content(k, dir.as_deref()).map_err(|e| e.to_string())
}

/// 删除内容项。
#[tauri::command]
fn delete_content_cmd(
    kind: String,
    name: String,
    instance_id: Option<String>,
) -> Result<bool, String> {
    let k = content::ContentKind::parse(&kind).ok_or_else(|| format!("未知内容类型：{kind}"))?;
    let dir = instance_dir_for(&instance_id);
    content::delete_content(k, &name, dir.as_deref()).map_err(|e| e.to_string())
}

/// 实例内容目录解析（None = 全局 .minecraft）。
fn instance_dir_for(instance_id: &Option<String>) -> Option<std::path::PathBuf> {
    instance_id
        .as_ref()
        .map(|id| instances_store().instance_dir(id))
}

// ---- 设置 ----

/// 当前设置（不含敏感字段原文：azure_client_id 只返回是否已配置）。
#[tauri::command]
fn get_settings() -> serde_json::Value {
    let s = settings::load();
    json!({
        "extraJvmArgs": s.extra_jvm_args,
        "azureClientIdSet": s.azure_client_id.is_some(),
        "azureClientId": s.azure_client_id,
        "memoryMb": s.memory_mb,
        "selectedInstance": s.selected_instance,
    })
}

/// 覆盖式设置用户自定义 JVM 参数。
#[tauri::command]
fn set_jvm_args_cmd(args: Vec<String>) -> Result<(), String> {
    settings::set_jvm_args(args)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// 设置 Azure Public Client ID（None 清除）。
#[tauri::command]
fn set_azure_client_id(id: Option<String>) -> Result<(), String> {
    settings::set_azure_client_id(id)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// 游戏启动（M0 已接入）
// ---------------------------------------------------------------------------

/// 启动日志事件（stdout / stderr / quartz 系统行）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchLog {
    pub source: String,
    pub line: String,
}

/// 退出事件。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchExit {
    pub code: i32,
    pub version: String,
    /// 本次启动的完整日志文件路径（UI 可直接展示/复制）。
    pub log_file: Option<String>,
    /// 退出时日志尾部（游戏进程 + 启动器）：崩溃证据直达 UI，用户反馈可直接粘贴。
    pub log_tail: Option<String>,
}

/// 启动前自检：版本文件（含继承链）/ Java 主版本 / 账号 / 运行时状态。
#[tauri::command]
fn launch_preflight(version_id: String) -> serde_json::Value {
    let chain_ok = meta::resolve_version_chain(&version_id).ok();
    let java_major = chain_ok
        .as_ref()
        .map(|r| {
            r.version
                .java_version
                .as_ref()
                .map(|j| j.major_version)
                .unwrap_or(17)
        })
        .unwrap_or(17);
    let jar_installed = chain_ok
        .as_ref()
        .map(|r| {
            paths::versions_dir()
                .join(&r.jar_version)
                .join(format!("{}.jar", r.jar_version))
                .exists()
        })
        .unwrap_or(false);
    let account_ok = auth::active_account().unwrap_or(None).is_some();
    json!({
        "versionInstalled": chain_ok.is_some() && jar_installed,
        "versionJsonOk": chain_ok.is_some(),
        "javaMajor": java_major,
        "accountOk": account_ok,
        "runtimeInstalled": runtime::is_installed(java_major),
        "runtimeSpecAvailable": runtime::spec_for(java_major, host_arch()).is_some(),
        "runtimeVersion": runtime::installed_version(java_major),
        "runtimeDir": runtime::install_dir(java_major).to_string_lossy(),
    })
}

/// 解析版本 JSON（含 inheritsFrom 链）并装配 LaunchPlan。
/// 实例隔离：instance_id 有值时游戏目录 = 实例目录（saves/mods/config 隔离，
/// 资产/库仍全局共享，与 FCL 的版本隔离语义一致）。
fn build_plan_for(
    version_id: &str,
    instance_id: Option<&str>,
) -> Result<(model::VersionJson, jvmops::LaunchPlan, u32), String> {
    let resolved =
        meta::resolve_version_chain(version_id).map_err(|e| format!("版本解析失败：{e}"))?;
    let version = resolved.version;
    let jar_path = paths::versions_dir()
        .join(&resolved.jar_version)
        .join(format!("{}.jar", resolved.jar_version));
    if !jar_path.exists() {
        return Err(format!(
            "版本 {} 的 client jar 缺失，请重新下载",
            resolved.jar_version
        ));
    }
    let major = version
        .java_version
        .as_ref()
        .map(|j| j.major_version)
        .unwrap_or(17);

    let ctx = instance::standard_rules_ctx();
    let allowed: Vec<&model::Library> = version
        .libraries
        .iter()
        .filter(|l| rules::evaluate(&l.rules, &ctx))
        .collect();

    let mc_dir = paths::minecraft_dir();
    let natives_root = mc_dir.join("natives");
    let instance_dir = match instance_id {
        Some(id) => {
            let store = instance::InstanceStore::new(mc_dir.join("instances"));
            let found = store
                .get(id)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("实例不存在：{id}"))?;
            let _ = found;
            store.instance_dir(id)
        }
        None => mc_dir.clone(),
    };
    // Android：移植版 LWJGL 合并 jar 替换 Mojang org.lwjgl:*（glibc natives 不可用）
    // 启动前就地升级老版本 pack（修订版 1 → 2：jar 去 .sha1，消 LWJGL
    // 「Incompatible Java and native library versions」ERROR，免 154MB 重下）
    let lwjgl_jar = if cfg!(target_os = "android") && lwjgl::is_installed() {
        let _ = lwjgl::upgrade_in_place();
        Some(lwjgl::classes_jar())
    } else {
        None
    };
    let plan = jvmops::build_launch_plan(&jvmops::PlanInputs {
        ctx: &ctx,
        version: &version,
        allowed_libs: &allowed,
        libraries_root: &paths::libraries_dir(),
        natives_root: &natives_root,
        assets_root: &paths::assets_dir(),
        versions_root: &paths::versions_dir(),
        client_jar_path: Some(jar_path),
        instance_dir: &instance_dir,
        account: &auth::active_account()
            .map_err(|e| e.to_string())?
            .ok_or("尚未登录，请先在「账户」页添加账号")?,
        memory_mb: get_memory_mb_inner(),
        java_major: major,
        java_path: None,
        window: None,
        lwjgl_classes_jar: lwjgl_jar,
    })
    .map_err(|e| format!("启动计划装配失败：{e}"))?;
    Ok((version, plan, major))
}

/// Android 12+ tagged-pointer 兼容层 shim 落盘（源码：scripts/tagfix/quartz_tagfix.c）。
///
/// Android 12（API 31）起 AArch64 堆指针最高字节被系统保留，OpenJDK top-byte
/// 用途与之冲突 -> scudo CHECK 失败 abort（SIGABRT，退出码 134，典型崩在 GC
/// 工作线程）。修复 = java 进程早期 mallopt(M_BIONIC_SET_HEAP_TAGGING_LEVEL,
/// NONE)（Termux openjdk-17 PR#10347 同款方案）；shim 以 include_bytes 编入
/// 壳层，启动前写到 JVM pack 私有目录，经 LD_PRELOAD 注入 java 进程。
#[cfg(target_os = "android")]
fn ensure_tagfix_lib() -> Option<PathBuf> {
    const SHIM: &[u8] = include_bytes!("../tagfix/libqztagfix.so");
    let dst = paths::runtime_dir().join("libqztagfix.so");
    if let Ok(meta) = std::fs::metadata(&dst) {
        if meta.len() == SHIM.len() as u64 {
            return Some(dst);
        }
    }
    if std::fs::write(&dst, SHIM).is_ok() {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dst, std::fs::Permissions::from_mode(0o644));
        return Some(dst);
    }
    None
}

/// 拉起 JVM 子进程并托管日志/退出（`launch://log` / `launch://exit` 事件）。
/// v0.1.7 起 Android 走 game_bridge::launch_via_game_process（:game 独立进程）。
#[cfg_attr(target_os = "android", allow(dead_code))]
fn spawn_game(
    version_id: String,
    plan: jvmops::LaunchPlan,
    major: u32,
    game_pid: Arc<Mutex<Option<u32>>>,
    app: AppHandle,
) -> Result<(), String> {
    use std::io::BufRead;
    use std::io::Write;
    use std::process::{Command, Stdio};

    let java = runtime::java_bin(major);
    let jre_home = runtime::install_dir(major);
    let game_dir = paths::minecraft_dir();

    // ---- 持久化启动日志（完整证据链）----
    // stdout/stderr/quartz 行 + 完整命令行/环境 全部落盘 <game_root>/logs/launch-*.log。
    // 原生崩溃（SIGABRT 等）的 JVM 报告只写 hs_err 文件 / logcat，不会进 stderr ——
    // 因此异常退出时自动摘录 hs_err 到本日志，下次运行即可拿到全部诊断信息。
    let logs_dir = paths::game_root().join("logs");
    let _ = std::fs::create_dir_all(&logs_dir);
    let log_path = logs_dir.join(format!(
        "launch-{}.log",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    ));
    let log_file: Arc<Mutex<Option<std::fs::File>>> =
        Arc::new(Mutex::new(std::fs::File::create(&log_path).ok()));

    // 控制台 + 文件双写。
    let qlog = {
        let app = app.clone();
        let log_file = log_file.clone();
        move |source: &str, line: &str| {
            let _ = app.emit(
                "launch://log",
                &LaunchLog {
                    source: source.into(),
                    line: line.into(),
                },
            );
            if let Ok(mut guard) = log_file.lock() {
                if let Some(f) = guard.as_mut() {
                    let _ = writeln!(f, "[{source}] {line}");
                    let _ = f.flush();
                }
            }
        }
    };

    // ---- JVM 动态库搜索路径（必须先于 smoke test 计算，两处共用） ----
    // bionic linker 不认 RUNPATH，bin/java 找 libjli.so 完全依赖 LD_LIBRARY_PATH。
    // 顺序（先者优先）：渲染层目录 -> LWJGL 安卓 natives -> 游戏 natives ->
    // JRE lib / lib/jli / lib/server / lib/client。
    let mut ld_parts: Vec<String> = Vec::new();
    #[cfg(target_os = "android")]
    let active_renderer = {
        let id = renderers::active();
        let spec = renderers::spec_for(&id);
        if renderers::is_installed(&id) {
            ld_parts.push(renderers::install_dir(&id).to_string_lossy().into_owned());
        }
        if lwjgl::is_installed() {
            ld_parts.push(lwjgl::natives_dir().to_string_lossy().into_owned());
        }
        qlog(
            "quartz",
            &format!(
                "[quartz] 渲染层：{}（{}）｜ LWJGL 安卓包：{}",
                spec.map(|s| s.name).unwrap_or("未知"),
                id,
                if lwjgl::is_installed() {
                    "已就绪"
                } else {
                    "未安装"
                },
            ),
        );
        id
    };
    #[cfg(not(target_os = "android"))]
    let active_renderer = String::new();
    ld_parts.push(jre_home.join("lib").display().to_string());
    ld_parts.push(jre_home.join("lib/jli").display().to_string());
    ld_parts.push(jre_home.join("lib/server").display().to_string());
    ld_parts.push(jre_home.join("lib/client").display().to_string());
    ld_parts.push(plan.natives_dir.display().to_string());
    let ld_library_path = ld_parts.join(":");

    // Android 无 /tmp：JVM 的 hsperfdata / 临时文件目录必须显式指向内部可写目录
    //（tmpdir 初始化失败是 Pojav 系 JRE 早期 abort 的常见诱因）。
    #[cfg(target_os = "android")]
    let tmp_dir = {
        let d = paths::runtime_dir().join("tmp");
        let _ = std::fs::create_dir_all(&d);
        d
    };
    #[cfg(not(target_os = "android"))]
    let tmp_dir = std::env::temp_dir();

    // ---- Android JVM 参数包：java.library.path 私有目录优先 + 渲染层接线 ----
    // FUSE（/storage/emulated/0）挂载为 noexec，System.loadLibrary/dlopen 无法从
    // 公共目录装载 native：顺序必须是 LWJGL natives（含 pojavexec/jnidispatch）->
    // 渲染层 -> 游戏 natives（FUSE，仅兜底）。附加在 plan 参数之后（同名 -D 后者生效）。
    #[cfg(target_os = "android")]
    let (jlp, android_jvm_extra) = {
        let jlp = format!(
            "{}:{}:{}",
            lwjgl::natives_dir().display(),
            renderers::install_dir(&active_renderer).display(),
            plan.natives_dir.display(),
        );
        let gl_lib = renderers::spec_for(&active_renderer)
            .map(|s| s.gl_lib)
            .unwrap_or("libgl4es_114.so");
        let extra = vec![
            format!("-Djava.library.path={jlp}"),
            // liblwjgl_opengl 按该属性 dlopen 渲染器 GL 库；PojavRendererInit 也据它识别渲染层
            format!("-Dorg.lwjgl.opengl.libname={gl_lib}"),
            // tmpdir 显式内部化 + 关闭 hsperfdata（部分安卓构建在 /tmp 缺失时初始化 abort）
            format!("-Djava.io.tmpdir={}", tmp_dir.display()),
            "-XX:-UsePerfData".to_string(),
            // JVM 崩溃报告定向到日志目录（默认写 CWD，FUSE 上难定位）
            format!("-XX:ErrorFile={}", logs_dir.join("hs_err_%p.log").display()),
            // 早期初始化诊断：abort 若发生在 JVM 初始化阶段，stderr 会留下最后一步
            "-Xlog:init=info:stderr".to_string(),
        ];
        (jlp, extra)
    };
    #[cfg_attr(not(target_os = "android"), allow(unused_variables))]
    #[cfg(not(target_os = "android"))]
    let (jlp, android_jvm_extra): (String, Vec<String>) = (String::new(), Vec::new());

    // ---- Android 12+ tagged-pointer 兼容层（JRE GC 线程 SIGABRT 修复）----
    // 落盘 shim 并以 LD_PRELOAD 注入（smoke test / 二分自检 / 真实启动共用）。
    #[cfg(target_os = "android")]
    let tagfix_lib = ensure_tagfix_lib();
    #[cfg(target_os = "android")]
    {
        match tagfix_lib.as_ref() {
            Some(p) => qlog(
                "quartz",
                &format!(
                    "[quartz] tagged-pointer 兼容层：{}（LD_PRELOAD 注入）",
                    p.display()
                ),
            ),
            None => qlog(
                "quartz",
                "[quartz] tagged-pointer 兼容层落盘失败（跳过注入）",
            ),
        }
    }

    // ---- 头部：完整命令行 + 环境（诊断核心：一眼看出坏参数/坏路径） ----
    if let Ok(mut guard) = log_file.lock() {
        if let Some(f) = guard.as_mut() {
            let _ = writeln!(f, "==== Quartz launch {version_id} (java {major}) ====");
            let _ = writeln!(f, "java: {}", java.display());
            let _ = writeln!(f, "cwd: {}", plan.working_dir.display());
            let _ = writeln!(f, "JAVA_HOME: {}", jre_home.display());
            let _ = writeln!(f, "LD_LIBRARY_PATH: {ld_library_path}");
            let _ = writeln!(f, "TMPDIR: {}", tmp_dir.display());
            #[cfg(target_os = "android")]
            {
                let _ = writeln!(f, "java.library.path: {jlp}");
                let gl_lib = renderers::spec_for(&active_renderer)
                    .map(|s| s.gl_lib)
                    .unwrap_or("libgl4es_114.so");
                let _ = writeln!(f, "org.lwjgl.opengl.libname: {gl_lib}");
                let _ = writeln!(
                    f,
                    "LD_PRELOAD: {}",
                    tagfix_lib
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "未启用".to_string())
                );
            }
            let _ = writeln!(f, "---- jvm args ({} 个) ----", plan.jvm_args.len());
            for a in &plan.jvm_args {
                let _ = writeln!(f, "  {a}");
            }
            let _ = writeln!(f, "---- classpath ({} jars) ----", plan.classpath.len());
            for c in &plan.classpath {
                let _ = writeln!(f, "  {}", c.display());
            }
            let _ = writeln!(f, "main_class: {}", plan.main_class);
            let _ = writeln!(f, "---- game args ({} 个) ----", plan.game_args.len());
            for a in &plan.game_args {
                let _ = writeln!(f, "  {a}");
            }
            let _ = writeln!(f, "========================================");
            let _ = f.flush();
        }
    }

    // ---- smoke test：JRE 可执行性（与真实启动同 LD_LIBRARY_PATH/JAVA_HOME）----
    // Pojav android-openjdk 的 bin/java 无 RUNPATH，裸跑必报 CANNOT LINK。
    // 自检失败直接终止启动并回显全部输出（可诊断，不静默）。
    qlog(
        "quartz",
        &format!("[quartz] JRE {major}：{}", jre_home.display()),
    );
    let mut smoke = Command::new(&java);
    smoke
        .arg("-version")
        .env("JAVA_HOME", &jre_home)
        .env("LD_LIBRARY_PATH", &ld_library_path);
    #[cfg(target_os = "android")]
    {
        if let Some(p) = &tagfix_lib {
            smoke.env("LD_PRELOAD", p);
        }
    }
    match smoke.output() {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stderr);
            for line in text.lines().filter(|l| !l.trim().is_empty()).take(2) {
                qlog("quartz", &format!("[quartz] {line}"));
            }
        }
        Ok(out) => {
            // 自检退出码非 0：把完整输出带回给 UI（linker 报错 / JVM 初始化错误都在这里）
            let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&out.stderr));
            let tail: String = text
                .lines()
                .filter(|l| !l.trim().is_empty())
                .collect::<Vec<_>>()
                .join(" | ");
            qlog("quartz", &format!("[quartz] JVM 自检失败：{tail}"));
            return Err(format!(
                "JVM 自检失败（退出码 {}，LD_LIBRARY_PATH={}）：{}",
                out.status.code().unwrap_or(-1),
                ld_library_path,
                if tail.is_empty() {
                    "无输出（进程可能被信号杀死）"
                } else {
                    &tail
                }
            ));
        }
        Err(e) => {
            return Err(format!(
                "无法执行 JVM（{e}）。JVM pack 需位于可执行目录：{}",
                jre_home.display()
            ))
        }
    }

    // ---- 二分自检：完整 JVM 参数 + 相同环境/cwd，仅无 classpath 与主类 ----
    // 通过 => 崩溃（若有）必然在游戏类加载或原生库装载层，stderr 会留下痕迹；
    // 失败 => JVM 参数/环境层问题，当场带回证据报错，避免游戏进程静默 134 退出。
    #[cfg(target_os = "android")]
    {
        let mut probe = Command::new(&java);
        probe
            .args(&android_jvm_extra)
            .arg("-version")
            .env("JAVA_HOME", &jre_home)
            .env("LD_LIBRARY_PATH", &ld_library_path)
            .env("TMPDIR", &tmp_dir)
            .env("HOME", &game_dir)
            .current_dir(&plan.working_dir);
        for (k, v) in renderers::launch_env(&active_renderer) {
            probe.env(k, v);
        }
        if let Some(p) = &tagfix_lib {
            probe.env("LD_PRELOAD", p);
        }
        match probe.output() {
            Ok(out) if out.status.success() => {
                qlog("quartz", "[quartz] JVM 二分自检（完整参数）：通过");
            }
            Ok(out) => {
                let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
                text.push_str(&String::from_utf8_lossy(&out.stderr));
                let tail: String = text
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .take(12)
                    .collect::<Vec<_>>()
                    .join(" | ");
                let code = out.status.code().unwrap_or(-1);
                qlog(
                    "quartz",
                    &format!("[quartz] JVM 二分自检失败（退出码 {code}）：{tail}"),
                );
                if tail.is_empty() {
                    dump_crash_evidence(None, &qlog, &log_file);
                }
                return Err(format!(
                    "JVM 二分自检失败（退出码 {code}）：完整 JVM 参数即触发崩溃，详情见启动日志"
                ));
            }
            Err(e) => {
                return Err(format!("JVM 二分自检无法执行：{e}"));
            }
        }
    }

    // ---- 组装命令：jvm_args + main_class + game_args ----
    let mut cmd = Command::new(&java);
    cmd.args(&plan.jvm_args);
    #[cfg(target_os = "android")]
    {
        // 安卓 JVM 参数包在上方计算（二分自检与真实启动共用），此处直接复用。
        for a in &android_jvm_extra {
            cmd.arg(a);
        }
        for (k, v) in renderers::launch_env(&active_renderer) {
            cmd.env(k, v);
        }
        cmd.env("TMPDIR", &tmp_dir);
    }
    cmd.arg(&plan.main_class)
        .args(&plan.game_args)
        .current_dir(&plan.working_dir)
        .env("HOME", &game_dir)
        .env("JAVA_HOME", &jre_home)
        .env("LD_LIBRARY_PATH", &ld_library_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(target_os = "android")]
    {
        if let Some(p) = &tagfix_lib {
            cmd.env("LD_PRELOAD", p);
        }
    }

    // 进入游戏自动切横屏（游戏退出时在退出看护线程切回竖屏）。
    // 放在全部前置就绪之后：JVM/渲染层/LWJGL 任一失败都不切方向，用户仍处竖屏 UI。
    #[cfg(target_os = "android")]
    android_set_orientation(true);
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            #[cfg(target_os = "android")]
            android_set_orientation(false);
            return Err(format!("游戏进程启动失败：{e}"));
        }
    };
    let pid = child.id();
    if let Ok(mut slot) = game_pid.lock() {
        *slot = Some(pid);
    }
    qlog(
        "quartz",
        &format!(
            "[quartz] 游戏进程已启动 (pid {pid})，工作目录：{}",
            plan.working_dir.display()
        ),
    );

    // ---- 日志泵：stdout / stderr -> launch://log + 启动日志文件 ----
    if let Some(stdout) = child.stdout.take() {
        let qlog2 = qlog.clone();
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(stdout)
                .lines()
                .map_while(Result::ok)
            {
                qlog2("stdout", &line);
            }
        });
    }
    if let Some(stderr) = child.stderr.take() {
        let qlog2 = qlog.clone();
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(stderr)
                .lines()
                .map_while(Result::ok)
            {
                qlog2("stderr", &line);
            }
        });
    }

    // ---- 退出看护：wait -> launch://exit -> hs_err 摘录 -> 清理 pid 槽位 ----
    // 退出码语义：正常退出 = code；信号死亡 = 128+信号号（137=SIGKILL，134=SIGABRT）。
    let game_pid_exit = game_pid;
    std::thread::spawn(move || {
        let code = child.wait().map(|st| exit_status_code(&st)).unwrap_or(-1);
        // 游戏已退出：立即切回竖屏（启动器 UI 恢复竖屏布局）。
        #[cfg(target_os = "android")]
        android_set_orientation(false);
        if let Ok(mut slot) = game_pid_exit.lock() {
            // stop_game 已 take 时这里已是 None，避免覆盖后续新进程
            if slot.is_some() {
                *slot = None;
            }
        }
        qlog("quartz", &format!("[quartz] 游戏进程退出，代码 {code}"));
        if code == 137 {
            qlog("quartz", "[quartz] 退出码 137 = SIGKILL：常见于系统低内存回收（尝试调小内存）或 Android 12+ 幽灵进程限制");
        } else if code == 134 {
            qlog("quartz", "[quartz] 退出码 134 = SIGABRT：原生层崩溃，详细信息在 JVM 崩溃报告（hs_err）与 logcat，不进 stderr");
        }
        // 异常退出：先摘 logcat 崩溃证据（SIGABRT 不进 stderr/hs_err），再摘 hs_err。
        if code != 0 {
            #[cfg(target_os = "android")]
            dump_crash_evidence(Some(pid), &qlog, &log_file);
            let mut hs_errs: Vec<(PathBuf, std::time::SystemTime)> = Vec::new();
            for dir in [&logs_dir, &plan.working_dir, &game_dir, &jre_home] {
                if let Ok(rd) = std::fs::read_dir(dir) {
                    for e in rd.flatten() {
                        let name = e.file_name().to_string_lossy().into_owned();
                        if name.starts_with("hs_err_pid") && name.ends_with(".log") {
                            let mtime = e
                                .metadata()
                                .and_then(|m| m.modified())
                                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                            hs_errs.push((e.path(), mtime));
                        }
                    }
                }
            }
            if let Some((hp, _)) = hs_errs.into_iter().max_by_key(|(_, t)| *t) {
                qlog(
                    "quartz",
                    &format!("[quartz] 发现 JVM 崩溃报告：{}", hp.display()),
                );
                if let Ok(text) = std::fs::read_to_string(&hp) {
                    let excerpt: Vec<&str> = text.lines().take(60).collect();
                    if let Some(first) = excerpt.first() {
                        qlog("stderr", &format!("hs_err: {first}"));
                    }
                    if let Ok(mut guard) = log_file.lock() {
                        if let Some(f) = guard.as_mut() {
                            let _ = writeln!(f, "==== hs_err 摘录（前 60 行）====");
                            for l in &excerpt {
                                let _ = writeln!(f, "{l}");
                            }
                            let _ = f.flush();
                        }
                    }
                }
            }
            qlog(
                "quartz",
                &format!("[quartz] 完整启动日志：{}", log_path.display()),
            );
        }
        let _ = app.emit(
            "launch://exit",
            &LaunchExit {
                code,
                version: version_id,
                log_file: Some(log_path.to_string_lossy().into_owned()),
                log_tail: None,
            },
        );
    });
    Ok(())
}

/// ExitStatus -> 退出码：正常退出取 code()；信号死亡取 128+signal。
#[cfg_attr(target_os = "android", allow(dead_code))]
fn exit_status_code(st: &std::process::ExitStatus) -> i32 {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        st.code()
            .or_else(|| st.signal().map(|sig| 128 + sig))
            .unwrap_or(-1)
    }
    #[cfg(not(unix))]
    {
        st.code().unwrap_or(-1)
    }
}

/// 原生层崩溃（SIGABRT 等）的证据摘录。
///
/// bionic 的 abort 不写 stderr、JVM 不写 hs_err，唯一记录在 logd：
/// crash 缓冲（Fatal signal / Abort message）与 main 缓冲（同 uid 可读）。
/// 另附运行时组件清单，验证 pack 落位与 so 齐全。
/// （v0.1.5 起仅桌面子进程路径使用；Android 进程内模式崩溃即进程死亡，
/// 证据由启动日志 + QuartzApp 崩溃落盘承接。）
#[cfg_attr(target_os = "android", allow(dead_code))]
#[cfg(target_os = "android")]
fn dump_crash_evidence(
    pid: Option<u32>,
    qlog: &impl Fn(&str, &str),
    log_file: &Arc<Mutex<Option<std::fs::File>>>,
) {
    use std::io::Write;
    let write_log = |text: &str| {
        if let Ok(mut guard) = log_file.lock() {
            if let Some(f) = guard.as_mut() {
                let _ = writeln!(f, "{text}");
                let _ = f.flush();
            }
        }
    };
    // ---- logcat：crash 缓冲优先，main 缓冲兜底（各限 30 行防刷屏） ----
    for buf in ["crash", "main"] {
        let out = std::process::Command::new("logcat")
            .args(["-d", "-b", buf, "-t", "300"])
            .output();
        match out {
            Ok(out) => {
                let text = String::from_utf8_lossy(&out.stdout);
                let mut kept = 0usize;
                for line in text.lines().rev() {
                    if kept >= 30 {
                        break;
                    }
                    let hit = match pid {
                        // threadtime 格式：date time PID TID level tag: msg
                        Some(p) => line
                            .split_whitespace()
                            .nth(2)
                            .is_some_and(|f| f == p.to_string()),
                        None => {
                            line.contains("Fatal signal")
                                || line.contains("Abort message")
                                || line.contains("SIGABRT")
                                || line.contains("SIGSEGV")
                                || line.contains("SIGBUS")
                                || line.contains("CheckJNI")
                        }
                    };
                    if hit {
                        qlog("logcat", line);
                        write_log(&format!("[logcat-{buf}] {line}"));
                        kept += 1;
                    }
                }
                if kept == 0 {
                    qlog(
                        "quartz",
                        &format!("[quartz] logcat {buf} 缓冲无匹配记录（可能已被滚动或无权限）"),
                    );
                }
            }
            Err(e) => {
                qlog("quartz", &format!("[quartz] logcat 不可用（{e}）"));
            }
        }
    }
    // ---- 组件清单：确认运行时 pack 实际落位 ----
    let dirs: [(&str, PathBuf); 4] = [
        ("lwjgl3/natives", lwjgl::natives_dir()),
        (
            "renderers/active",
            renderers::install_dir(&renderers::active()),
        ),
        (
            "jre lib/server",
            runtime::install_dir(17).join("lib/server"),
        ),
        ("game natives", paths::minecraft_dir().join("natives")),
    ];
    for (label, dir) in dirs {
        let mut names: Vec<String> = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten().take(40) {
                let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                names.push(format!("{}({})", e.file_name().to_string_lossy(), size));
            }
        }
        let line = if names.is_empty() {
            format!("[quartz] 组件清单 {label}：空或不可读（{}）", dir.display())
        } else {
            format!("[quartz] 组件清单 {label}：{}", names.join(", "))
        };
        qlog("quartz", &line);
        write_log(&line);
    }
}

/// Android 启动前置：LWJGL 安卓 pack 缺失时阻塞在线下载（进度写启动控制台）。
#[cfg(target_os = "android")]
fn ensure_lwjgl_pack(app: &AppHandle) -> Result<(), String> {
    if lwjgl::is_installed() {
        return Ok(());
    }
    let _ = app.emit(
        "launch://log",
        &LaunchLog {
            source: "quartz".into(),
            line: "[quartz] 首次启动：在线获取 LWJGL 安卓包…".into(),
        },
    );
    let (tx, mut rx) = tokio::sync::mpsc::channel::<DownloadProgress>(64);
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(ev) = rx.recv().await {
            let line = if ev.stage == "error" {
                format!("[quartz] LWJGL 下载错误：{}", ev.error.unwrap_or_default())
            } else {
                format!("[quartz] LWJGL 下载 {}%", ev.percent as u32)
            };
            let _ = app2.emit(
                "launch://log",
                &LaunchLog {
                    source: "quartz".into(),
                    line,
                },
            );
        }
    });
    let client = meta::http_client();
    tauri::async_runtime::block_on(async move { lwjgl::download_and_install(&client, tx).await })
        .map_err(|e| format!("LWJGL 安卓包下载失败：{e}"))?;
    Ok(())
}

/// Android 启动前置：当前渲染层缺失时阻塞在线下载。
#[cfg(target_os = "android")]
fn ensure_renderer(app: &AppHandle) -> Result<(), String> {
    let active = renderers::active();
    if renderers::is_installed(&active) {
        return Ok(());
    }
    let has_source = renderers::spec_for(&active)
        .map(|s| s.source.is_some())
        .unwrap_or(false);
    if !has_source {
        return Err(format!("所选渲染层 {active} 暂无下载源，请在设置页更换"));
    }
    let _ = app.emit(
        "launch://log",
        &LaunchLog {
            source: "quartz".into(),
            line: format!("[quartz] 首次使用渲染层 {active}：在线获取…"),
        },
    );
    let (tx, mut rx) = tokio::sync::mpsc::channel::<DownloadProgress>(64);
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(ev) = rx.recv().await {
            let line = if ev.stage == "error" {
                format!("[quartz] 渲染层下载错误：{}", ev.error.unwrap_or_default())
            } else {
                format!("[quartz] 渲染层下载 {}%", ev.percent as u32)
            };
            let _ = app2.emit(
                "launch://log",
                &LaunchLog {
                    source: "quartz".into(),
                    line,
                },
            );
        }
    });
    let client = meta::http_client();
    tauri::async_runtime::block_on(async move {
        renderers::download_and_install(&client, &renderers::active(), tx).await
    })
    .map_err(|e| format!("渲染层下载失败：{e}"))?;
    Ok(())
}

/// 真实启动：版本 JSON -> LaunchPlan -> 游戏运行（M0 已接入）。
/// Android：进程内 JVM + 显示桥（SurfaceView + EGL，v0.1.5 游戏显示接入）；
/// 桌面：JVM 子进程。JVM pack 缺失时返回 `RUNTIME_MISSING:<major>`，UI 自动触发下载。
/// instance_id 有值时启动到实例隔离目录（FCL 版本隔离语义）。
#[tauri::command]
fn launch_game(
    version_id: String,
    instance_id: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let running = {
        #[cfg(target_os = "android")]
        {
            crate::game_bridge::game_running()
        }
        #[cfg(not(target_os = "android"))]
        {
            state
                .game_pid
                .lock()
                .map_err(|_| "内部状态锁异常")?
                .is_some()
        }
    };
    if running {
        return Err("已有游戏进程在运行，请先停止后再启动".to_string());
    }
    let (_version, plan, major) = build_plan_for(&version_id, instance_id.as_deref())?;
    if !runtime::is_installed(major) {
        return Err(format!("RUNTIME_MISSING:{major}"));
    }
    // 资源自动化（v9）：启动前 stat 扫描资产完整性，缺失则阻塞补全（进度写启动控制台）。
    ensure_assets_ready(&version_id, &app)?;
    #[cfg(target_os = "android")]
    {
        ensure_lwjgl_pack(&app)?;
        ensure_renderer(&app)?;
        // 显示桥落位（:game 进程 dlopen）：覆盖 Pojav APK 提取的旧 C 版 pojavexec
        //（jar↔so 错位根因）。
        let _ = &state;
        game_bridge::ensure_bridge_lib()?;
        // tagged-pointer 兼容层落盘（:game 进程 dlopen 用，Android 12+ GC SIGABRT 修复）
        if ensure_tagfix_lib().is_none() {
            eprintln!("[quartz] tagfix 落盘失败（Android 12+ 可能 GC abort）");
        }
    }
    std::fs::create_dir_all(&plan.working_dir).ok();
    std::fs::create_dir_all(&plan.natives_dir).ok();

    #[cfg(target_os = "android")]
    {
        // 持久化启动日志（:game 进程直写本文件，主进程 tail 转发到 WebView）
        let logs_dir = paths::game_root().join("logs");
        let _ = std::fs::create_dir_all(&logs_dir);
        let log_path = logs_dir.join(format!(
            "launch-{}.log",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        ));
        let log_file: Arc<Mutex<Option<std::fs::File>>> =
            Arc::new(Mutex::new(std::fs::File::create(&log_path).ok()));
        if let Ok(mut guard) = log_file.lock() {
            if let Some(f) = guard.as_mut() {
                use std::io::Write;
                let _ = writeln!(
                    f,
                    "==== Quartz launch {version_id} (java {major}, :game process) ===="
                );
                let _ = writeln!(
                    f,
                    "mode: separate :game process JVM + quartz-bridge (Rust display bridge)"
                );
                let _ = writeln!(f, "main_class: {}", plan.main_class);
                let _ = writeln!(f, "---- jvm options ({} 个) ----", plan.jvm_args.len());
                for a in &plan.jvm_args {
                    let _ = writeln!(f, "  {a}");
                }
                let _ = writeln!(f, "---- game args ({} 个) ----", plan.game_args.len());
                for a in &plan.game_args {
                    let _ = writeln!(f, "  {a}");
                }
                let _ = f.flush();
            }
        }
        let _ = app.emit(
            "launch://log",
            &LaunchLog {
                source: "quartz".into(),
                line: format!(
                    "[quartz] 启动模式：:game 独立进程 JVM + Rust 显示桥（{version_id}）"
                ),
            },
        );
        game_bridge::launch_via_game_process(
            version_id.clone(),
            plan,
            major,
            app.clone(),
            log_file,
            &log_path,
        )?;
        return Ok(());
    }

    #[cfg(not(target_os = "android"))]
    {
        spawn_game(
            version_id.clone(),
            plan,
            major,
            state.game_pid.clone(),
            app.clone(),
        )?;
        Ok(())
    }
}

/// 终止运行中的游戏。
/// Android（:game 独立进程）：向服务发 MSG_STOP，进程自杀，UI 由 binder
/// death 回调恢复；桌面：SIGKILL 游戏子进程。
#[tauri::command]
fn stop_game(state: State<'_, AppState>, app: AppHandle) -> Result<bool, String> {
    #[cfg(target_os = "android")]
    {
        let _ = &state;
        let _ = &app;
        if crate::game_bridge::game_running() {
            crate::game_bridge::stop_game_process();
            return Ok(true);
        }
        return Ok(false);
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = &app;
        let pid = state.game_pid.lock().map_err(|_| "内部状态锁异常")?.take();
        match pid {
            Some(pid) => {
                #[cfg(unix)]
                unsafe {
                    libc::kill(pid as libc::pid_t, libc::SIGKILL);
                }
                Ok(true)
            }
            None => Ok(false),
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 崩溃证据落盘必须先于一切：setup/主线程 panic 与后续任何线程 panic 都有据可查
    install_panic_hook();
    tauri::Builder::default()
        .manage(AppState::default())
        .setup(|_app| {
            // 启动即清理失败下载的残留缓存（回收空间，断「塞满分区 → 闪退」循环），
            // 再解析真实可写存储并创建标准目录布局（诊断信息供 UI 展示）。
            cleanup_stale_partials();
            let diag = init_storage();
            *STORAGE_DIAG.lock().expect("锁中毒") = Some(diag);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_meta,
            renderer_advice,
            plan_offline,
            offline_login,
            get_account,
            list_accounts,
            select_account,
            remove_account,
            list_versions,
            list_installed_versions,
            download_game,
            get_game_dir,
            get_env_info,
            get_diagnostics,
            get_permission_state,
            request_storage_permission,
            reinit_storage,
            get_runtime_status,
            download_runtime,
            launch_preflight,
            launch_game,
            stop_game,
            list_renderers,
            download_renderer,
            set_active_renderer,
            get_lwjgl_status,
            download_lwjgl,
            get_memory_mb,
            set_memory_mb,
            assets_status,
            repair_assets,
            list_logs,
            read_log_tail,
            list_loader_versions,
            install_loader_cmd,
            list_instances,
            create_instance_cmd,
            delete_instance_cmd,
            select_instance,
            set_instance_memory,
            set_instance_renderer,
            ms_login,
            authlib_login,
            authlib_status,
            download_authlib,
            import_mrpack,
            list_content_cmd,
            delete_content_cmd,
            get_settings,
            set_jvm_args_cmd,
            set_azure_client_id,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
