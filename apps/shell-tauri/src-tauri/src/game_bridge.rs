//! Android 进程内游戏启动 + 显示桥接线（v0.1.5「游戏显示接入」主战场）。
//!
//! 架构（对标 PojavLauncher 的进程内 JVM 方案，但全部用 Rust 实现）：
//! 1. 壳层把 SurfaceView 的 Surface 交给 quartz-bridge（ANativeWindow_fromSurface）；
//! 2. 壳层 dlopen JVM pack 的 libjvm.so -> JNI_CreateJavaVM（同进程，与 ART 并存）；
//! 3. 游戏 LWJGL 类（gladiolus 官方 jar）System.loadLibrary("pojavexec") 时
//!    加载的是我们 Rust 重构的桥（crates/quartz-bridge，以 libpojavexec.so 落位）；
//! 4. 输入：Kotlin SurfaceView/虚拟按键 -> QuartzBridge(ART native) -> 桥 C ABI
//!    -> 事件队列 -> 渲染线程 PumpEvents -> LWJGL 回调 -> Minecraft；
//! 5. 渲染：pojavCreateContext 经渲染层自带 EGL（MobileGlues）在 ANativeWindow
//!    上建 GLES3 上下文，MobileGlues 桌面 GL 语义由 LWJGL 函数指针绑定。
//!
//! 退出语义：Main.main 正常返回 -> 恢复竖屏/UI、上报退出码 0；MC 崩溃路径
//! System.exit -> OpenJDK os::exit 直接终止进程（本 JRE 无 exit 补丁），
//! 启动日志已实时落盘，崩溃后重启应用即可取证；再次启动时检测 JVM 已消费，
//! 自动重启应用进程以重置 JVM（同进程内 JVM 无法二次创建）。

#![cfg(target_os = "android")]

use crate::LaunchExit;
use crate::LaunchLog;
use tauri::Emitter;
use jni_sys::{jboolean, jint, JNI_OK, JNI_VERSION_1_8};
use std::ffi::{c_char, c_int, c_void, CString};
use std::path::Path;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

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
}

impl BridgeApi {
    fn load() -> Option<&'static BridgeApi> {
        static API: std::sync::OnceLock<Option<BridgeApi>> = std::sync::OnceLock::new();
        if let Some(cached) = API.get() {
            return cached.as_ref();
        }
        let loaded = (|| -> Option<BridgeApi> {
            let path = super::paths::runtime_dir().join("libpojavexec.so");
            let cpath = CString::new(path.to_string_lossy().into_owned()).ok()?;
            let handle = unsafe { dlopen(cpath.as_ptr(), RTLD_NOW | RTLD_GLOBAL) };
            if handle.is_null() {
                eprintln!("[quartz] 显示桥 dlopen 失败：{}", path.display());
                return None;
            }
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
            })
        })();
        let _ = API.set(loaded);
        API.get()?.as_ref()
    }
}

unsafe extern "C" {
    fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}
const RTLD_NOW: c_int = 2;
const RTLD_GLOBAL: c_int = 0x100;

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
// Kotlin 壳层静态方法（Surface 视图显隐 / 方向 / 应用重启）
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

/// 重启应用进程（JVM 无法在进程内二次创建时的重置手段）。
pub fn restart_app() {
    let _ = call_activity_static("restartAppStatic", "()V", &[]);
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
// 进程内 JVM 启动
// ---------------------------------------------------------------------------

/// JVM 已消费标记：OpenJDK 在一个进程内只能 JNI_CreateJavaVM 一次。
static JVM_SPENT: AtomicBool = AtomicBool::new(false);
/// 游戏运行中标记（含 pid 占位语义；进程内模式 pid = 本进程）。
static GAME_RUNNING: AtomicBool = AtomicBool::new(false);

pub fn game_running() -> bool {
    GAME_RUNNING.load(Ordering::SeqCst)
}

pub fn jvm_spent() -> bool {
    JVM_SPENT.load(Ordering::SeqCst)
}

/// 进程内停止：MC 无干净终止通道，System.exit 将结束应用进程（日志已落盘）。
pub fn stop_in_process(app: &tauri::AppHandle) {
    let _ = app.emit(
        "launch://log",
        &LaunchLog {
            source: "quartz".into(),
            line: "[quartz] 停止游戏：进程内 JVM 将强制结束（应用自动重启，启动日志已保存）".into(),
        },
    );
    let _ = app.emit("launch://exit", &LaunchExit {
        code: 137,
        version: String::new(),
        log_file: None,
    });
    if let Some(vm) = JVM.vm() {
        unsafe {
            let iface = &**vm;
            let mut env: EnvPtr = ptr::null_mut();
            if (iface.GetEnv.unwrap())(vm, &mut env as *mut EnvPtr as *mut *mut c_void, JNI_VERSION_1_8) == JNI_OK
                && !env.is_null()
            {
                let tbl = &**env;
                let sys = (tbl.FindClass.unwrap())(env, c"java/lang/System".as_ptr());
                if !sys.is_null() {
                    let exit_mid = (tbl.GetStaticMethodID.unwrap())(
                        env,
                        sys,
                        c"exit".as_ptr(),
                        c"(I)V".as_ptr(),
                    );
                    if !exit_mid.is_null() {
                        let arg = jni_sys::jvalue { i: 0 };
                        (tbl.CallStaticVoidMethodA.unwrap())(env, sys, exit_mid, &arg);
                    }
                }
                if (tbl.ExceptionCheck.unwrap())(env) != 0 {
                    (tbl.ExceptionClear.unwrap())(env);
                }
            }
        }
    }
    // JVM 不可用时兜底：直接结束本进程
    std::process::exit(0);
}

/// JVM 句柄缓存（CreateVM 成功后存入；stop 用）。
/// raw 指针存 usize（raw 指针非 Send，无法进 static Mutex）。
struct JvmSlot(Mutex<Option<usize>>);
static JVM: JvmSlot = JvmSlot(Mutex::new(None));

impl JvmSlot {
    fn vm(&self) -> Option<VmPtr> {
        self.0.lock().ok().and_then(|g| g.map(|p| p as VmPtr))
    }
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

/// 进程内启动主流程。前置：JVM pack / LWJGL pack / 渲染层 / 显示桥文件均已就位。
pub fn launch_in_process(
    version_id: String,
    plan: crate::jvmops::LaunchPlan,
    major: u32,
    app: tauri::AppHandle,
    log_file: Arc<Mutex<Option<std::fs::File>>>,
) -> Result<(), String> {
    use std::io::BufRead;

    let qlog: Arc<dyn Fn(&str, &str) + Send + Sync> = {
        let app = app.clone();
        let log_file = log_file.clone();
        Arc::new(move |source: &str, line: &str| {
            qlog_emit(&app, &log_file, source, line);
        })
    };

    // ---- JVM 参数装配（CreateVM options；-cp 对转 -Djava.class.path）----
    let mut options: Vec<String> = Vec::new();
    let mut args_iter = plan.jvm_args.iter().cloned().peekable();
    while let Some(arg) = args_iter.next() {
        if arg == "-cp" {
            if let Some(cp) = args_iter.next() {
                options.push(format!("-Djava.class.path={cp}"));
            }
            continue;
        }
        if arg.starts_with("-cp=") {
            options.push(format!("-Djava.class.path={}", &arg[4..]));
            continue;
        }
        options.push(arg);
    }
    // Android 附加 JVM 参数（java.library.path / 渲染层 GL 库 / tmpdir / 错误报告）
    let jre_home = crate::runtime::install_dir(major);
    let tmp_dir = crate::paths::runtime_dir().join("tmp");
    let _ = std::fs::create_dir_all(&tmp_dir);
    let active_renderer = crate::renderers::active();
    let gl_lib = crate::renderers::spec_for(&active_renderer)
        .map(|s| s.gl_lib)
        .unwrap_or("libgl4es_114.so");
    let jlp = format!(
        "{}:{}:{}",
        crate::lwjgl::natives_dir().display(),
        crate::renderers::install_dir(&active_renderer).display(),
        plan.natives_dir.display()
    );
    options.push(format!("-Djava.library.path={jlp}"));
    options.push(format!("-Dorg.lwjgl.opengl.libname={gl_lib}"));
    options.push(format!("-Djava.io.tmpdir={}", tmp_dir.display()));
    options.push("-XX:-UsePerfData".into());
    options.push(format!(
        "-XX:ErrorFile={}",
        crate::paths::game_root().join("logs").join("hs_err_%p.log").display()
    ));
    options.push("-Xlog:init=info:stderr".into());

    // ---- 环境变量集（getenv 可见；实际 setenv 在 run_jvm 启动线程内执行）----
    let renderer_env = crate::renderers::launch_env(&active_renderer);

    // ---- Surface 交接（先建窗口再开 JVM）----
    ensure_bridge_loaded()?;
    qlog("quartz", "[quartz] 显示游戏视图（横屏 SurfaceView）");
    show_game_surface();
    let mut ready = false;
    for i in 0..240 {
        let ok = BridgeApi::load().map(|a| unsafe { (a.window_ready)() }).unwrap_or(0);
        if ok == 1 {
            ready = true;
            break;
        }
        if i % 20 == 19 {
            qlog("quartz", "[quartz] 等待 Surface 创建…");
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    if !ready {
        hide_game_surface();
        return Err("游戏 Surface 创建超时（12s）：显示桥未收到 Surface".into());
    }
    qlog("quartz", "[quartz] Surface 就绪，开始创建进程内 JVM");

    // ---- stdout/stderr 重定向到管道（JVM/游戏输出进控制台 + 日志文件）----
    let saved_fds: Arc<Mutex<Vec<i32>>> = Arc::new(Mutex::new(Vec::new()));
    unsafe {
        let mut out_pipe = [0 as libc::c_int; 2];
        let mut err_pipe = [0 as libc::c_int; 2];
        if libc::pipe(out_pipe.as_mut_ptr()) == 0 && libc::pipe(err_pipe.as_mut_ptr()) == 0 {
            let saved_out = libc::dup(1);
            let saved_err = libc::dup(2);
            libc::dup2(out_pipe[1], 1);
            libc::dup2(err_pipe[1], 2);
            libc::close(out_pipe[1]);
            libc::close(err_pipe[1]);
            *saved_fds.lock().unwrap() = vec![saved_out, saved_err, out_pipe[0], err_pipe[0]];
            let qout = qlog.clone();
            std::thread::spawn(move || {
                let reader = std::io::BufReader::new(unsafe {
                    std::mem::transmute::<i32, std::fs::File>(out_pipe[0])
                });
                for line in reader.lines().map_while(Result::ok) {
                    qout("stdout", &line);
                }
            });
            let qerr = qlog.clone();
            std::thread::spawn(move || {
                let reader = std::io::BufReader::new(unsafe {
                    std::mem::transmute::<i32, std::fs::File>(err_pipe[0])
                });
                for line in reader.lines().map_while(Result::ok) {
                    qerr("stderr", &line);
                }
            });
        }
    }

    // ---- 启动线程：dlopen JVM -> CreateVM -> invoke main ----
    GAME_RUNNING.store(true, Ordering::SeqCst);
    let (done_tx, done_rx) = std::sync::mpsc::channel::<Result<i32, String>>();
    let (destroy_tx, destroy_rx) = std::sync::mpsc::channel::<String>();
    let version_id_thread = version_id.clone();
    let qlog2 = qlog.clone();
    let jre_home_thread = jre_home.clone();
    let workdir = plan.working_dir.clone();
    std::thread::Builder::new()
        .name("quartz-jvm".into())
        .stack_size(64 * 1024 * 1024)
        .spawn(move || {
            let result = run_jvm(
                &jre_home_thread,
                &workdir,
                &plan.main_class,
                &plan.game_args,
                &options,
                &active_renderer,
                &renderer_env,
                &qlog2,
            );
            let _ = done_tx.send(result);
            let _ = destroy_tx;
        })
        .map_err(|e| format!("启动线程创建失败：{e}"))?;

    // ---- 看护线程：启动结果 -> 退出恢复（UI/竖屏/事件）----
    let app_exit = app.clone();
    let version_id_exit = version_id_thread;
    let log_file_exit = log_file.clone();
    let saved_fds_exit = saved_fds.clone();
    let qlog_exit = qlog.clone();
    let _destroy_rx = destroy_rx;
    std::thread::spawn(move || {
        let code: i32 = match done_rx.recv() {
            Ok(Ok(code)) => code,
            Ok(Err(e)) => {
                qlog_exit("quartz", &format!("[quartz] JVM 启动失败：{e}"));
                -1
            }
            Err(_) => -1,
        };
        JVM_SPENT.store(true, Ordering::SeqCst);
        GAME_RUNNING.store(false, Ordering::SeqCst);
        // 恢复 fd（短暂延迟让管道尾部输出被泵完）
        std::thread::sleep(std::time::Duration::from_millis(300));
        unsafe {
            if let Ok(fds) = saved_fds_exit.lock() {
                if fds.len() == 4 {
                    libc::dup2(fds[0], 1);
                    libc::dup2(fds[1], 2);
                }
            }
        }
        qlog_exit("quartz", &format!("[quartz] 游戏退出，代码 {code}（进程内模式）"));
        hide_game_surface();
        let _ = app_exit.emit(
            "launch://exit",
            &LaunchExit {
                code,
                version: version_id_exit,
                log_file: None,
            },
        );
        let _ = log_file_exit;
    });
    Ok(())
}

/// 线程体：dlopen tagfix/bridge/JRE -> JNI_CreateJavaVM -> 调主类。
#[allow(clippy::too_many_arguments)]
fn run_jvm(
    jre_home: &Path,
    workdir: &Path,
    main_class: &str,
    game_args: &[String],
    options: &[String],
    active_renderer: &str,
    renderer_env: &[(String, String)],
    qlog: &Arc<dyn Fn(&str, &str) + Send + Sync>,
) -> Result<i32, String> {
    unsafe {
        // 当前目录（MC 工作目录）
        let cwd = CString::new(workdir.to_string_lossy().into_owned()).unwrap();
        libc::chdir(cwd.as_ptr());

        // 环境变量
        std::env::set_var("JAVA_HOME", jre_home);
        std::env::set_var("HOME", workdir);
        let ld = format!(
            "{}:{}",
            crate::renderers::install_dir(active_renderer).display(),
            crate::lwjgl::natives_dir().display()
        );
        std::env::set_var("LD_LIBRARY_PATH", ld);
        let tmp = crate::paths::runtime_dir().join("tmp");
        std::env::set_var("TMPDIR", &tmp);
        let egl_lib = crate::renderers::spec_for(active_renderer)
            .map(|s| s.egl_lib)
            .unwrap_or("libEGL.so");
        let egl_path = crate::renderers::install_dir(active_renderer).join(egl_lib);
        std::env::set_var("POJAVEXEC_EGL", &egl_path);
        std::env::set_var("POJAV_RENDERER", format!("opengles_{active_renderer}"));
        for (k, v) in renderer_env {
            std::env::set_var(k, v);
        }

        // 1) tagged-pointer 兼容层（Android 12+ GC SIGABRT 修复）—— 最先 dlopen
        let tagfix = crate::paths::runtime_dir().join("libqztagfix.so");
        if tagfix.exists() {
            let c = CString::new(tagfix.to_string_lossy().into_owned()).unwrap();
            let h = dlopen(c.as_ptr(), RTLD_NOW | RTLD_GLOBAL);
            if h.is_null() {
                qlog("quartz", "[quartz] tagfix dlopen 失败（继续，Android 12+ 可能崩溃）");
            } else {
                qlog("quartz", "[quartz] tagged-pointer 兼容层已加载（进程内）");
            }
        }

        // 2) JRE 依赖装载（绝对路径，绕开 bionic 启动期缓存的 LD_LIBRARY_PATH）
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
                qlog("quartz", &format!("[quartz] {} dlopen 失败（跳过）", p.display()));
            } else if rel.ends_with("libjvm.so") {
                jvm_handle = h;
            }
        }
        if jvm_handle.is_null() {
            return Err("libjvm.so 未加载".into());
        }
        qlog("quartz", "[quartz] JVM 动态库装载完成（进程内）");

        // 3) JNI_CreateJavaVM
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
        let mut jvm_options: Vec<jni_sys::JavaVMOption> = Vec::new();
        let mut leaked: Vec<CString> = Vec::new();
        for opt in options {
            let cs = CString::new(opt.clone()).map_err(|_| "JVM 参数含 NUL".to_string())?;
            jvm_options.push(jni_sys::JavaVMOption {
                optionString: cs.as_ptr() as *mut c_char,
                extraInfo: ptr::null_mut(),
            });
            leaked.push(cs); // 保活
        }
        let mut init_args = jni_sys::JavaVMInitArgs {
            version: JNI_VERSION_1_8,
            nOptions: jvm_options.len() as jint,
            options: jvm_options.as_mut_ptr(),
            ignoreUnrecognized: 0,
        };
        let mut vm: VmPtr = ptr::null_mut();
        let mut env: EnvPtr = ptr::null_mut();
        let rc = create(&mut vm, &mut env, &mut init_args);
        if rc != JNI_OK || vm.is_null() || env.is_null() {
            return Err(format!("JNI_CreateJavaVM 失败（rc={rc}）"));
        }
        *JVM.0.lock().unwrap() = Some(vm as usize);
        qlog("quartz", "[quartz] 进程内 JVM 创建成功（OpenJDK 17，与 ART 并存）");

        // 4) 定位主类并调用 main
        let tbl = &**env;
        let main_path = format!("{}",
            main_class.replace('.', "/"));
        let cls_c = CString::new(main_path.clone()).unwrap();
        let cls = (tbl.FindClass.unwrap())(env, cls_c.as_ptr());
        if cls.is_null() {
            (tbl.ExceptionClear.unwrap())(env);
            return Err(format!("主类 {main_class} 未找到（classpath 配置错误？）"));
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
        let arr = (tbl.NewObjectArray.unwrap())(env, game_args.len() as i32, str_cls, ptr::null_mut());
        if arr.is_null() {
            (tbl.ExceptionClear.unwrap())(env);
            return Err("参数数组创建失败".into());
        }
        let mut cstrs: Vec<CString> = Vec::new();
        for (i, a) in game_args.iter().enumerate() {
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
        qlog("quartz", &format!(
            "[quartz] 调用 {} main（{} 个游戏参数）",
            main_class,
            game_args.len()
        ));
        let call = tbl.CallStaticVoidMethodA.unwrap();
        call(env, cls, mid, &arg_value);
        let threw = (tbl.ExceptionCheck.unwrap())(env) != 0;
        if threw {
            // 异常逃逸主类（MC 崩溃且未 System.exit 的路径）：打印后按 255 处理
            (tbl.ExceptionDescribe.unwrap())(env);
            (tbl.ExceptionClear.unwrap())(env);
            qlog("quartz", "[quartz] 主类抛出未捕获异常（详见上方 stderr）");
            return Ok(255);
        }
        qlog("quartz", "[quartz] main 返回（游戏正常退出）");
        Ok(0)
    }
}
