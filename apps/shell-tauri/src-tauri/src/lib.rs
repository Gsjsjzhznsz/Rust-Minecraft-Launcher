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
//! - `stop_game()`                终止游戏进程（SIGKILL）

use quartz_core::instance::DownloadProgress;
use quartz_core::renderbus::{self, DeviceCaps};
use quartz_core::{auth, host_arch, instance, jvmops, meta, model, paths, platform, rules, runtime, RuntimeCtx};
use serde::Serialize;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, State};

/// 下载并发互斥：同一时刻只允许一个下载任务。
#[derive(Default)]
pub struct AppState {
    downloading: Arc<std::sync::Mutex<bool>>,
    /// 运行时 pack 下载互斥。
    runtime_busy: Arc<std::sync::atomic::AtomicBool>,
    /// 运行中的游戏进程 pid（None = 空闲）。
    game_pid: Arc<Mutex<Option<u32>>>,
}

// ---------------------------------------------------------------------------
// Android JNI 基础（存储解析 / 权限检测共用）
// ---------------------------------------------------------------------------

#[cfg(target_os = "android")]
fn with_jni<T>(
    f: impl FnOnce(&mut jni::JNIEnv, &jni::objects::JObject) -> Option<T>,
) -> Option<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let ctx = ndk_context::android_context();
        let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }.ok()?;
        let mut env = vm.attach_current_thread().ok()?;
        let context = unsafe { jni::objects::JObject::from_raw(ctx.context().cast()) };
        f(&mut env, &context)
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
fn jni_external_files_dir(env: &mut jni::JNIEnv, context: &jni::objects::JObject) -> Option<PathBuf> {
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
fn jni_internal_files_dir(env: &mut jni::JNIEnv, context: &jni::objects::JObject) -> Option<PathBuf> {
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

/// Android 端存储权限是否已授权。
/// 优先走壳层 Kotlin 判定（Android 11+ = isExternalStorageManager，
/// 10- = WRITE_EXTERNAL_STORAGE 已 GRANTED）；JNI 不可用时回退纯 checkSelfPermission。
#[cfg(target_os = "android")]
fn android_storage_permission_granted() -> bool {
    // 1) MainActivity.isStorageAccessGranted()（覆盖 MANAGE_EXTERNAL_STORAGE）
    if let Some(granted) = with_jni(|env, ctx| {
        Some(
            env.call_method(ctx, "isStorageAccessGranted", "()Z", &[])
                .ok()?
                .z()
                .ok()?,
        )
    }) {
        return granted;
    }
    // 2) 回退：WRITE checkSelfPermission（仅覆盖旧模型，JNI 异常时尽力而为）
    use jni::objects::JValue;
    with_jni(|env, ctx| {
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
        // 1) MainActivity.requestStorageAccess()：按系统版本分流（11+ 设置页 / 10- 弹窗）
        let via_kotlin = with_jni(|env, ctx| {
            env.call_method(ctx, "requestStorageAccess", "()V", &[]).ok()?;
            Some(())
        });
        if via_kotlin.is_some() {
            return Ok(true);
        }
        // 2) 回退：直接 requestPermissions 弹窗（旧路径，仅 WRITE/READ）
        use jni::objects::JValue;
        with_jni(|env, ctx| {
            let write = env.new_string("android.permission.WRITE_EXTERNAL_STORAGE").ok()?;
            let read = env.new_string("android.permission.READ_EXTERNAL_STORAGE").ok()?;
            let write_obj: jni::objects::JObject = write.into();
            let arr = env
                .new_object_array(2, "java/lang/String", write_obj)
                .ok()?;
            let read_obj: jni::objects::JObject = read.into();
            env.set_object_array_element(&arr, 1, read_obj).ok()?;
            env.call_method(
                ctx,
                "requestPermissions",
                "([Ljava/lang/String;I)V",
                &[JValue::Object(&arr), JValue::Int(42)],
            )
            .ok()?;
            Some(())
        });
        Ok(true)
    }
    #[cfg(not(target_os = "android"))]
    {
        Ok(true)
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
        let size = std::fs::metadata(&old_accounts).map(|m| m.len()).unwrap_or(0);
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
    eprintln!("[quartz] 开始迁移存储：{} 个文件（{} 字节）", plan.len(), total);
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
        serde_json::from_str(quartz_core::fixtures::VERSION_1_20_4)
            .map_err(|e| e.to_string())?;
    let hal = platform::default_hal();
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
        java_major: version.java_version.as_ref().map(|j| j.major_version).unwrap_or(17),
        java_path: None,
        window: None,
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
            AccountEntry { account: a, selected }
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
async fn download_runtime(java_major: u32, app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    if state
        .runtime_busy
        .swap(true, std::sync::atomic::Ordering::SeqCst)
    {
        return Err("已有运行时下载任务正在进行".to_string());
    }
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
}

/// 启动前自检：版本文件 / Java 主版本 / 账号 / 运行时状态。
#[tauri::command]
fn launch_preflight(version_id: String) -> serde_json::Value {
    let json_path = paths::version_json(&version_id);
    let parsed = std::fs::read_to_string(&json_path)
        .ok()
        .and_then(|t| serde_json::from_str::<model::VersionJson>(&t).ok());
    let java_major = parsed
        .as_ref()
        .map(|v| v.java_version.as_ref().map(|j| j.major_version).unwrap_or(17))
        .unwrap_or(17);
    let account_ok = auth::active_account().unwrap_or(None).is_some();
    json!({
        "versionInstalled": paths::version_jar(&version_id).exists(),
        "versionJsonOk": parsed.is_some(),
        "javaMajor": java_major,
        "accountOk": account_ok,
        "runtimeInstalled": runtime::is_installed(java_major),
        "runtimeSpecAvailable": runtime::spec_for(java_major, host_arch()).is_some(),
        "runtimeVersion": runtime::installed_version(java_major),
        "runtimeDir": runtime::install_dir(java_major).to_string_lossy(),
    })
}

/// 解析版本 JSON 并装配 LaunchPlan（版本未安装 / JSON 损坏时报错）。
fn build_plan_for(version_id: &str) -> Result<(model::VersionJson, jvmops::LaunchPlan, u32), String> {
    let text = std::fs::read_to_string(paths::version_json(version_id))
        .map_err(|_| format!("版本 {version_id} 未安装（缺少版本 JSON），请先在「下载游戏」页完成下载"))?;
    let version: model::VersionJson =
        serde_json::from_str(&text).map_err(|e| format!("版本 JSON 解析失败：{e}"))?;
    if !paths::version_jar(version_id).exists() {
        return Err(format!("版本 {version_id} 的 client jar 缺失，请重新下载"));
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

    let game_dir = paths::minecraft_dir();
    let natives_root = game_dir.join("natives");
    let plan = jvmops::build_launch_plan(&jvmops::PlanInputs {
        ctx: &ctx,
        version: &version,
        allowed_libs: &allowed,
        libraries_root: &paths::libraries_dir(),
        natives_root: &natives_root,
        assets_root: &paths::assets_dir(),
        versions_root: &paths::versions_dir(),
        instance_dir: &game_dir,
        account: &auth::active_account()
            .map_err(|e| e.to_string())?
            .ok_or("尚未登录，请先在「账户」页添加账号")?,
        memory_mb: 1024,
        java_major: major,
        java_path: None,
        window: None,
    })
    .map_err(|e| format!("启动计划装配失败：{e}"))?;
    Ok((version, plan, major))
}

/// 拉起 JVM 子进程并托管日志/退出（`launch://log` / `launch://exit` 事件）。
///
/// 前置：JVM pack 已安装（Android 内部目录，targetSdk 28 允许 exec）。
/// smoke test 先跑 `java -version` 把 JRE 版本写进控制台，失败时给出明确报错。
fn spawn_game(
    version_id: String,
    plan: jvmops::LaunchPlan,
    major: u32,
    game_pid: Arc<Mutex<Option<u32>>>,
    app: AppHandle,
) -> Result<(), String> {
    use std::io::BufRead;
    use std::process::{Command, Stdio};

    let java = runtime::java_bin(major);
    let jre_home = runtime::install_dir(major);
    let game_dir = paths::minecraft_dir();

    // ---- JVM 动态库搜索路径（必须先于 smoke test 计算，两处共用） ----
    // bionic linker 不认 RUNPATH，bin/java 找 libjli.so 完全依赖 LD_LIBRARY_PATH。
    // 覆盖包内全部可能落点：lib（libjli.so 实测在此）、lib/jli、lib/server、
    // lib/client（arm32 client VM）、natives 目录（游戏 so）。
    let ld_library_path = format!(
        "{}:{}:{}:{}:{}",
        jre_home.join("lib").display(),
        jre_home.join("lib/jli").display(),
        jre_home.join("lib/server").display(),
        jre_home.join("lib/client").display(),
        plan.natives_dir.display()
    );

    // ---- smoke test：JRE 可执行性（与真实启动完全同环境）----
    // 必须带 LD_LIBRARY_PATH/JAVA_HOME：Pojav android-openjdk 的 bin/java 无 RUNPATH，
    // 裸跑必报 CANNOT LINK。自检失败直接终止启动并回显全部输出（可诊断，不静默）。
    let _ = app.emit(
        "launch://log",
        &LaunchLog {
            source: "quartz".into(),
            line: format!("[quartz] JRE {major}：{}", jre_home.display()),
        },
    );
    match Command::new(&java)
        .arg("-version")
        .env("JAVA_HOME", &jre_home)
        .env("LD_LIBRARY_PATH", &ld_library_path)
        .output()
    {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stderr);
            for line in text.lines().filter(|l| !l.trim().is_empty()).take(2) {
                let _ = app.emit(
                    "launch://log",
                    &LaunchLog {
                        source: "quartz".into(),
                        line: format!("[quartz] {line}"),
                    },
                );
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
            return Err(format!(
                "JVM 自检失败（退出码 {}，LD_LIBRARY_PATH={}）：{}",
                out.status.code().unwrap_or(-1),
                ld_library_path,
                if tail.is_empty() { "无输出（进程可能被信号杀死）" } else { &tail }
            ));
        }
        Err(e) => {
            return Err(format!(
                "无法执行 JVM（{e}）。JVM pack 需位于可执行目录：{}",
                jre_home.display()
            ))
        }
    }

    // ---- 组装命令：jvm_args + main_class + game_args ----
    let mut cmd = Command::new(&java);
    cmd.args(&plan.jvm_args)
        .arg(&plan.main_class)
        .args(&plan.game_args)
        .current_dir(&plan.working_dir)
        .env("HOME", &game_dir)
        .env("JAVA_HOME", &jre_home)
        .env("LD_LIBRARY_PATH", &ld_library_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("游戏进程启动失败：{e}"))?;
    let pid = child.id();
    if let Ok(mut slot) = game_pid.lock() {
        *slot = Some(pid);
    }
    {
        let app2 = app.clone();
        let _ = app2.emit(
            "launch://log",
            &LaunchLog {
                source: "quartz".into(),
                line: format!("[quartz] 游戏进程已启动 (pid {pid})，工作目录：{}", plan.working_dir.display()),
            },
        );
    }

    // ---- 日志泵：stdout / stderr -> launch://log ----
    if let Some(stdout) = child.stdout.take() {
        let app2 = app.clone();
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(stdout).lines().map_while(Result::ok) {
                let _ = app2.emit("launch://log", &LaunchLog { source: "stdout".into(), line });
            }
        });
    }
    if let Some(stderr) = child.stderr.take() {
        let app2 = app.clone();
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(stderr).lines().map_while(Result::ok) {
                let _ = app2.emit("launch://log", &LaunchLog { source: "stderr".into(), line });
            }
        });
    }

    // ---- 退出看护：wait -> launch://exit -> 清理 pid 槽位 ----
    // 退出码语义：正常退出 = code；信号死亡 = 128+信号号（137=SIGKILL，134=SIGABRT，
    // 之前恒报 -1 无法区分 linker abort / SIGSEGV / 被系统杀死）。
    let game_pid_exit = game_pid;
    std::thread::spawn(move || {
        let code = child.wait().map(|st| exit_status_code(&st)).unwrap_or(-1);
        if let Ok(mut slot) = game_pid_exit.lock() {
            // stop_game 已 take 时这里已是 None，避免覆盖后续新进程
            if slot.is_some() {
                *slot = None;
            }
        }
        // 常见信号死亡的提示行（帮助用户/开发者定位）
        if code == 137 {
            let _ = app.emit(
                "launch://log",
                &LaunchLog {
                    source: "quartz".into(),
                    line: "[quartz] 退出码 137 = SIGKILL：常见于系统低内存回收（尝试调小内存）或 Android 12+ 幽灵进程限制".into(),
                },
            );
        } else if code == 134 {
            let _ = app.emit(
                "launch://log",
                &LaunchLog {
                    source: "quartz".into(),
                    line: "[quartz] 退出码 134 = SIGABRT：常见于动态链接失败或 JVM fatal error，见上方 stderr 输出".into(),
                },
            );
        }
        let _ = app.emit(
            "launch://exit",
            &LaunchExit { code, version: version_id },
        );
    });
    Ok(())
}

/// ExitStatus -> 退出码：正常退出取 code()；信号死亡取 128+signal。
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

/// 真实启动：版本 JSON -> LaunchPlan -> JVM 子进程（M0 已接入）。
/// JVM pack 缺失时返回 `RUNTIME_MISSING:<major>`，UI 自动触发运行时下载。
#[tauri::command]
fn launch_game(version_id: String, app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    if state.game_pid.lock().map_err(|_| "内部状态锁异常")?.is_some() {
        return Err("已有游戏进程在运行，请先停止后再启动".to_string());
    }
    let (_version, plan, major) = build_plan_for(&version_id)?;
    if !runtime::is_installed(major) {
        return Err(format!("RUNTIME_MISSING:{major}"));
    }
    std::fs::create_dir_all(&plan.working_dir).ok();
    std::fs::create_dir_all(&plan.natives_dir).ok();
    spawn_game(version_id.clone(), plan, major, state.game_pid.clone(), app.clone())?;
    Ok(())
}

/// 终止运行中的游戏进程（SIGKILL）。
#[tauri::command]
fn stop_game(state: State<'_, AppState>) -> Result<bool, String> {
    let pid = state
        .game_pid
        .lock()
        .map_err(|_| "内部状态锁异常")?
        .take();
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .setup(|_app| {
            // 启动即解析真实可写存储并创建标准目录布局（诊断信息供 UI 展示）。
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
            get_permission_state,
            request_storage_permission,
            reinit_storage,
            get_runtime_status,
            download_runtime,
            launch_preflight,
            launch_game,
            stop_game,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
