//! Android 游戏启动 + 显示桥接线（v0.1.7「崩溃隔离」重构：游戏 JVM 进程化）。
//!
//! 架构（对标 PojavLauncher 的进程内 JVM 方案，但全部用 Rust 实现，且游戏
//! 运行在独立 ：game 进程中）：
//! 1. 主进程（WebView 启动器 UI）把 SurfaceView 的 Surface 经 Binder 传给
//!    :game 进程的 GameService（Surface 是 Parcelable，跨进程共享同一
//!    BufferQueue，ANativeWindow_fromSurface 在 :game 侧照常工作）；
//! 2. :game 进程 dlopen JVM pack 的 libjvm.so -> JNI_CreateJavaVM（与 ART
//!    并存），跑 net.minecraft.client.main.Main；
//! 3. 游戏 LWJGL 类（gladiolus 官方 jar）System.loadLibrary("pojavexec") 时
//!    加载的是我们 Rust 重构的桥（crates/quartz-bridge，以 libpojavexec.so
//!    落位）；
//! 4. 输入：主进程 SurfaceView/虚拟按键 -> Messenger IPC -> GameService ->
//!    QuartzBridge(ART native) -> 桥 C ABI -> 事件队列 -> LWJGL 回调；
//! 5. 渲染：pojavCreateContext 经渲染层自带 EGL 在 ANativeWindow 上建 GLES
//!    上下文，桌面 GL 语义由渲染层转译。
//!
//! 崩溃语义（v0.1.7 核心）：游戏 JVM 运行在独立进程，SIGSEGV/SIGABRT/
//! System.exit 只终止 :game 进程；主进程经 ServiceConnection binder death
//! 回调感知，恢复竖屏启动器 UI 并上报退出码（finish_game_exit）——崩溃
//! 不再陪葬整个 app。退出码走 MSG_EXIT（干净退出）或 binder death（崩溃）。
//! 退出捕获 SecurityManager（quartz-game-hook.jar）保留：拿到 MC 崩溃码。

#![cfg(target_os = "android")]

use crate::LaunchExit;
use crate::LaunchLog;
use tauri::Emitter;
use jni_sys::{jboolean, jint, JNI_OK, JNI_VERSION_1_8};
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

type EnvPtr = *mut jni_sys::JNIEnv;
type VmPtr = *mut jni_sys::JavaVM;

/// 桥 C ABI（dlopen + dlsym 后的函数指针表）。
pub struct BridgeApi {
    pub init: unsafe extern "C" fn(vm: VmPtr, class: jni_sys::jobject, m_grab: jni_sys::jmethodID, m_clip: jni_sys::jmethodID),
    pub setup_surface: unsafe extern "C" fn(env: EnvPtr, surface: jni_sys::jobject) -> c_int,
    pub release_surface: unsafe extern "C" fn(),
    pub window_ready: unsafe extern "C" fn() -> c_int,
    pub send_cursor_pos: unsafe extern "C" fn(x: f32, y: f32),
    pub send_mouse_button: unsafe extern "C" fn(button: c_int, action: c_int, mods: c_int),
    pub send_key: unsafe extern "C" fn(key: c_int, scancode: c_int, action: c_int, mods: c_int),
    pub send_char: unsafe extern "C" fn(codepoint: c_int, mods: c_int),
    pub send_scroll: unsafe extern "C" fn(dx: f64, dy: f64),
    pub send_screen_size: unsafe extern "C" fn(w: c_int, h: c_int),
    pub is_grabbed: unsafe extern "C" fn() -> c_int,
    pub notify_rt_vm: unsafe extern "C" fn(vm: VmPtr) -> c_int,
}

impl BridgeApi {
    fn load() -> Option<&'static BridgeApi> {
        static API: std::sync::OnceLock<Option<BridgeApi>> = std::sync::OnceLock::new();
        if let Some(cached) = API.get() {
            return cached.as_ref();
        }
        let loaded = (|| -> Option<BridgeApi> {
            // v0.1.10 双副本分裂脑修复：本函数与游戏 JVM 的
            // System.loadLibrary("pojavexec") 必须命中**同一个文件**——
            // bionic 按路径缓存 soinfo，两个路径 = 两份独立全局状态
            // （Surface 挂在壳层副本、pojavInit 在 JVM 副本里等窗口到天
            // 荒地老 —— 设备日志实证：稳定窗口检测通过 + pojavInit 8s
            // 超时并存）。故优先 dlopen natives_dir 副本（java.library.path
            // 首目录，JVM 必命中），runtime_dir 副本仅作兼容兜底。
            let candidates = [
                super::lwjgl::natives_dir().join("libpojavexec.so"),
                super::paths::runtime_dir().join("libpojavexec.so"),
            ];
            let (path, handle) = candidates.iter().find_map(|path| {
                if !path.exists() {
                    return None;
                }
                let cpath = CString::new(path.to_string_lossy().into_owned()).ok()?;
                let handle = unsafe { dlopen(cpath.as_ptr(), RTLD_NOW | RTLD_GLOBAL) };
                if handle.is_null() {
                    eprintln!("[quartz] 显示桥 dlopen 失败：{}", path.display());
                    return None;
                }
                Some((path.clone(), handle))
            })?;
            eprintln!("[quartz] 显示桥已加载：{}", path.display());
            let handle = handle;
            macro_rules! sym {
                ($name:literal, $ty:ty) => {{
                    let cname = CString::new($name).unwrap();
                    let p = unsafe { dlsym(handle, cname.as_ptr()) };
                    if p.is_null() {
                        eprintln!("[quartz] 显示桥符号缺失：{}", $name);
                        return None;
                    }
                    unsafe { std::mem::transmute::<*mut c_void, $ty>(p) }
                }};
            }
            Some(BridgeApi {
                init: sym!("quartz_bridge_init", unsafe extern "C" fn(VmPtr, jni_sys::jobject, jni_sys::jmethodID, jni_sys::jmethodID)),
                setup_surface: sym!("quartz_bridge_setup_surface", unsafe extern "C" fn(EnvPtr, jni_sys::jobject) -> c_int),
                release_surface: sym!("quartz_bridge_release_surface", unsafe extern "C" fn()),
                window_ready: sym!("quartz_bridge_window_ready", unsafe extern "C" fn() -> c_int),
                send_cursor_pos: sym!("quartz_bridge_send_cursor_pos", unsafe extern "C" fn(f32, f32)),
                send_mouse_button: sym!("quartz_bridge_send_mouse_button", unsafe extern "C" fn(c_int, c_int, c_int)),
                send_key: sym!("quartz_bridge_send_key", unsafe extern "C" fn(c_int, c_int, c_int, c_int)),
                send_char: sym!("quartz_bridge_send_char", unsafe extern "C" fn(c_int, c_int)),
                send_scroll: sym!("quartz_bridge_send_scroll", unsafe extern "C" fn(f64, f64)),
                send_screen_size: sym!("quartz_bridge_send_screen_size", unsafe extern "C" fn(c_int, c_int)),
                is_grabbed: sym!("quartz_bridge_is_grabbed", unsafe extern "C" fn() -> c_int),
                notify_rt_vm: sym!("quartz_bridge_notify_rt_vm", unsafe extern "C" fn(VmPtr) -> c_int),
            })
        })();
        let _ = API.set(loaded);
        API.get()?.as_ref()
    }
}

unsafe extern "C" {
    fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlerror() -> *const c_char;
}
const RTLD_NOW: c_int = 2;
const RTLD_GLOBAL: c_int = 0x100;

/// dlopen 失败时取 dlerror 文本（安全封装，空串兑底）。
unsafe fn last_dlerror() -> String {
    let p = dlerror();
    if p.is_null() {
        return "（无 dlerror 详情）".into();
    }
    CStr::from_ptr(p).to_string_lossy().into_owned()
}

/// 显示桥 .so 落位：runtime_dir()/libpojavexec.so + lwjgl3/natives/libpojavexec.so
/// （后者是 OpenJDK System.loadLibrary("pojavexec") 的 java.library.path 首目录，
/// 必须覆盖掉 Pojav APK 提取的旧 C 版 —— gladiolus 发布包 jar↔so 版本错位）。
pub fn ensure_bridge_lib() -> Result<(), String> {
    const BRIDGE_SO: &[u8] = include_bytes!("../bridge/libpojavexec.so");
    let dst1 = super::paths::runtime_dir().join("libpojavexec.so");
    let dst2 = super::lwjgl::natives_dir().join("libpojavexec.so");
    for dst in [dst1, dst2] {
        let stale = std::fs::metadata(&dst)
            .map(|m| m.len() != BRIDGE_SO.len() as u64)
            .unwrap_or(true);
        if stale {
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败：{e}"))?;
            }
            std::fs::write(&dst, BRIDGE_SO).map_err(|e| format!("写入显示桥失败：{e}"))?;
        }
    }
    Ok(())
}

/// 游戏退出捕获 hook jar 落位（自研 SecurityManager：checkExit 否决
/// System.exit/halt 并抛 ExitSignal，让崩溃/退出回到启动器而非杀死进程）。
/// jar 由 scripts/build-android.sh 在构建期用 javac --release 17 编译。
pub fn ensure_hook_jar() -> Result<(), String> {
    const HOOK_JAR: &[u8] = include_bytes!("../bridge/quartz-game-hook.jar");
    let dst = super::paths::runtime_dir().join("quartz-game-hook.jar");
    let stale = std::fs::metadata(&dst)
        .map(|m| m.len() != HOOK_JAR.len() as u64)
        .unwrap_or(true);
    if stale {
        std::fs::write(&dst, HOOK_JAR).map_err(|e| format!("写入退出捕获 hook 失败：{e}"))?;
    }
    Ok(())
}

/// zink 专用：以带正确 SONAME 的 libcutils shim 覆盖 pack 缺陷库。
///
/// 根因（实机 2026-11）：IronizedZink v1.1.0 的 libcutils.so 无 DT_SONAME，
/// bionic dlopen 按完整路径注册 soinfo；libgallium_dri / libEGL_mesa 的
/// DT_NEEDED "libcutils.so" 按名解析在 clns 命名空间必败（两次预载 pass
/// 均失败，EGL 起不来）。shim 由交叉 gcc 编译（SONAME=libcutils.so、
/// 无 DT_NEEDED、property_* 转发系统 bionic、atrace no-op），符号集与
/// pack 原版完全对齐。幂等：size 一致即跳过。
pub fn ensure_zink_cutils_shim() -> Result<(), String> {
    const SHIM: &[u8] = include_bytes!("../bridge/libqzcutils.so");
    let dst = crate::renderers::install_dir("zink").join("libcutils.so");
    if !dst.parent().map(|p| p.exists()).unwrap_or(false) {
        return Ok(()); // zink 未安装（不选 zink 渲染时目录不存在，跳过）
    }
    let stale = std::fs::metadata(&dst)
        .map(|m| m.len() != SHIM.len() as u64)
        .unwrap_or(true);
    if stale {
        std::fs::write(&dst, SHIM).map_err(|e| format!("写入 libcutils shim 失败：{e}"))?;
        eprintln!("[quartz] zink libcutils shim 已落位（SONAME 修复覆盖 pack 缺陷库）");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// ART 侧桥接初始化（QuartzBridge 类注入）
// ---------------------------------------------------------------------------

unsafe fn art_vm_raw() -> Option<VmPtr> {
    let ctx = ndk_context::android_context();
    let vm = ctx.vm();
    if vm.is_null() {
        None
    } else {
        Some(vm as VmPtr)
    }
}

/// dlopen 显示桥并注入 ART JavaVM + QuartzBridge 全局引用与回调方法 ID。
pub fn ensure_bridge_loaded() -> Result<(), String> {
    let Some(api) = (unsafe { BridgeApi::load() }) else {
        return Err("显示桥装载失败".into());
    };
    static INITED: AtomicBool = AtomicBool::new(false);
    if INITED.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    let Some(vm) = (unsafe { art_vm_raw() }) else {
        return Err("ART JavaVM 不可用".into());
    };
    crate::with_jni(|env, ctx| {
        use jni::objects::JValue;
        unsafe {
            let loader = env
                .call_method(ctx, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])
                .ok()?
                .l()
                .ok()?;
            let name = env.new_string("com.quartz.launcher.QuartzBridge").ok()?;
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
            let gref = env.new_global_ref(&cls_obj).ok()?;
            let cls: jni::objects::JClass = cls_obj.into();
            let m_grab = env
                .get_static_method_id(&cls, "onGrabStateChanged", "(Z)V")
                .ok()?;
            let m_clip = env
                .get_static_method_id(
                    &cls,
                    "accessAndroidClipboard",
                    "(ILjava/lang/String;)Ljava/lang/String;",
                )
                .ok()?;
            let m_grab_raw: jni_sys::jmethodID = std::mem::transmute(m_grab);
            let m_clip_raw: jni_sys::jmethodID = std::mem::transmute(m_clip);
            let cls_raw = gref.as_obj().as_raw();
            std::mem::forget(gref); // 全局引用随进程存活，不释放
            (api.init)(vm, cls_raw, m_grab_raw, m_clip_raw);
            Some(())
        }
    })
    .ok_or_else(|| "QuartzBridge 类注入失败".to_string())
}

// ---------------------------------------------------------------------------
// Kotlin 壳层静态方法（Surface 视图显隐 / 方向 / 服务绑定）
// ---------------------------------------------------------------------------

fn call_activity_static(method: &str, sig: &str, args: &[JValueOwned]) -> bool {
    use jni::objects::JClass;
    use jni::objects::JValue;
    crate::with_jni(|env, ctx| {
        unsafe {
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
            let values: Vec<JValue> = args.iter().map(|v| v.borrow()).collect();
            env.call_static_method(&cls, method, sig, &values).ok()?;
            Some(())
        }
    })
    .is_some()
}

/// with_jni 便捷传参（暂存值，借用转发）。
enum JValueOwned {
    Bool(bool),
    Int(i32),
}

impl JValueOwned {
    fn borrow(&self) -> jni::objects::JValue {
        match self {
            JValueOwned::Bool(b) => jni::objects::JValue::Bool(u8::from(*b)),
            JValueOwned::Int(i) => jni::objects::JValue::Int(*i),
        }
    }
}

/// 显示游戏 Surface（Kotlin 创建 SurfaceView + 切横屏）。
pub fn show_game_surface() {
    if !call_activity_static("showGameSurfaceStatic", "()V", &[]) {
        eprintln!("[quartz] showGameSurfaceStatic 调用失败");
    }
}

/// 隐藏游戏 Surface（Kotlin 移除 SurfaceView + 恢复竖屏）。
pub fn hide_game_surface() {
    if !call_activity_static("hideGameSurfaceStatic", "()V", &[]) {
        eprintln!("[quartz] hideGameSurfaceStatic 调用失败");
    }
}

/// 带一个 String 参数的 MainActivity 静态方法调用（startGameServiceStatic）。
fn call_activity_string(method: &str, sig: &str, arg: &str) -> bool {
    use jni::objects::{JClass, JValue};
    crate::with_jni(|env, ctx| {
        unsafe {
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
            let jarg = env.new_string(arg).ok()?;
            env.call_static_method(&cls, method, sig, &[JValue::Object(&jarg)]).ok()?;
            Some(())
        }
    })
    .is_some()
}

// ---------------------------------------------------------------------------
// ART 侧 QuartzBridge native 转发（Kotlin external fun 绑到本库）
// ---------------------------------------------------------------------------

#[no_mangle]
pub unsafe extern "C" fn Java_com_quartz_launcher_QuartzBridge_setupSurface(
    env: *mut jni_sys::JNIEnv,
    _class: jni_sys::jclass,
    surface: jni_sys::jobject,
) -> jint {
    match BridgeApi::load() {
        Some(api) => (api.setup_surface)(env, surface),
        None => 0,
    }
}

#[no_mangle]
pub unsafe extern "C" fn Java_com_quartz_launcher_QuartzBridge_releaseSurface(
    _env: *mut jni_sys::JNIEnv,
    _class: jni_sys::jclass,
) {
    if let Some(api) = BridgeApi::load() {
        (api.release_surface)();
    }
}

#[no_mangle]
pub unsafe extern "C" fn Java_com_quartz_launcher_QuartzBridge_sendCursorPos(
    _env: *mut jni_sys::JNIEnv,
    _class: jni_sys::jclass,
    x: f32,
    y: f32,
) {
    if let Some(api) = BridgeApi::load() {
        (api.send_cursor_pos)(x, y);
    }
}

#[no_mangle]
pub unsafe extern "C" fn Java_com_quartz_launcher_QuartzBridge_sendMouseButton(
    _env: *mut jni_sys::JNIEnv,
    _class: jni_sys::jclass,
    button: jint,
    action: jint,
    mods: jint,
) {
    if let Some(api) = BridgeApi::load() {
        (api.send_mouse_button)(button, action, mods);
    }
}

#[no_mangle]
pub unsafe extern "C" fn Java_com_quartz_launcher_QuartzBridge_sendKey(
    _env: *mut jni_sys::JNIEnv,
    _class: jni_sys::jclass,
    key: jint,
    scancode: jint,
    action: jint,
    mods: jint,
) {
    if let Some(api) = BridgeApi::load() {
        (api.send_key)(key, scancode, action, mods);
    }
}

#[no_mangle]
pub unsafe extern "C" fn Java_com_quartz_launcher_QuartzBridge_sendChar(
    _env: *mut jni_sys::JNIEnv,
    _class: jni_sys::jclass,
    codepoint: jint,
    mods: jint,
) {
    if let Some(api) = BridgeApi::load() {
        (api.send_char)(codepoint, mods);
    }
}

#[no_mangle]
pub unsafe extern "C" fn Java_com_quartz_launcher_QuartzBridge_sendScroll(
    _env: *mut jni_sys::JNIEnv,
    _class: jni_sys::jclass,
    dx: f64,
    dy: f64,
) {
    if let Some(api) = BridgeApi::load() {
        (api.send_scroll)(dx, dy);
    }
}

#[no_mangle]
pub unsafe extern "C" fn Java_com_quartz_launcher_QuartzBridge_sendScreenSize(
    _env: *mut jni_sys::JNIEnv,
    _class: jni_sys::jclass,
    w: jint,
    h: jint,
) {
    if let Some(api) = BridgeApi::load() {
        (api.send_screen_size)(w, h);
    }
}

#[no_mangle]
pub unsafe extern "C" fn Java_com_quartz_launcher_QuartzBridge_isGrabbed(
    _env: *mut jni_sys::JNIEnv,
    _class: jni_sys::jclass,
) -> jboolean {
    match BridgeApi::load() {
        Some(api) => (api.is_grabbed)() as jboolean,
        None => 0,
    }
}

// ---------------------------------------------------------------------------
// :game 独立进程启动（v0.1.7 崩溃隔离）
// ---------------------------------------------------------------------------

/// 游戏运行中标记（launch 发起 -> 退出回调确认）。
static GAME_RUNNING: AtomicBool = AtomicBool::new(false);
/// 本次启动是否已上报退出（MSG_EXIT 与 binder death 双路径幂等）。
static EXIT_EMITTED: AtomicBool = AtomicBool::new(false);
/// 用户主动停止标记（区分崩溃 134 与停止 137）。
static STOP_REQUESTED: AtomicBool = AtomicBool::new(false);
/// 日志 tail 线程活跃标记。
static TAIL_ACTIVE: AtomicBool = AtomicBool::new(false);
/// 主进程 AppHandle（GameHost 回调 emit 事件用；launch 时注入）。
static APP_HANDLE: OnceLock<tauri::AppHandle> = OnceLock::new();
/// 当前启动版本号（退出事件负载）。
static STATE_VERSION: Mutex<String> = Mutex::new(String::new());
/// 当前启动日志文件句柄（qlog_emit 写入目标）。
static STATE_LOG_FILE: Mutex<Option<Arc<Mutex<Option<std::fs::File>>>>> = Mutex::new(None);
/// 当前游戏进程日志路径（:game stdout/stderr 直写；退出摘要 + 看门狗判定用）。
static STATE_GAME_LOG_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);
/// 当前启动器侧日志路径（退出摘要第二段）。
static STATE_MAIN_LOG_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);
/// tail 线程已读偏移（退出时同步排干剩余行，避免 250ms 轮询竞态丢掉最后的
/// 错误详情 —— 252 盲盒的根因之一：nativeRunGame 的 eprintln 与 MSG_EXIT
/// 几乎同时写入，tail 线程来不及读）。
static GAME_LOG_POS: Mutex<u64> = Mutex::new(0);

pub fn game_running() -> bool {
    GAME_RUNNING.load(Ordering::SeqCst)
}

/// 读日志尾部（最多 max_lines 行，单行截断 400 字符）。
fn log_tail_text(path: Option<&Path>, max_lines: usize) -> Option<String> {
    let path = path?;
    let text = std::fs::read_to_string(path).ok()?;
    let lines: Vec<String> = text
        .lines()
        .rev()
        .take(max_lines)
        .map(|l| {
            if l.chars().count() > 400 {
                format!("{}…", l.chars().take(400).collect::<String>())
            } else {
                l.to_string()
            }
        })
        .collect();
    if lines.is_empty() {
        return None;
    }
    Some(
        lines
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// 退出前把游戏日志尚未 tail 到的行同步写进控制台流（防秒退竞态丢错误详情）。
fn drain_game_log_to_console() {
    let path = match STATE_GAME_LOG_PATH.lock().ok().and_then(|g| g.clone()) {
        Some(p) => p,
        None => return,
    };
    let pos = GAME_LOG_POS.lock().ok().map(|p| *p).unwrap_or(0);
    let meta = match std::fs::metadata(&path) {
        Ok(m) => m,
        Err(_) => return,
    };
    if meta.len() <= pos {
        return;
    }
    let Ok(mut f) = std::fs::File::open(&path) else {
        return;
    };
    use std::io::{Read, Seek, SeekFrom};
    if f.seek(SeekFrom::Start(pos)).is_err() {
        return;
    }
    let mut buf = Vec::new();
    if f.read_to_end(&mut buf).is_err() {
        return;
    }
    if let Ok(mut p) = GAME_LOG_POS.lock() {
        *p = pos + buf.len() as u64;
    }
    let text = String::from_utf8_lossy(&buf);
    let lines: Vec<&str> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect();
    if let Some(app) = APP_HANDLE.get() {
        if let Ok(g) = STATE_LOG_FILE.lock() {
            if let Some(lf) = g.as_ref() {
                // 最多补 100 行，且保持原顺序
                for line in lines.iter().rev().take(100).collect::<Vec<_>>().into_iter().rev() {
                    qlog_emit(app, lf, "stdout", line);
                }
            }
        }
    }
}

/// 统一退出上报：隐藏游戏层 + 恢复竖屏 + emit launch://exit（附日志尾部）。幂等。
/// 调用方：GameHost.gameExited（:game 正常结束）/ gameProcessGone（崩溃/停止）/ 看门狗。
pub fn finish_game_exit(code: i32) {
    if EXIT_EMITTED.swap(true, Ordering::SeqCst) {
        return;
    }
    // 同步排干游戏日志剩余行：nativeRunGame 的错误详情（如 JNI_CreateJavaVM
    // 失败原因）与 MSG_EXIT 同窗期落盘，tail 线程轮询赶不上 —— 不排干就是盲盒。
    drain_game_log_to_console();
    GAME_RUNNING.store(false, Ordering::SeqCst);
    TAIL_ACTIVE.store(false, Ordering::SeqCst);
    hide_game_surface();
    if let Some(app) = APP_HANDLE.get() {
        let version = STATE_VERSION.lock().ok().map(|g| g.clone()).unwrap_or_default();
        // 崩溃证据直达 UI：游戏进程日志尾部 + 启动器日志尾部（用户下次反馈可直接粘贴）。
        let game_tail = log_tail_text(
            STATE_GAME_LOG_PATH.lock().ok().and_then(|g| g.clone()).as_deref(),
            40,
        );
        let main_tail = log_tail_text(
            STATE_MAIN_LOG_PATH.lock().ok().and_then(|g| g.clone()).as_deref(),
            16,
        );
        let mut tail = String::new();
        match game_tail {
            Some(t) => tail.push_str(&format!("--- 游戏进程日志尾部 ---\n{t}")),
            None => tail.push_str("--- 游戏进程日志为空（进程未产出任何输出） ---"),
        }
        if let Some(t) = main_tail {
            tail.push_str(&format!("\n\n--- 启动器日志尾部 ---\n{t}"));
        }
        let _ = app.emit(
            "launch://exit",
            &LaunchExit {
                code,
                version,
                log_file: None,
                log_tail: if tail.is_empty() { None } else { Some(tail) },
            },
        );
    }
}

fn append_state_log(line: &str) {
    if let Some(app) = APP_HANDLE.get() {
        if let Ok(g) = STATE_LOG_FILE.lock() {
            if let Some(lf) = g.as_ref() {
                qlog_emit(app, lf, "quartz", line);
            }
        }
    }
}

/// 停止游戏：向 :game 进程服务发 MSG_STOP（服务自杀进程），
/// UI 恢复由 binder death 回调（gameProcessGone）完成。
pub fn stop_game_process() {
    STOP_REQUESTED.store(true, Ordering::SeqCst);
    let _ = call_activity_static("stopGameServiceStatic", "()V", &[]);
}

/// 致命信号拦截（:game 进程）：SIGABRT/SIGBUS/SIGFPE/SIGILL 到达时写一行证据到
/// 日志（fd 2 已重定向到游戏日志），随后 _exit(128+sig)。
/// 目的：让 :game 进程以“正常退出”姿态死亡 —— 不经 debuggerd/ams_crash 通道，
/// 部分 ROM（MIUI 等）对崩溃进程会整组杀掉应用，此拦截切断该路径；
/// 启动器 UI 经 binder death 回调可靠恢复。
/// 注意：故意不拦截 SIGSEGV —— HotSpot 依赖它做隐式空检查；JVM 致命错误写完
/// hs_err 后走 abort()，仍会落入本拦截。
extern "C" fn quartz_on_fatal_signal(sig: c_int) {
    const MSG: &str = "==== [quartz] :game fatal signal (intercepted, clean exit) ====\n";
    unsafe {
        libc::write(2, MSG.as_ptr() as *const c_void, MSG.len());
        libc::_exit(128 + sig);
    }
}

fn install_fatal_signal_handlers() {
    unsafe {
        for sig in [libc::SIGABRT, libc::SIGBUS, libc::SIGFPE, libc::SIGILL] {
            let mut act: libc::sigaction = std::mem::zeroed();
            act.sa_sigaction = quartz_on_fatal_signal as usize;
            act.sa_flags = libc::SA_RESTART;
            libc::sigaction(sig, &act, ptr::null_mut());
        }
    }
}

/// 主进程回调：:game 进程正常结束（GameService 发 MSG_EXIT 后自杀）。
#[no_mangle]
pub unsafe extern "C" fn Java_com_quartz_launcher_GameHost_gameExited(
    _env: *mut jni_sys::JNIEnv,
    _obj: jni_sys::jobject,
    code: jint,
) {
    append_state_log(&format!("[quartz] 游戏进程退出，代码 {code}"));
    finish_game_exit(code as i32);
}

/// 主进程回调：:game 进程死亡（binder death：原生崩溃 / killProcess）。
#[no_mangle]
pub unsafe extern "C" fn Java_com_quartz_launcher_GameHost_gameProcessGone(
    _env: *mut jni_sys::JNIEnv,
    _obj: jni_sys::jobject,
) {
    let stopped = STOP_REQUESTED.load(Ordering::SeqCst);
    if !EXIT_EMITTED.load(Ordering::SeqCst) {
        append_state_log(if stopped {
            "[quartz] 游戏进程已停止"
        } else {
            "[quartz] 游戏进程异常终止（原生崩溃），已自动恢复启动器界面"
        });
    }
    finish_game_exit(if stopped { 137 } else { 134 });
    // 崩溃证据补充采集（异步，不拖慢 UI 恢复）：v0.1.8 起致命信号已在 :game
    // 侧拦截为干净退出（通常无 debuggerd 记录），此处兜底抓取 crash 缓冲，
    // JVM 写 hs_err 前的 abort / 拦截器未覆盖的极端路径仍可留痕。
    if !stopped {
        std::thread::spawn(|| {
            let out = std::process::Command::new("logcat")
                .args(["-d", "-b", "crash", "-t", "200"])
                .output();
            if let Ok(o) = out {
                let text = String::from_utf8_lossy(&o.stdout);
                // 40 行才能覆盖异常头（FATAL EXCEPTION / 异常类名 / Process 行）：
                // 只取 12 行会把栈中部留下、根因行丢掉（设备日志曾验证此坑）。
                let recent: Vec<&str> = text.lines().rev().take(40).collect();
                for line in recent.into_iter().rev() {
                    if line.trim().is_empty() {
                        continue;
                    }
                    append_state_log(&format!("[logcat-crash] {line}"));
                }
            }
        });
    }
}

/// 主进程回调：:game 进程上报鼠标 grab 状态（GameService 转发桥回调）。
#[no_mangle]
pub unsafe extern "C" fn Java_com_quartz_launcher_GameHost_grabChanged(
    _env: *mut jni_sys::JNIEnv,
    _obj: jni_sys::jobject,
    grabbed: jboolean,
) {
    // grab 状态的 UI 反馈由 Kotlin 侧 GameHost Handler 直接处理，
    // 这里仅留 JNI 挂点（未来 Rust 侧需要感知时使用）。
    let _ = grabbed;
}

fn qlog_emit(app: &tauri::AppHandle, log_file: &Arc<Mutex<Option<std::fs::File>>>, source: &str, line: &str) {
    let _ = app.emit("launch://log", &LaunchLog { source: source.into(), line: line.into() });
    if let Ok(mut guard) = log_file.lock() {
        if let Some(f) = guard.as_mut() {
            use std::io::Write;
            let _ = writeln!(f, "[{source}] {line}");
            let _ = f.flush();
        }
    }
}

/// 启动规格（主进程装配 -> spec 文件 -> :game 进程读取执行）。
/// game_root/runtime_dir（v0.1.8）：:game 是独立进程，paths 模块的进程级注入
/// static 在该进程不存在 —— 必须随 spec 传入并在 run_game_from_spec 顶部
/// 重新注入，否则 tagfix/hook jar/tmp/java.library.path/渲染层全部解析到
/// 错误回退路径（v0.1.7 游戏秒崩的根因）。
#[derive(serde::Serialize, serde::Deserialize)]
pub struct GameSpec {
    pub version_id: String,
    pub main_class: String,
    pub workdir: String,
    pub jre_home: String,
    /// 原始 JVM 参数（-cp 对；tmp/library.path 相关项在 :game 侧重新注入）
    pub jvm_args: Vec<String>,
    pub game_args: Vec<String>,
    /// Mojang 版本 natives 目录（含在 java.library.path 尾部）
    pub natives_dir: String,
    pub log_path: String,
    /// 主进程解析的游戏根目录（含 .minecraft / logs / accounts.json）
    pub game_root: String,
    /// 主进程解析的可 exec 运行时目录（JVM pack / 渲染层 / LWJGL / 桥 / hook jar）
    pub runtime_dir: String,
}

/// :game 独立进程启动主流程（v0.1.7 崩溃隔离重构）。
/// 主进程侧只做：装配 spec -> 落盘 -> 显示游戏视图 -> 绑定 :game 服务；
/// JVM 的创建与运行全部在 :game 进程（GameService.nativeRunGame）。
/// 前置：JVM pack / LWJGL pack / 渲染层 / 显示桥文件均已就位。
pub fn launch_via_game_process(
    version_id: String,
    plan: crate::jvmops::LaunchPlan,
    major: u32,
    app: tauri::AppHandle,
    log_file: Arc<Mutex<Option<std::fs::File>>>,
    log_path: &Path,
) -> Result<(), String> {
    let _ = APP_HANDLE.set(app.clone());
    EXIT_EMITTED.store(false, Ordering::SeqCst);
    STOP_REQUESTED.store(false, Ordering::SeqCst);
    TAIL_ACTIVE.store(true, Ordering::SeqCst);
    GAME_RUNNING.store(true, Ordering::SeqCst);
    if let Ok(mut g) = STATE_VERSION.lock() {
        *g = version_id.clone();
    }
    if let Ok(mut g) = STATE_LOG_FILE.lock() {
        *g = Some(log_file.clone());
    }

    let jre_home = crate::runtime::install_dir(major);
    // 游戏进程输出独立落盘（与启动器日志分离）：tail 只跟游戏输出，
    // 看门狗以“游戏日志是否产出内容”判定 :game 是否真的跑起来了。
    let game_log_path = log_path
        .parent()
        .map(|dir| {
            dir.join(format!(
                "game-{}.log",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0)
            ))
        })
        .unwrap_or_else(|| log_path.to_path_buf());
    if let Ok(mut g) = STATE_GAME_LOG_PATH.lock() {
        *g = Some(game_log_path.clone());
    }
    if let Ok(mut g) = STATE_MAIN_LOG_PATH.lock() {
        *g = Some(log_path.to_path_buf());
    }
    let spec = GameSpec {
        version_id: version_id.clone(),
        main_class: plan.main_class.clone(),
        workdir: plan.working_dir.to_string_lossy().into_owned(),
        jre_home: jre_home.to_string_lossy().into_owned(),
        jvm_args: plan.jvm_args.clone(),
        game_args: plan.game_args.clone(),
        natives_dir: plan.natives_dir.to_string_lossy().into_owned(),
        log_path: game_log_path.to_string_lossy().into_owned(),
        game_root: crate::paths::game_root().to_string_lossy().into_owned(),
        runtime_dir: crate::paths::runtime_dir().to_string_lossy().into_owned(),
    };

    // spec 落盘（内部目录，:game 进程同 uid 可读）
    let spec_path = crate::paths::runtime_dir().join("game-spec.json");
    let json = serde_json::to_string(&spec).map_err(|e| format!("spec 序列化失败：{e}"))?;
    std::fs::write(&spec_path, json).map_err(|e| format!("spec 写入失败：{e}"))?;

    qlog_emit(
        &app,
        &log_file,
        "quartz",
        &format!(
            "[quartz] 启动模式：:game 独立进程 JVM + Rust 显示桥（崩溃不影响启动器 UI）；游戏输出：{}",
            game_log_path.display()
        ),
    );

    // 显示游戏视图（横屏 SurfaceView）+ 绑定 :game 服务
    show_game_surface();
    let ok = call_activity_string(
        "startGameServiceStatic",
        "(Ljava/lang/String;)V",
        &spec_path.to_string_lossy(),
    );
    if !ok {
        TAIL_ACTIVE.store(false, Ordering::SeqCst);
        GAME_RUNNING.store(false, Ordering::SeqCst);
        hide_game_surface();
        return Err("绑定 :game 游戏进程服务失败".into());
    }

    // tail 线程：:game 进程把 stdout/stderr 直写游戏日志文件，主进程跟踪新行
    // -> launch://log（WebView 实时控制台）。退出回调会把 TAIL_ACTIVE 置否。
    let tail_app = app.clone();
    let tail_lf = log_file.clone();
    let tail_path = game_log_path.clone();
    std::thread::spawn(move || {
        use std::io::{Read, Seek, SeekFrom};
        let mut pos: u64 = std::fs::metadata(&tail_path).map(|m| m.len()).unwrap_or(0);
        // 上限 40 分钟（游戏长跑时 tail 静默停止，日志文件仍在实时落盘）
        for _ in 0..(40 * 60 * 4) {
            if !TAIL_ACTIVE.load(Ordering::SeqCst) {
                break;
            }
            if let Ok(meta) = std::fs::metadata(&tail_path) {
                if meta.len() > pos {
                    if let Ok(mut f) = std::fs::File::open(&tail_path) {
                        let _ = f.seek(SeekFrom::Start(pos));
                        let mut buf = Vec::new();
                        if f.read_to_end(&mut buf).is_ok() {
                            pos += buf.len() as u64;
                            if let Ok(mut p) = GAME_LOG_POS.lock() {
                                *p = pos;
                            }
                            let text = String::from_utf8_lossy(&buf);
                            for line in text.lines() {
                                if line.trim().is_empty() {
                                    continue;
                                }
                                qlog_emit(&tail_app, &tail_lf, "stdout", line);
                            }
                        }
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
    });

    // 看门狗：正常路径下 :game 进程在 MSG_INIT 后 1 秒内就会向游戏日志写入
    // 首行输出（日志重定向先于一切）。20 秒仍空 = 服务没起来 / spec 未送达 /
    // 进程秒死（失败于 loadLibrary 等极早期阶段，binder 死亡都来不及触发）——
    // 强制结束并恢复启动器 UI，避免黑屏 Surface 永久悬挂。
    std::thread::spawn(move || {
        let game_log = game_log_path.clone();
        let stop_app = app.clone();
        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(500));
            if !TAIL_ACTIVE.load(Ordering::SeqCst) {
                return; // 已退出（MSG_EXIT / binder death），看门狗退位
            }
            if std::fs::metadata(&game_log)
                .map(|m| m.len() > 0)
                .unwrap_or(false)
            {
                return; // 游戏进程已产出输出，启动正常
            }
        }
        if EXIT_EMITTED.load(Ordering::SeqCst) {
            return;
        }
        qlog_emit(
            &stop_app,
            &log_file,
            "quartz",
            "[quartz] 看门狗：游戏进程 20 秒无任何输出（服务未启动 / spec 未送达 / 进程秒死），强制恢复启动器",
        );
        let _ = call_activity_static("stopGameServiceStatic", "()V", &[]);
        finish_game_exit(253);
    });
    Ok(())
}

// ---------------------------------------------------------------------------
// :game 进程执行体（GameService.nativeRunGame）
// ---------------------------------------------------------------------------

/// 显示桥注入（:game 进程版）：以给定 env/ctx（GameService 实例）注册
/// grab/剪贴板回调。与 ensure_bridge_loaded 逻辑一致，但不依赖 ndk-context
///（:game 进程没有 Tauri 胶水，ndk_context 为空）。
unsafe fn bridge_init_with(env: EnvPtr, ctx: jni_sys::jobject) -> Result<(), String> {
    let api = BridgeApi::load().ok_or("显示桥装载失败")?;
    static INITED: AtomicBool = AtomicBool::new(false);
    if INITED.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    let tbl = &**env;
    // 防御：清上游遗留挂起异常（同 with_jni 教训）
    if (tbl.ExceptionCheck.unwrap())(env) != 0 {
        (tbl.ExceptionClear.unwrap())(env);
    }
    // ctx.getClassLoader()
    let ctx_cls = (tbl.GetObjectClass.unwrap())(env, ctx);
    let get_loader_mid = (tbl.GetMethodID.unwrap())(
        env,
        ctx_cls,
        c"getClassLoader".as_ptr(),
        c"()Ljava/lang/ClassLoader;".as_ptr(),
    );
    let loader = (tbl.CallObjectMethodA.unwrap())(env, ctx, get_loader_mid, ptr::null());
    if loader.is_null() {
        (tbl.ExceptionClear.unwrap())(env);
        return Err("ClassLoader 获取失败".into());
    }
    // loader.loadClass("com.quartz.launcher.QuartzBridge")
    let str_cls = (tbl.FindClass.unwrap())(env, c"java/lang/String".as_ptr());
    if str_cls.is_null() {
        (tbl.ExceptionClear.unwrap())(env);
        return Err("java/lang/String 未找到".into());
    }
    let name = (tbl.NewStringUTF.unwrap())(env, c"com.quartz.launcher.QuartzBridge".as_ptr());
    let cls_cls = (tbl.GetObjectClass.unwrap())(env, loader);
    let load_mid = (tbl.GetMethodID.unwrap())(
        env,
        cls_cls,
        c"loadClass".as_ptr(),
        c"(Ljava/lang/String;)Ljava/lang/Class;".as_ptr(),
    );
    let jargs = jni_sys::jvalue { l: name };
    let cls_obj = (tbl.CallObjectMethodA.unwrap())(env, loader, load_mid, &jargs);
    if cls_obj.is_null() {
        (tbl.ExceptionClear.unwrap())(env);
        return Err("QuartzBridge 类加载失败".into());
    }
    let gref = (tbl.NewGlobalRef.unwrap())(env, cls_obj);
    if gref.is_null() {
        (tbl.ExceptionClear.unwrap())(env);
        return Err("全局引用创建失败".into());
    }
    let m_grab = (tbl.GetStaticMethodID.unwrap())(
        env,
        gref,
        c"onGrabStateChanged".as_ptr(),
        c"(Z)V".as_ptr(),
    );
    let m_clip = (tbl.GetStaticMethodID.unwrap())(
        env,
        gref,
        c"accessAndroidClipboard".as_ptr(),
        c"(ILjava/lang/String;)Ljava/lang/String;".as_ptr(),
    );
    if m_grab.is_null() || m_clip.is_null() {
        (tbl.ExceptionClear.unwrap())(env);
        return Err("回调方法注入失败".into());
    }
    // ART JavaVM（从当前 env 取，不依赖 ndk-context）
    let mut vm: VmPtr = ptr::null_mut();
    let rc = (tbl.GetJavaVM.unwrap())(env, &mut vm);
    if rc != JNI_OK || vm.is_null() {
        return Err("ART JavaVM 获取失败".into());
    }
    (api.init)(vm, gref, m_grab, m_clip);
    eprintln!("[quartz] 显示桥已注入 :game 进程 ART 侧");
    Ok(())
}

/// 调用 QuartzBridge.latestSurface()（静态，返回最近收到的有效 Surface 或 null）。
unsafe fn quartz_bridge_latest_surface(env: EnvPtr, ctx: jni_sys::jobject) -> Option<jni_sys::jobject> {
    let tbl = &**env;
    if (tbl.ExceptionCheck.unwrap())(env) != 0 {
        (tbl.ExceptionClear.unwrap())(env);
    }
    let ctx_cls = (tbl.GetObjectClass.unwrap())(env, ctx);
    let get_loader_mid = (tbl.GetMethodID.unwrap())(
        env,
        ctx_cls,
        c"getClassLoader".as_ptr(),
        c"()Ljava/lang/ClassLoader;".as_ptr(),
    );
    let loader = (tbl.CallObjectMethodA.unwrap())(env, ctx, get_loader_mid, ptr::null());
    if loader.is_null() {
        (tbl.ExceptionClear.unwrap())(env);
        return None;
    }
    let str_cls = (tbl.FindClass.unwrap())(env, c"java/lang/String".as_ptr());
    if str_cls.is_null() {
        (tbl.ExceptionClear.unwrap())(env);
        return None;
    }
    let name = (tbl.NewStringUTF.unwrap())(env, c"com.quartz.launcher.QuartzBridge".as_ptr());
    let cls_cls = (tbl.GetObjectClass.unwrap())(env, loader);
    let load_mid = (tbl.GetMethodID.unwrap())(
        env,
        cls_cls,
        c"loadClass".as_ptr(),
        c"(Ljava/lang/String;)Ljava/lang/Class;".as_ptr(),
    );
    let jargs = jni_sys::jvalue { l: name };
    let cls_obj = (tbl.CallObjectMethodA.unwrap())(env, loader, load_mid, &jargs);
    if cls_obj.is_null() {
        (tbl.ExceptionClear.unwrap())(env);
        return None;
    }
    let mid = (tbl.GetStaticMethodID.unwrap())(
        env,
        cls_obj,
        c"latestSurface".as_ptr(),
        c"()Landroid/view/Surface;".as_ptr(),
    );
    if mid.is_null() {
        (tbl.ExceptionClear.unwrap())(env);
        return None;
    }
    let out = (tbl.CallStaticObjectMethodA.unwrap())(env, cls_obj, mid, ptr::null());
    if (tbl.ExceptionCheck.unwrap())(env) != 0 {
        (tbl.ExceptionClear.unwrap())(env);
        return None;
    }
    if out.is_null() { None } else { Some(out) }
}

/// JNI 返回码符号化（设备日志直读，不再对着 252 猜）。
fn jvm_rc_name(rc: jint) -> &'static str {
    match rc {
        0 => "JNI_OK",
        -1 => "JNI_ERR",
        -2 => "JNI_EDETACHED",
        -3 => "JNI_EVERSION",
        -4 => "JNI_ENOMEM（内存/地址空间预留失败）",
        -5 => "JNI_EEXIST（VM 已存在）",
        -6 => "JNI_EINVAL（参数非法）",
        _ => "未知返回码",
    }
}

/// 进程内存态势（CreateJavaVM 失败诊断用）。
fn memory_profile() -> String {
    let mut out = Vec::new();
    if let Ok(s) = std::fs::read_to_string("/proc/self/status") {
        for l in s.lines() {
            if l.starts_with("VmSize") || l.starts_with("VmRSS") {
                out.push(l.trim().to_string());
            }
        }
    }
    if let Ok(s) = std::fs::read_to_string("/proc/meminfo") {
        if let Some(l) = s.lines().find(|l| l.starts_with("MemAvailable")) {
            out.push(l.trim().to_string());
        }
    }
    if out.is_empty() {
        out.push("(内存信息不可读)".into());
    }
    out.join(" | ")
}

/// :game 进程入口（GameService.nativeRunGame JNI 调用）：
/// 读取启动规格 -> 日志重定向 -> 环境准备 -> Surface 等待 ->
/// dlopen JVM -> CreateJavaVM（失败带诊断+最小集重试）-> 退出捕获 -> 调用 main。
unsafe fn run_game_from_spec(
    env: EnvPtr,
    ctx: jni_sys::jobject,
    spec_path: &str,
) -> Result<i32, String> {
    let raw = std::fs::read_to_string(spec_path).map_err(|e| format!("spec 读取失败：{e}"))?;
    let spec: GameSpec = serde_json::from_str(&raw).map_err(|e| format!("spec 解析失败：{e}"))?;

    // 0) 路径重注入（v0.1.8 根因修复，必须先于一切路径使用）：:game 是独立
    //    进程，quartz-core 的 RUNTIME_ROOT/RUNTIME_JRES_ROOT 在本进程为空 ——
    //    不注入则 tagfix/hook jar/tmp/java.library.path/渲染层/LWJGL 全部解析
    //    到错误的回退路径（Android 11+ FUSE 上甚至不可写）→ JVM/游戏秒崩。
    //    重注入后 quartz-core 全部路径/渲染层/LWJGL 函数在本进程即恢复正确。
    crate::paths::set_runtime_root(PathBuf::from(&spec.game_root));
    crate::paths::set_runtime_jres_root(PathBuf::from(&spec.runtime_dir));

    // 0) stdout/stderr 直写日志文件（先于一切：本函数所有输出都进日志）
    {
        let c = CString::new(spec.log_path.clone()).map_err(|_| "日志路径含 NUL")?;
        let fd = libc::open(c.as_ptr(), libc::O_WRONLY | libc::O_CREAT | libc::O_APPEND, 0o644);
        if fd >= 0 {
            libc::dup2(fd, 1);
            libc::dup2(fd, 2);
            if fd > 2 {
                libc::close(fd);
            }
        } else {
            eprintln!("[quartz] 日志文件打开失败（输出将进 logcat）");
        }
    }
    eprintln!(
        "[quartz] :game 进程 {} 启动（OpenJDK JVM 独立进程模式）",
        std::process::id()
    );
    eprintln!("[quartz] 本次游戏崩溃将只终止本进程，启动器 UI 不受影响");
    eprintln!(
        "[quartz] 路径重注入完成：game_root={} | runtime_dir={}",
        spec.game_root,
        spec.runtime_dir
    );
    // 致命信号拦截：先装一次（覆盖 JVM dlopen/初始化期间的 abort）——
    // 让崩溃进程以干净退出码消失，避免部分 ROM 因 debuggerd 崩溃报告整组杀掉应用。
    install_fatal_signal_handlers();

    // 1) 当前目录 + 环境变量
    let workdir = std::path::PathBuf::from(&spec.workdir);
    let jre_home = std::path::PathBuf::from(&spec.jre_home);
    let cwd = CString::new(spec.workdir.clone()).unwrap();
    libc::chdir(cwd.as_ptr());
    std::env::set_var("JAVA_HOME", &jre_home);
    std::env::set_var("HOME", &workdir);
    let active_renderer = crate::renderers::active();
    let ld = format!(
        "{}:{}",
        crate::renderers::install_dir(&active_renderer).display(),
        crate::lwjgl::natives_dir().display()
    );
    std::env::set_var("LD_LIBRARY_PATH", ld);
    let tmp_dir = crate::paths::runtime_dir().join("tmp");
    let _ = std::fs::create_dir_all(&tmp_dir);
    std::env::set_var("TMPDIR", &tmp_dir);
    let egl_lib = crate::renderers::spec_for(&active_renderer)
        .map(|s| s.egl_lib)
        .unwrap_or("libEGL.so");
    std::env::set_var(
        "POJAVEXEC_EGL",
        crate::renderers::install_dir(&active_renderer).join(egl_lib),
    );
    // Zink（Mesa GL4.6→Vulkan）走真桌面 GL 上下文（EGL_KHR_create_context）；
    // 其余渲染层维持 GLES 语义。桥按 POJAV_RENDERER 是否含 "desktopgl" 分流。
    if active_renderer == "zink" {
        std::env::set_var("POJAV_RENDERER", format!("desktopgl_{active_renderer}"));
        // Mesa DRI 驱动（libgallium_dri.so）搜索路径：Android 构建不从系统
        // dri 目录找驱动，必须指向渲染层目录本身。
        std::env::set_var(
            "LIBGL_DRIVERS_PATH",
            crate::renderers::install_dir(&active_renderer),
        );
    } else {
        std::env::set_var("POJAV_RENDERER", format!("opengles_{active_renderer}"));
    }
    for (k, v) in crate::renderers::launch_env(&active_renderer) {
        std::env::set_var(k, v);
    }

    // 1.4) zink：SONAME 修复 shim 覆盖 pack 缺陷 libcutils（预载前必须就位）
    if active_renderer == "zink" {
        if let Err(e) = ensure_zink_cutils_shim() {
            eprintln!("[quartz] libcutils shim 落位失败（zink 按名解析将失败）：{e}");
        }
    }

    // 1.5) 渲染层依赖预载（链接器命名空间按名解析 DT_NEEDED 时不搜索应用
    //      数据目录：zink 的 libc++_shared/libcutils/libglapi/libglxshim/
    //      libgallium_dri 必须由壳层按绝对路径先行 dlopen，之后 Mesa EGL
    //      内部的按名 dlopen 才能命中已载 soinfo —— 否则 libEGL_mesa.so
    //      本体都装不起来）。两遍执行：第一遍失败的多为依赖未就绪，第二遍
    //      补刀；仍失败则记录 dlerror 详情（设备日志可直读根因）。
    {
        let rdir = crate::renderers::install_dir(&active_renderer);
        let mut list: Vec<String> = crate::renderers::preload_libs(&active_renderer)
            .iter()
            .map(|s| s.to_string())
            .collect();
        // 兑底：预载清单外，把目录里全部 .so 也扫一遍（pack 更新新增依赖时
        // 不改代码也能命中；顺序无关——第二遍会补齐先后依赖）。
        if let Ok(rd) = std::fs::read_dir(&rdir) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.ends_with(".so") && !list.contains(&name) {
                    list.push(name);
                }
            }
        }
        let mut failed: Vec<String> = Vec::new();
        for pass in 1..=2 {
            let try_list = if pass == 1 {
                list.clone()
            } else {
                failed.clone()
            };
            failed.clear();
            for lib in &try_list {
                let p = rdir.join(lib);
                if !p.exists() {
                    eprintln!("[quartz] 渲染层预载文件缺失：{}（pass{pass}）", p.display());
                    continue;
                }
                let c = CString::new(p.to_string_lossy().into_owned()).unwrap();
                if dlopen(c.as_ptr(), RTLD_NOW | RTLD_GLOBAL).is_null() {
                    let err = unsafe { last_dlerror() };
                    eprintln!("[quartz] 渲染层预载失败：{lib}（pass{pass}）：{err}");
                    failed.push(lib.clone());
                } else {
                    eprintln!("[quartz] 渲染层预载完成：{lib}（pass{pass}）");
                }
            }
            if failed.is_empty() {
                break;
            }
        }
        if !failed.is_empty() {
            eprintln!(
                "[quartz] 渲染层预载最终未就绪：{}（渲染层 EGL 可能初始化失败）",
                failed.join(", ")
            );
        }
    }
    if active_renderer == "zink" {
        // Mesa 着色器磁盘缓存（可写内部目录；不设则 mesa 退回内存缓存）
        let mesa_cache = crate::paths::game_root().join(".cache").join("mesa");
        let _ = std::fs::create_dir_all(&mesa_cache);
        std::env::set_var("MESA_GLSL_CACHE_DIR", &mesa_cache);
    }

    // 2) 显示桥注入 ART（grab/剪贴板回调注册；ctx = GameService 实例）
    bridge_init_with(env, ctx)?;

    // 2.5) Surface 重挂载兜底：MSG_SURFACE 可能先于本线程的路径重注入到达
    //      （主线程 handler 那时显示桥尚未就绪/路径回退，挂载失败被静默吞掉）。
    //      QuartzBridge.latestSurface() 保存了最近一条有效 Surface —— 这里在
    //      显示桥就绪后重试一次，再进入稳定窗口等待。
    if let Some(api) = BridgeApi::load() {
        if (api.window_ready)() != 1 {
            match quartz_bridge_latest_surface(env, ctx) {
                Some(surface) => {
                    let rc = (api.setup_surface)(env, surface);
                    eprintln!("[quartz] Surface 重挂载兜底完成（rc={rc}）");
                }
                None => eprintln!("[quartz] 无待挂载 Surface（主进程尚未发送）"),
            }
        }
    }

    // 3) tagged-pointer 兼容层（Android 12+ GC SIGABRT 修复）—— 最先于 JVM dlopen
    let tagfix = crate::paths::runtime_dir().join("libqztagfix.so");
    if tagfix.exists() {
        let c = CString::new(tagfix.to_string_lossy().into_owned()).unwrap();
        if dlopen(c.as_ptr(), RTLD_NOW | RTLD_GLOBAL).is_null() {
            eprintln!("[quartz] tagfix dlopen 失败（继续，Android 12+ 可能崩溃）");
        } else {
            eprintln!("[quartz] tagged-pointer 兼容层已加载");
        }
    }

    // 4) 等 Surface（主进程 MSG_SURFACE 先于/伴随 MSG_INIT 到达；横屏旋转需时间）
    if let Some(api) = BridgeApi::load() {
        // v0.1.9-c 稳定窗口检测：主进程切横屏会销毁重建 Surface（surfaceDestroyed
        // -> surfaceCreated），首条 Surface 常在方向切换完成前送达 —— 实机日志
        // 显示「Surface 就绪」后窗口被释放、MC 的 pojavInit 又等了 8s 全程 NULL，
        // 最终无窗口跑崩。改为：窗口就绪后保持 2.5s 未被释放才算就绪；中途被
        // 释放（方向切换空档）则重新计时等待。
        let step = std::time::Duration::from_millis(50);
        let stable_need = 50; // 2.5s 稳定期（50ms × 50）
        let mut stable: u32 = 0;
        let mut waited: u32 = 0;
        let mut ever_ready = false;
        let mut ready = false;
        while waited < 16_000 / 50 {
            std::thread::sleep(step);
            waited += 1;
            if (api.window_ready)() == 1 {
                ever_ready = true;
                stable += 1;
                if stable >= stable_need {
                    ready = true;
                    break;
                }
            } else {
                if ever_ready && stable > 0 {
                    eprintln!(
                        "[quartz] Surface 被主进程释放（方向切换销毁重建），重新等待…（已稳定 {}/2.5s）",
                        stable as f32 * 0.05
                    );
                }
                ever_ready = false;
                stable = 0;
            }
            if waited % 80 == 0 && !ever_ready {
                eprintln!("[quartz] 等待 Surface 创建…");
            }
        }
        if !ready {
            return Err(
                "游戏 Surface 创建超时（16s 含 2.5s 稳定期）：显示桥未收到稳定 Surface".into(),
            );
        }
        eprintln!("[quartz] Surface 就绪（稳定窗口检测通过）");
    }

    // 5) JVM 参数装配：-cp 对转 -Djava.class.path + 追加退出捕获 hook jar；
    //    jna.tmpdir / SharedLibraryExtractPath / java.library.path 统一改指
    //    内部可 exec 目录（/storage/emulated/0 挂载 noexec，从那里 dlopen
    //    提取出的 .so 必败 —— JNA/oshi NPE 与 LWJGL 版本错乱的根因）。
    ensure_hook_jar()?;
    let hook_jar = crate::paths::runtime_dir().join("quartz-game-hook.jar");
    let mut options: Vec<String> = Vec::new();
    let mut args_iter = spec.jvm_args.iter().cloned().peekable();
    while let Some(arg) = args_iter.next() {
        if arg == "-cp" {
            if let Some(cp) = args_iter.next() {
                options.push(format!("-Djava.class.path={cp}:{}", hook_jar.display()));
            }
            continue;
        }
        if arg.starts_with("-cp=") {
            options.push(format!(
                "-Djava.class.path={}:{}",
                &arg[4..],
                hook_jar.display()
            ));
            continue;
        }
        if arg.starts_with("-Djna.tmpdir=")
            || arg.starts_with("-Dorg.lwjgl.system.SharedLibraryExtractPath=")
            || arg.starts_with("-Djava.library.path=")
        {
            continue;
        }
        options.push(arg);
    }
    let gl_lib = crate::renderers::spec_for(&active_renderer)
        .map(|s| s.gl_lib)
        .unwrap_or("libgl4es_114.so");
    let jlp = format!(
        "{}:{}:{}",
        crate::lwjgl::natives_dir().display(),
        crate::renderers::install_dir(&active_renderer).display(),
        spec.natives_dir
    );
    options.push(format!("-Djava.library.path={jlp}"));
    options.push(format!("-Dorg.lwjgl.opengl.libname={gl_lib}"));
    options.push(format!("-Djava.io.tmpdir={}", tmp_dir.display()));
    options.push(format!("-Djna.tmpdir={}", tmp_dir.display()));
    // JNA 引导库修复（v9）：jna.jar 自带的 linux-aarch64 libjnidispatch 是
    // glibc 构建，bionic 拒载 → com.sun.jna.Native 类初始化失败 →
    // oshi/text2speech 全线 NoClassDefFoundError。jna.boot.library.path
    // 优先级高于 jar 解包，指向 LWJGL natives 的 bionic 构建即命中。
    options.push(format!(
        "-Djna.boot.library.path={}",
        crate::lwjgl::natives_dir().display()
    ));
    options.push("-Djna.nounpack=true".into());
    options.push(format!(
        "-Dorg.lwjgl.system.SharedLibraryExtractPath={}",
        tmp_dir.display()
    ));
    // G1 调优（Pojav 系移动端默认配方）：降低长停顿，改善低端机体验。
    // v0.2.0-e 修复：G1NewSizePercent 在 OpenJDK17 为 experimental —— 解锁项
    // 必须位于所有实验项之前（缺它则全参数集 rc=-6，退化到最小集回退路径）。
    options.push("-XX:+UnlockExperimentalVMOptions".into());
    options.push("-XX:+UseG1GC".into());
    options.push("-XX:G1NewSizePercent=20".into());
    options.push("-XX:G1ReservePercent=20".into());
    options.push("-XX:MaxGCPauseMillis=50".into());
    options.push("-XX:G1HeapRegionSize=16M".into());
    options.push("-XX:-UsePerfData".into());
    options.push(format!(
        "-XX:ErrorFile={}",
        crate::paths::game_root().join("logs").join("hs_err_%p.log").display()
    ));
    options.push("-Xlog:init=info:stderr".into());
    options.push("-Djava.security.manager=allow".into());

    // classpath 预检（v0.2.0-e）：逐 jar 存在性/非空 —— 下载管线漏项、
    // client jar 缺失（「主类未找到」头号嫌疑）在 CreateJavaVM 前直报。
    for o in &options {
        if let Some(cp) = o.strip_prefix("-Djava.class.path=") {
            let entries: Vec<&str> = cp.split(':').collect();
            let mut missing: Vec<String> = Vec::new();
            let mut ok = 0usize;
            for (i, entry) in entries.iter().enumerate() {
                match std::fs::metadata(entry) {
                    Ok(m) if m.len() > 0 => ok += 1,
                    Ok(_) => missing.push(format!("[{i}] {entry}（0 字节）")),
                    Err(e) => missing.push(format!("[{i}] {entry}（{e}）")),
                }
            }
            if missing.is_empty() {
                eprintln!("[quartz] classpath 预检：{ok}/{} 项全部就绪", entries.len());
            } else {
                eprintln!(
                    "[quartz] classpath 预检：{}/{} 项就绪，{} 项异常：",
                    ok,
                    entries.len(),
                    missing.len()
                );
                for m in &missing {
                    eprintln!("[quartz] classpath 异常：{m}");
                }
            }
        }
    }

    // 6) dlopen JRE 依赖（绝对路径，绕开 bionic 启动期缓存的 LD_LIBRARY_PATH）
    let mut jvm_handle: *mut c_void = ptr::null_mut();
    let libs = [
        "lib/jli/libjli.so", "lib/libjli.so", "lib/server/libjvm.so",
        "lib/libverify.so", "lib/libjava.so", "lib/libnet.so", "lib/libnio.so",
        "lib/libzip.so", "lib/libjimage.so", "lib/libawt.so",
        "lib/libawt_headless.so", "lib/libfreetype.so", "lib/libfontmanager.so",
    ];
    for rel in libs {
        let p = jre_home.join(rel);
        if !p.exists() {
            continue;
        }
        let c = CString::new(p.to_string_lossy().into_owned()).unwrap();
        let h = dlopen(c.as_ptr(), RTLD_NOW | RTLD_GLOBAL);
        if h.is_null() {
            if rel.ends_with("libjvm.so") {
                return Err(format!("libjvm.so dlopen 失败：{}", p.display()));
            }
            eprintln!("[quartz] {} dlopen 失败（跳过）", p.display());
        } else if rel.ends_with("libjvm.so") {
            jvm_handle = h;
        }
    }
    if jvm_handle.is_null() {
        return Err("libjvm.so 未加载".into());
    }
    eprintln!("[quartz] JVM 动态库装载完成");

    // 7) JNI_CreateJavaVM
    let create: unsafe extern "C" fn(
        *mut VmPtr,
        *mut *mut jni_sys::JNIEnv,
        *mut jni_sys::JavaVMInitArgs,
    ) -> jint = {
        let cname = CString::new("JNI_CreateJavaVM").unwrap();
        let p = dlsym(jvm_handle, cname.as_ptr());
        if p.is_null() {
            return Err("JNI_CreateJavaVM 符号缺失".into());
        }
        std::mem::transmute(p)
    };
    let make_args = |opts: &[String]| -> Result<(Vec<jni_sys::JavaVMOption>, Vec<CString>), String> {
        let mut vopts: Vec<jni_sys::JavaVMOption> = Vec::new();
        let mut leaked: Vec<CString> = Vec::new();
        for opt in opts {
            let cs = CString::new(opt.clone()).map_err(|_| "JVM 参数含 NUL".to_string())?;
            vopts.push(jni_sys::JavaVMOption {
                optionString: cs.as_ptr() as *mut c_char,
                extraInfo: ptr::null_mut(),
            });
            leaked.push(cs); // 保活
        }
        Ok((vopts, leaked))
    };
    let (mut jvm_options, _leaked) = make_args(&options)?;
    let mut init_args = jni_sys::JavaVMInitArgs {
        version: JNI_VERSION_1_8,
        nOptions: jvm_options.len() as jint,
        options: jvm_options.as_mut_ptr(),
        ignoreUnrecognized: 0,
    };
    let mut vm: VmPtr = ptr::null_mut();
    let mut jenv: EnvPtr = ptr::null_mut();
    let rc = create(&mut vm, &mut jenv, &mut init_args);
    if rc != JNI_OK || vm.is_null() || jenv.is_null() {
        // 诊断直达游戏日志（退出时同步排干进控制台）：rc 符号名 + 内存态势，
        // 并用最小参数集自动重试 —— 区分「JVM 起不来」与「某参数不被接受」。
        eprintln!(
            "[quartz] JNI_CreateJavaVM 失败：rc={rc}（{}）；进程内存：{}；参数 {} 项",
            jvm_rc_name(rc),
            memory_profile(),
            options.len()
        );
        let mut minimal: Vec<String> = Vec::new();
        // v0.2.0-e 最小集扩充：除 classpath/library.path/tmpdir 等硬需求外，
        // 保留 JNA/LWJGL/Netty 路径与启动器标识 —— 历史版本最小集过瘦，
        // 回退成功后 JNA 引导库缺失会让 oshi/text2speech 全线炸。
        // Xmx 用 spec 原值（-Xmx 前缀匹配），不再硬编码 512M。
        for o in &options {
            let keep = o.starts_with("-Djava.class.path=")
                || o.starts_with("-Djava.library.path=")
                || o.starts_with("-Djava.io.tmpdir=")
                || o.starts_with("-Djna.tmpdir=")
                || o.starts_with("-Djna.boot.library.path=")
                || o.starts_with("-Djna.nounpack=")
                || o.starts_with("-Dorg.lwjgl.")
                || o.starts_with("-Dio.netty.native.workdir=")
                || o.starts_with("-Dminecraft.launcher.")
                || o.starts_with("-Xmx")
                || o.starts_with("-Djava.security.manager=")
                || o.starts_with("-XX:ErrorFile=");
            if keep {
                minimal.push(o.clone());
            }
        }
        minimal.push("-XX:-UsePerfData".into());
        minimal.push("-XX:+UnlockExperimentalVMOptions".into());
        eprintln!(
            "[quartz] 最小参数集（{} 项）重试 CreateJavaVM…",
            minimal.len()
        );
        let (mut opts2, _leaked2) = make_args(&minimal)?;
        let mut args2 = jni_sys::JavaVMInitArgs {
            version: JNI_VERSION_1_8,
            nOptions: opts2.len() as jint,
            options: opts2.as_mut_ptr(),
            ignoreUnrecognized: 0,
        };
        vm = ptr::null_mut();
        jenv = ptr::null_mut();
        let rc2 = create(&mut vm, &mut jenv, &mut args2);
        if rc2 != JNI_OK || vm.is_null() || jenv.is_null() {
            return Err(format!(
                "JNI_CreateJavaVM 失败：全参数 rc={rc}（{}），最小集 rc={rc2}（{}）；内存：{}",
                jvm_rc_name(rc),
                jvm_rc_name(rc2),
                memory_profile()
            ));
        }
        eprintln!(
            "[quartz] 最小参数集重试成功 —— 全参数集存在该 JVM 构建不接受的项（首次 rc={rc}，{}）",
            jvm_rc_name(rc)
        );
    }
    eprintln!("[quartz] :game 进程内 JVM 创建成功（OpenJDK 17，与 ART 并存）");

    // JVM 初始化会覆盖 SIGABRT 处理器（HotSpot 崩溃报告用）：重装拦截 ——
    // HotSpot 写完 hs_err 后调用 abort()，仍落入本拦截（干净退出，不惊动系统）。
    install_fatal_signal_handlers();

    // 7.5) 显式注入 runtime VM：统一桥路径后，壳层先 dlopen 了桥库，libart
    //      的 System.loadLibrary 发现库已载会跳过 JNI_OnLoad —— rt_vm/GLFW
    //      缓存必须由这里补齐（quartz_bridge_notify_rt_vm，幂等）。
    if let Some(api) = BridgeApi::load() {
        let rc = (api.notify_rt_vm)(vm);
        eprintln!("[quartz] runtime VM 注入显示桥（rc={rc}）");
    }

    // 之后所有 JNI（hook 安装 / 主类定位 / main 调用）都必须在**游戏 JVM 侧**
    // 执行（用 CreateJavaVM 返回的 jenv）。v0.1.7 重构曾误用 ART env ——
    // ART 的类加载器找不到 hook jar 与游戏主类（此前设备从未走到此处，
    // CreateJavaVM 先失败了；不修则修好 JVM 后必然报「主类未找到」）。
    let env = jenv;

    // 8) 安装退出捕获 SecurityManager（自研 hook jar）：System.exit/halt 被
    //    否决并转为 ExitSignal，崩溃码沿调用栈返回本函数而非 os::exit。
    let tbl = &**env;
    {
        let sm_name = CString::new("com/quartz/game/QuartzExitManager").unwrap();
        let sm_cls = (tbl.FindClass.unwrap())(env, sm_name.as_ptr());
        if sm_cls.is_null() {
            // v0.2.0-e：先打印真实异常堆栈再清除（诊断「找不到」的真实原因）
            (tbl.ExceptionDescribe.unwrap())(env);
            (tbl.ExceptionClear.unwrap())(env);
            eprintln!("[quartz] 退出捕获类未找到（hook jar 未进 classpath？）");
        } else {
            let mid = (tbl.GetStaticMethodID.unwrap())(
                env,
                sm_cls,
                c"install".as_ptr(),
                c"()V".as_ptr(),
            );
            if mid.is_null() {
                (tbl.ExceptionClear.unwrap())(env);
                eprintln!("[quartz] 退出捕获 install 未找到");
            } else {
                (tbl.CallStaticVoidMethodA.unwrap())(env, sm_cls, mid, ptr::null());
                if (tbl.ExceptionCheck.unwrap())(env) != 0 {
                    (tbl.ExceptionClear.unwrap())(env);
                    eprintln!("[quartz] 退出捕获安装失败（需 -Djava.security.manager=allow）");
                } else {
                    eprintln!("[quartz] 退出捕获已安装（崩溃/退出将回报启动器）");
                }
            }
        }
        if (tbl.ExceptionCheck.unwrap())(env) != 0 {
            (tbl.ExceptionClear.unwrap())(env);
        }
    }

    // v0.2.0-e 诊断：回读 JVM 侧 java.class.path / java.library.path 实际值 ——
    // 「主类未找到」时区分「属性未生效」与「jar 不可读」两类根因。
    {
        let tbl = &**env;
        let sys_cls = (tbl.FindClass.unwrap())(env, c"java/lang/System".as_ptr());
        if sys_cls.is_null() {
            (tbl.ExceptionClear.unwrap())(env);
            eprintln!("[quartz][diag] java/lang/System 定位失败（异常环境）");
        } else if let Some(mid) = {
            let m = (tbl.GetStaticMethodID.unwrap())(
                env,
                sys_cls,
                c"getProperty".as_ptr(),
                c"(Ljava/lang/String;)Ljava/lang/String;".as_ptr(),
            );
            if m.is_null() {
                (tbl.ExceptionClear.unwrap())(env);
                None
            } else {
                Some(m)
            }
        } {
            for key in ["java.class.path", "java.library.path"] {
                let jkey = (tbl.NewStringUTF.unwrap())(env, key.as_ptr());
                let jargs = [jni_sys::jvalue { l: jkey }];
                let jval = (tbl.CallStaticObjectMethodA.unwrap())(env, sys_cls, mid, jargs.as_ptr());
                if (tbl.ExceptionCheck.unwrap())(env) != 0 {
                    (tbl.ExceptionClear.unwrap())(env);
                    continue;
                }
                if jval.is_null() {
                    eprintln!("[quartz][diag] JVM 属性 {key} = <null>（未生效！）");
                    continue;
                }
                let p = (tbl.GetStringUTFChars.unwrap())(env, jval, ptr::null_mut());
                if p.is_null() {
                    (tbl.ExceptionClear.unwrap())(env);
                    continue;
                }
                let s = CStr::from_ptr(p).to_string_lossy().into_owned();
                (tbl.ReleaseStringUTFChars.unwrap())(env, jval, p);
                let head: String = s.chars().take(240).collect();
                eprintln!("[quartz][diag] JVM 属性 {key}：len={} head={head}", s.len());
            }
        }
        if (tbl.ExceptionCheck.unwrap())(env) != 0 {
            (tbl.ExceptionClear.unwrap())(env);
        }
    }


    // 9) 定位主类并调用 main
    let main_path = format!("{}", spec.main_class.replace('.', "/"));
    let cls_c = CString::new(main_path.clone()).unwrap();
    let cls = (tbl.FindClass.unwrap())(env, cls_c.as_ptr());
    if cls.is_null() {
        // v0.2.0-e：打印真实异常（ClassNotFoundException 详情）再清除 ——
        // 区分「类缺失」与「加载器初始化失败」；上方 [diag] 回读可交叉验证。
        (tbl.ExceptionDescribe.unwrap())(env);
        (tbl.ExceptionClear.unwrap())(env);
        return Err(format!("主类 {} 未找到（classpath 配置错误？）", spec.main_class));
    }
    let mid = (tbl.GetStaticMethodID.unwrap())(
        env,
        cls,
        c"main".as_ptr(),
        c"([Ljava/lang/String;)V".as_ptr(),
    );
    if mid.is_null() {
        (tbl.ExceptionClear.unwrap())(env);
        return Err("main 方法未找到".into());
    }
    let str_cls = (tbl.FindClass.unwrap())(env, c"java/lang/String".as_ptr());
    if str_cls.is_null() {
        (tbl.ExceptionClear.unwrap())(env);
        return Err("java/lang/String 未找到".into());
    }
    let arr = (tbl.NewObjectArray.unwrap())(
        env,
        spec.game_args.len() as i32,
        str_cls,
        ptr::null_mut(),
    );
    if arr.is_null() {
        (tbl.ExceptionClear.unwrap())(env);
        return Err("参数数组创建失败".into());
    }
    let mut cstrs: Vec<CString> = Vec::new();
    for (i, a) in spec.game_args.iter().enumerate() {
        let cs = CString::new(a.clone()).map_err(|_| "游戏参数含 NUL".to_string())?;
        let js = (tbl.NewStringUTF.unwrap())(env, cs.as_ptr());
        cstrs.push(cs);
        if js.is_null() {
            (tbl.ExceptionClear.unwrap())(env);
            return Err("参数字符串创建失败".into());
        }
        (tbl.SetObjectArrayElement.unwrap())(env, arr, i as i32, js);
    }
    let arg_value = jni_sys::jvalue { l: arr };
    eprintln!(
        "[quartz] 调用 {} main（{} 个游戏参数）",
        spec.main_class,
        spec.game_args.len()
    );
    let call = tbl.CallStaticVoidMethodA.unwrap();
    call(env, cls, mid, &arg_value);
    let threw = (tbl.ExceptionCheck.unwrap())(env) != 0;
    if threw {
        // 异常逃逸主类（MC 崩溃且 ExitSignal 未被吞的路径）：打印后按 255 处理
        (tbl.ExceptionDescribe.unwrap())(env);
        (tbl.ExceptionClear.unwrap())(env);
        eprintln!("[quartz] 主类抛出未捕获异常（详见上方 stderr）");
        return Ok(255);
    }
    eprintln!("[quartz] main 返回（游戏正常退出）");
    Ok(0)
}

// ---------------------------------------------------------------------------
// JNI 入口（Kotlin -> Rust）
// ---------------------------------------------------------------------------

/// :game 进程执行入口 —— GameService.nativeRunGame(specPath)。
/// 返回游戏退出码；错误路径返回 251/252（详情写 stderr -> 日志文件）。
#[no_mangle]
pub unsafe extern "C" fn Java_com_quartz_launcher_GameService_nativeRunGame(
    env: EnvPtr,
    service: jni_sys::jobject,
    spec_path: jni_sys::jstring,
) -> jint {
    let tbl = &**env;
    let p = (tbl.GetStringUTFChars.unwrap())(env, spec_path, ptr::null_mut());
    if p.is_null() {
        return 251;
    }
    let path = CStr::from_ptr(p).to_string_lossy().into_owned();
    (tbl.ReleaseStringUTFChars.unwrap())(env, spec_path, p);
    match run_game_from_spec(env, service, &path) {
        Ok(code) => code as jint,
        Err(e) => {
            eprintln!("[quartz] 游戏启动失败：{e}");
            252
        }
    }
}
