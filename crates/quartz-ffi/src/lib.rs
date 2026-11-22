//! C-ABI 导出（基础演示层）。
//!
//! 定位：五端绑定的**稳定边界样例**。字符串进出统一 UTF-8 JSON，
//! 内存由 `qz_free` 归还。
//!
//! 生产路线（docs/ARCHITECTURE.md 绑定层）：
//! - Kotlin（Android）/ Swift（iOS+macOS）：UniFFI 自动生成绑定；
//! - ArkTS（鸿蒙）：NAPI-RS；
//! - C#（Windows）：P/Invoke 直接消费本 crate 的 C-ABI。

use quartz_core::renderbus::{self, DeviceCaps};
use std::ffi::{c_char, CStr, CString};
use std::path::PathBuf;

/// 返回 FFI 层版本号（调用方负责用 qz_free 释放）。
#[no_mangle]
pub extern "C" fn qz_version() -> *mut c_char {
    to_cstr(env!("CARGO_PKG_VERSION").to_string())
}

/// 释放由本 crate 返回的字符串（C 调用方按普通函数调用即可）。
///
/// # Safety
/// `ptr` 必须是本 crate 此前返回的指针，且不得重复释放。
#[no_mangle]
pub unsafe extern "C" fn qz_free(ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(CString::from_raw(ptr));
    }
}

/// 离线账号 UUID（Java nameUUIDFromBytes("OfflinePlayer:<name>") 语义）。
#[no_mangle]
pub extern "C" fn qz_offline_uuid(name: *const c_char) -> *mut c_char {
    with_input(name, |name| to_cstr(quartz_core::auth::offline_uuid(name)))
}

/// 渲染器选择。
///
/// 输入 JSON：{"os":"android","arch":"arm64","mc":"1.20.4",
///             "caps":{"vulkan":true,"metal":false,"gles3":true,"desktopGl32":false}}
/// 输出 JSON：{"id":"zink","backend":"vulkan","provides":"profile46"} 或 null。
#[no_mangle]
pub extern "C" fn qz_renderer_select(input: *const c_char) -> *mut c_char {
    with_input(input, |json| {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
            return std::ptr::null_mut();
        };
        let os = v.get("os").and_then(|s| s.as_str()).unwrap_or("android");
        let arch = v.get("arch").and_then(|s| s.as_str()).unwrap_or("arm64");
        let mc = v.get("mc").and_then(|s| s.as_str()).unwrap_or("1.20.4");
        let caps_v = v.get("caps").cloned().unwrap_or_default();
        let caps = DeviceCaps {
            vulkan: caps_v
                .get("vulkan")
                .and_then(|b| b.as_bool())
                .unwrap_or(false),
            metal: caps_v
                .get("metal")
                .and_then(|b| b.as_bool())
                .unwrap_or(false),
            gles3: caps_v
                .get("gles3")
                .and_then(|b| b.as_bool())
                .unwrap_or(false),
            desktop_gl_32: caps_v
                .get("desktopGl32")
                .and_then(|b| b.as_bool())
                .unwrap_or(false),
        };
        let required = renderbus::required_profile(mc);
        match renderbus::select(os, arch, required, &caps) {
            Some(p) => {
                let out = serde_json::json!({
                    "id": p.id,
                    "backend": p.backend,
                    "provides": p.provides,
                });
                to_cstr(out.to_string())
            }
            None => std::ptr::null_mut(),
        }
    })
}

/// 启动计划摘要（演示：离线账号 + 内嵌 1.20.4 样例）。
/// 输入 JSON：{"player":"Steve","memoryMb":2048,"root":"/path/to/root"}
#[no_mangle]
pub extern "C" fn qz_plan_summary(input: *const c_char) -> *mut c_char {
    with_input(input, |json| {
        let v: serde_json::Value = match serde_json::from_str(json) {
            Ok(v) => v,
            Err(_) => return std::ptr::null_mut(),
        };
        let player = v
            .get("player")
            .and_then(|s| s.as_str())
            .unwrap_or("Steve")
            .to_string();
        let memory = v.get("memoryMb").and_then(|m| m.as_u64()).unwrap_or(2048) as u32;
        let root = PathBuf::from(
            v.get("root")
                .and_then(|s| s.as_str())
                .unwrap_or(".quartz-root"),
        );

        let version: quartz_core::model::VersionJson =
            match serde_json::from_str(quartz_core::fixtures::VERSION_1_20_4) {
                Ok(v) => v,
                Err(_) => return std::ptr::null_mut(),
            };
        let os = quartz_core::host_os();
        let ctx = quartz_core::RuntimeCtx::new(os.rule_name(), quartz_core::host_arch());
        let allowed: Vec<&quartz_core::model::Library> = version
            .libraries
            .iter()
            .filter(|l| quartz_core::meta::library_allowed(l, &ctx))
            .collect();
        let account = quartz_core::auth::offline_account(&player);
        let plan = match quartz_core::jvmops::build_launch_plan(&quartz_core::jvmops::PlanInputs {
            ctx: &ctx,
            version: &version,
            allowed_libs: &allowed,
            libraries_root: &root.join("libraries"),
            natives_root: &root.join("natives"),
            assets_root: &root.join("assets"),
            versions_root: &root.join("versions"),
            instance_dir: &root.join("instances").join("demo"),
            account: &account,
            memory_mb: memory,
            java_major: 17,
            java_path: None,
            window: None,
            lwjgl_classes_jar: None,
        }) {
            Ok(p) => p,
            Err(_) => return std::ptr::null_mut(),
        };
        let summary = serde_json::json!({
            "mainClass": plan.main_class,
            "jvmArgs": plan.jvm_args,
            "gameArgCount": plan.game_args.len(),
            "classpathCount": plan.classpath.len(),
            "memoryMb": plan.memory_mb,
        });
        to_cstr(summary.to_string())
    })
}

fn to_cstr(s: String) -> *mut c_char {
    match CString::new(s) {
        Ok(c) => c.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

fn with_input<F>(ptr: *const c_char, f: F) -> *mut c_char
where
    F: FnOnce(&str) -> *mut c_char,
{
    if ptr.is_null() {
        return std::ptr::null_mut();
    }
    let s = unsafe { CStr::from_ptr(ptr) }.to_string_lossy();
    f(&s)
}
