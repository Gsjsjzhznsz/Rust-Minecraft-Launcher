//! Quartz Tauri 壳层。
//!
//! 架构契约（方案 A）：UI（WebView/JS）只调用本文件注册的 command；
//! command 是 quartz-core 能力的薄封装，不含业务逻辑。
//!
//! 本期命令契约（v3 蓝图：下载与账号彻底解耦）：
//! - `offline_login(name)`        离线登录：MD5("OfflinePlayer:"+name) -> accounts.json
//! - `get_account()`              当前选中账号
//! - `list_versions()`            真实 manifest 版本列表（release 优先）
//! - `download_game(version_id)`  后台 spawn 完整下载管线，
//!                                经 `download://progress` 事件上报 {stage,current,total,percent,file}
//! - `get_game_dir()`             标准游戏根目录绝对路径（UI 常显）
//! - `launch_game(version_id)`    本期占位：下载完成后再接入启动（M0 收尾）

use quartz_core::renderbus::{self, DeviceCaps};
use quartz_core::{auth, host_arch, instance, jvmops, meta, model, paths, platform, RuntimeCtx};
use serde::Serialize;
use serde_json::json;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

/// 下载并发互斥：同一时刻只允许一个下载任务。
#[derive(Default)]
pub struct AppState {
    downloading: Arc<std::sync::Mutex<bool>>,
}

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
    let ctx = RuntimeCtx::new(hal.os().rule_name(), host_arch());
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

/// 本期占位：下载完成后再接入启动（M0 收尾：进程内 JVM + Pojav LWJGL 运行时 pack）。
#[tauri::command]
fn launch_game(version_id: String) -> Result<String, String> {
    let jar = paths::version_jar(&version_id);
    if !jar.exists() {
        return Err(format!("版本 {version_id} 尚未下载，请先在「下载游戏」中完成下载"));
    }
    Ok("下载完成后再接入启动：游戏启动（进程内 JVM + Pojav LWJGL 运行时）将在 M0 收尾阶段接入，当前版本仅提供账号与完整下载管线。".to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .setup(|_app| {
            // 首次运行：创建全部标准目录并放置空 options.txt。
            if let Err(e) = paths::ensure_layout() {
                eprintln!("[quartz] 初始化标准目录布局失败：{e}");
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_meta,
            renderer_advice,
            plan_offline,
            offline_login,
            get_account,
            list_versions,
            download_game,
            get_game_dir,
            launch_game,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
