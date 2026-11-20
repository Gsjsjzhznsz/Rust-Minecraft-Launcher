//! Quartz Tauri 壳层。
//!
//! 架构契约（方案 A）：UI（WebView/JS）只调用本文件注册的 command；
//! command 是 quartz-core 能力的薄封装，不含业务逻辑。
//! UI 资源通过 ui-pack 管线热更（docs/UI-PACK.md），核心逻辑随包发版。

use quartz_core::renderbus::{self, DeviceCaps};
use quartz_core::{host_arch, model, meta, jvmops, platform, RuntimeCtx};
use serde_json::json;

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

/// 内嵌版本清单（离线演示；在线版改调 meta::fetch_version_manifest）。
#[tauri::command]
fn versions_offline() -> serde_json::Value {
    let manifest: model::VersionManifest =
        serde_json::from_str(quartz_core::fixtures::VERSION_MANIFEST_SAMPLE)
            .expect("fixture manifest must parse");
    json!({
        "latest": manifest.latest,
        "versions": manifest.versions.iter().take(30).collect::<Vec<_>>(),
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
    let account = quartz_core::auth::offline_account(&player);
    let root = std::path::PathBuf::from(".quartz-root");

    let plan = jvmops::build_launch_plan(&jvmops::PlanInputs {
        ctx: &ctx,
        version: &version,
        allowed_libs: &allowed,
        libraries_root: &root.join("libraries"),
        natives_root: &root.join("natives"),
        assets_root: &root.join("assets"),
        versions_root: &root.join("versions"),
        instance_dir: &root.join("instances").join("demo"),
        account: &account,
        memory_mb,
        java_major: version.java_version.as_ref().map(|j| j.major_version).unwrap_or(17),
        java_path: None,
        window: None,
    })
    .map_err(|e| e.to_string())?;

    serde_json::to_value(&plan).map_err(|e| e.to_string())
}

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            app_meta,
            versions_offline,
            renderer_advice,
            plan_offline,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
