//! Quartz 游戏显示桥核心（Rust 重构 Pojav C 桥 libpojavexec.so）。
//!
//! 契约来源：gladiolus 官方 `lwjgl-glfw-classes.jar` 字节码（javap 反编译核对）。
//! 所有 JNI 导出符号严格使用 `Java_<类路径>_<方法>` 命名（JVM 按名绑定）；
//! pojav* 函数表符号为 camelCase（Functions.<clinit> 逐名 dlsym），经
//! `#[export_name]` 导出 —— snake_case 原名会导致 dlsym 落空、GLFW 类初始化
//! 直接 ExceptionInInitializerError（v0.1.6 实机崩溃根因）。
//!
//! 与 Pojav C 版的关键差异（v0.1.5 修复 gladiolus 发布包 jar↔so 版本错位崩溃）：
//! - 不做任何 RegisterNatives（旧 so 的 JNI_OnLoad 注册 jar 不存在的
//!   nativeSetUseInputStackQueue -> RegisterNatives NoSuchMethodError -> 退出码 255）
//! - 输入事件常驻队列模式（新 jar 的 glfwPollEvents 恒走 Start/Pump/StopPumping）
//! - ART/OpenJDK 双 JVM 经显式 C ABI init 传递（不依赖 POJAV_ENVIRON 指针技巧）；
//!   本库仅被游戏侧 OpenJDK dlopen，壳层（ART）经 dlsym 调 C ABI

use jni_sys::{
    jboolean, jbyteArray, jclass, jint, jlong, jmethodID, jobject, jstring, jvalue, JNIEnv, JavaVM,
    JNI_OK, JNI_VERSION_1_4,
};
use std::collections::HashMap;
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, AtomicPtr, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

// ---------------------------------------------------------------------------
// 事件常量（对齐 Pojav input_bridge_v3.c）
// ---------------------------------------------------------------------------

const EVENT_TYPE_CHAR: i32 = 1000;
const EVENT_TYPE_CHAR_MODS: i32 = 1001;
const EVENT_TYPE_CURSOR_ENTER: i32 = 1002;
const EVENT_TYPE_CURSOR_POS: i32 = 1003;
const EVENT_TYPE_KEY: i32 = 1005;
const EVENT_TYPE_MOUSE_BUTTON: i32 = 1006;
const EVENT_TYPE_SCROLL: i32 = 1007;
const EVENT_TYPE_WINDOW_SIZE: i32 = 1008;
const EVENT_TYPE_FRAMEBUFFER_SIZE: i32 = 1009;

const RENDERER_GL4ES: i32 = 1;
const RENDERER_VK_ZINK: i32 = 2;

// jar 静态初始化的直读缓冲尺寸（字节码核对）：keyDownBuffer=317、mouseDownBuffer=8。
// keyDownBuffer 下标 = GLFW 键码 - 32（GLFW_KEY_SPACE=32 -> 0）。
const KEY_BUF_SIZE: usize = 317;
const MOUSE_BUF_SIZE: usize = 8;
const WINDOW_FORMAT_RGBX_8888: i32 = 2;

fn log(line: &str) {
    // stderr 会被壳层 fd 重定向捕获进启动控制台（与 [quartz] 行同流）。
    eprintln!("[quartz-bridge] {line}");
}

/// JNI 导出函数内 panic 会 abort 整个进程（游戏与壳层同进程）——统一吞掉并留痕。
/// 宏体内整体 unsafe：这些导出函数本质全是 FFI 边界，unsafe 是默认语境。
macro_rules! guard {
    ($body:block) => {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            $body
        }));
    };
}

// ---------------------------------------------------------------------------
// JNI 指针辅助（jni-sys 0.3：JavaVM/JNIEnv 均为「函数表指针」别名）
// ---------------------------------------------------------------------------

type EnvPtr = *mut JNIEnv; // C JNIEnv*
type VmPtr = *mut JavaVM; // C JavaVM*

#[inline]
unsafe fn env_tbl(env: EnvPtr) -> &'static jni_sys::JNINativeInterface_ {
    &**env
}

#[inline]
unsafe fn vm_tbl(vm: VmPtr) -> &'static jni_sys::JNIInvokeInterface_ {
    &**vm
}

unsafe fn jstring_to_rust(env: EnvPtr, s: jstring) -> Option<String> {
    if s.is_null() {
        return None;
    }
    let tbl = env_tbl(env);
    let chars = (tbl.GetStringUTFChars.unwrap())(env, s, ptr::null_mut());
    if chars.is_null() {
        return None;
    }
    let text = CStr::from_ptr(chars).to_string_lossy().into_owned();
    (tbl.ReleaseStringUTFChars.unwrap())(env, s, chars);
    Some(text)
}

unsafe fn rust_to_jstring(env: EnvPtr, text: &str) -> Option<jstring> {
    let cs = CString::new(text).ok()?;
    let tbl = env_tbl(env);
    let out = (tbl.NewStringUTF.unwrap())(env, cs.as_ptr());
    if env_tbl(env).ExceptionCheck.unwrap()(env) != 0 {
        env_tbl(env).ExceptionClear.unwrap()(env);
        return None;
    }
    Some(out)
}

/// 线程附着的 OpenJDK（runtime）env：GetEnv -> EDETACHED 则 Attach。
unsafe fn runtime_env() -> Option<EnvPtr> {
    let s = state();
    let vm = s.rt_vm.load(Ordering::Acquire);
    if vm.is_null() {
        return None;
    }
    let iface = vm_tbl(vm);
    let mut env: EnvPtr = ptr::null_mut();
    let rc = (iface.GetEnv.unwrap())(vm, &mut env as *mut EnvPtr as *mut *mut c_void, JNI_VERSION_1_4);
    if rc == JNI_OK && !env.is_null() {
        return Some(env);
    }
    if rc == jni_sys::JNI_EDETACHED {
        let rc = (iface.AttachCurrentThread.unwrap())(
            vm,
            &mut env as *mut EnvPtr as *mut *mut c_void,
            ptr::null_mut(),
        );
        if rc == JNI_OK && !env.is_null() {
            return Some(env);
        }
    }
    None
}

/// 线程附着的 ART（壳层）env。
unsafe fn art_env() -> Option<EnvPtr> {
    let s = state();
    let vm = s.art_vm.load(Ordering::Acquire);
    if vm.is_null() {
        return None;
    }
    let iface = vm_tbl(vm);
    let mut env: EnvPtr = ptr::null_mut();
    let rc = (iface.GetEnv.unwrap())(vm, &mut env as *mut EnvPtr as *mut *mut c_void, JNI_VERSION_1_4);
    if rc == JNI_OK && !env.is_null() {
        return Some(env);
    }
    if rc == jni_sys::JNI_EDETACHED {
        let rc = (iface.AttachCurrentThread.unwrap())(
            vm,
            &mut env as *mut EnvPtr as *mut *mut c_void,
            ptr::null_mut(),
        );
        if rc == JNI_OK && !env.is_null() {
            return Some(env);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// 事件
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct Event {
    pub typ: i32,
    pub i1: i32,
    pub i2: i32,
    pub i3: i32,
    pub i4: i32,
    pub dx: f64,
    pub dy: f64,
}

impl Event {
    fn ints(typ: i32, i1: i32, i2: i32, i3: i32, i4: i32) -> Self {
        Event { typ, i1, i2, i3, i4, dx: 0.0, dy: 0.0 }
    }
}

// ---------------------------------------------------------------------------
// 全局状态
// ---------------------------------------------------------------------------

struct State {
    /// ART（壳层）JavaVM，由壳层经 quartz_bridge_init 显式注入。
    art_vm: AtomicPtr<JavaVM>,
    /// OpenJDK（游戏）JavaVM，由 JNI_OnLoad（jar 的 System.loadLibrary）设置。
    rt_vm: AtomicPtr<JavaVM>,
    /// com.quartz.launcher.QuartzBridge 全局引用 + 方法 ID（壳层 init 注入）。
    art_class: AtomicPtr<c_void>,
    m_on_grab: AtomicPtr<c_void>,
    m_clipboard: AtomicPtr<c_void>,
    /// org.lwjgl.glfw.GLFW 全局引用 + 回调方法 ID（JNI_OnLoad 缓存）。
    rt_glfw_class: AtomicPtr<c_void>,
    m_monitor_size: AtomicPtr<c_void>,
    m_window_size: AtomicPtr<c_void>,
    /// jar 直读按键缓冲地址（allocateDirect，GetDirectBufferAddress 取得）。
    key_buf: AtomicPtr<u8>,
    mouse_buf: AtomicPtr<u8>,

    /// 当前 ANativeWindow（壳层 Surface 交接）。
    window: AtomicPtr<ANativeWindow>,

    /// 输入事件队列（ART 线程生产 → 渲染线程 pump 消费）。
    queue: Mutex<Vec<Event>>,
    /// 本帧 pump 快照（Pojav StartPumping 语义）。
    pump_buf: Mutex<Vec<Event>>,

    cursor: Mutex<(f64, f64)>,
    cursor_last: Mutex<(f64, f64)>,
    showing: AtomicI64,
    input_ready: AtomicBool,
    grabbing: AtomicBool,
    cursor_entered: AtomicBool,
    upd_mouse: AtomicBool,
    upd_monitor: AtomicBool,
    monitor_consumed: AtomicBool,
    saved_w: AtomicI32,
    saved_h: AtomicI32,

    cb_char: AtomicUsize,
    cb_char_mods: AtomicUsize,
    cb_cursor_enter: AtomicUsize,
    cb_cursor_pos: AtomicUsize,
    cb_key: AtomicUsize,
    cb_mouse: AtomicUsize,
    cb_scroll: AtomicUsize,

    /// v0.2.5 直调修复：每个回调的 Java 对象全局引用（经 jar 的
    /// Callback.get(蹦床指针) 反查）+ 对应接口的 invoke methodID。
    /// 设备（bionic liblwjgl 的 libffi 闭包）首次触发回调即 SIGSEGV pc=0
    /// （两次崩溃同指纹：lr=匿名段+0x99a8，参数 (window,0,1,0)=左键按下），
    /// 主机复现同链路却稳定 —— 闭包路径在设备 bionic 环境不可用。
    /// 直调路径：pump 用 CallVoidMethodA（A 变体，无 varargs）直接调
    /// GLFWxxCallbackI.invoke，完全绕开 libffi 闭包；失败时回退旧闭包路径。
    cb_char_obj: AtomicPtr<c_void>,
    cb_char_mods_obj: AtomicPtr<c_void>,
    cb_cursor_enter_obj: AtomicPtr<c_void>,
    cb_cursor_pos_obj: AtomicPtr<c_void>,
    cb_key_obj: AtomicPtr<c_void>,
    cb_mouse_obj: AtomicPtr<c_void>,
    cb_scroll_obj: AtomicPtr<c_void>,
    /// org.lwjgl.system.Callback.get(J) 静态方法 ID + Callback 类全局引用（各解析一次）
    cb_get_mid: AtomicPtr<c_void>,
    cb_cbcls: AtomicPtr<c_void>,
    /// 各接口 invoke methodID（解析一次）
    mid_char: AtomicPtr<c_void>,
    mid_char_mods: AtomicPtr<c_void>,
    mid_cursor_enter: AtomicPtr<c_void>,
    mid_cursor_pos: AtomicPtr<c_void>,
    mid_key: AtomicPtr<c_void>,
    mid_mouse: AtomicPtr<c_void>,
    mid_scroll: AtomicPtr<c_void>,

    renderer: AtomicI32,
    windows: Mutex<HashMap<usize, Arc<RenderWindow>>>,
    main_window: Mutex<Option<usize>>,
}

static STATE: OnceLock<State> = OnceLock::new();

fn state() -> &'static State {
    STATE.get_or_init(|| State {
        art_vm: AtomicPtr::new(ptr::null_mut()),
        rt_vm: AtomicPtr::new(ptr::null_mut()),
        art_class: AtomicPtr::new(ptr::null_mut()),
        m_on_grab: AtomicPtr::new(ptr::null_mut()),
        m_clipboard: AtomicPtr::new(ptr::null_mut()),
        rt_glfw_class: AtomicPtr::new(ptr::null_mut()),
        m_monitor_size: AtomicPtr::new(ptr::null_mut()),
        m_window_size: AtomicPtr::new(ptr::null_mut()),
        key_buf: AtomicPtr::new(ptr::null_mut()),
        mouse_buf: AtomicPtr::new(ptr::null_mut()),
        window: AtomicPtr::new(ptr::null_mut()),
        queue: Mutex::new(Vec::new()),
        pump_buf: Mutex::new(Vec::new()),
        cursor: Mutex::new((0.0, 0.0)),
        cursor_last: Mutex::new((0.0, 0.0)),
        showing: AtomicI64::new(0),
        input_ready: AtomicBool::new(false),
        grabbing: AtomicBool::new(false),
        cursor_entered: AtomicBool::new(false),
        upd_mouse: AtomicBool::new(false),
        upd_monitor: AtomicBool::new(false),
        monitor_consumed: AtomicBool::new(false),
        saved_w: AtomicI32::new(0),
        saved_h: AtomicI32::new(0),
        cb_char: AtomicUsize::new(0),
        cb_char_mods: AtomicUsize::new(0),
        cb_cursor_enter: AtomicUsize::new(0),
        cb_cursor_pos: AtomicUsize::new(0),
        cb_key: AtomicUsize::new(0),
        cb_mouse: AtomicUsize::new(0),
        cb_scroll: AtomicUsize::new(0),
        cb_char_obj: AtomicPtr::new(ptr::null_mut()),
        cb_char_mods_obj: AtomicPtr::new(ptr::null_mut()),
        cb_cursor_enter_obj: AtomicPtr::new(ptr::null_mut()),
        cb_cursor_pos_obj: AtomicPtr::new(ptr::null_mut()),
        cb_key_obj: AtomicPtr::new(ptr::null_mut()),
        cb_mouse_obj: AtomicPtr::new(ptr::null_mut()),
        cb_scroll_obj: AtomicPtr::new(ptr::null_mut()),
        cb_get_mid: AtomicPtr::new(ptr::null_mut()),
        cb_cbcls: AtomicPtr::new(ptr::null_mut()),
        mid_char: AtomicPtr::new(ptr::null_mut()),
        mid_char_mods: AtomicPtr::new(ptr::null_mut()),
        mid_cursor_enter: AtomicPtr::new(ptr::null_mut()),
        mid_cursor_pos: AtomicPtr::new(ptr::null_mut()),
        mid_key: AtomicPtr::new(ptr::null_mut()),
        mid_mouse: AtomicPtr::new(ptr::null_mut()),
        mid_scroll: AtomicPtr::new(ptr::null_mut()),
        renderer: AtomicI32::new(RENDERER_GL4ES),
        windows: Mutex::new(HashMap::new()),
        main_window: Mutex::new(None),
    })
}

// ---------------------------------------------------------------------------
// ANativeWindow（libandroid）
// ---------------------------------------------------------------------------

#[repr(C)]
pub struct ANativeWindow {
    _p: [u8; 0],
}

extern "C" {
    fn ANativeWindow_fromSurface(env: *mut JNIEnv, surface: jobject) -> *mut ANativeWindow;
    fn ANativeWindow_acquire(window: *mut ANativeWindow);
    fn ANativeWindow_release(window: *mut ANativeWindow);
    fn ANativeWindow_getWidth(window: *mut ANativeWindow) -> i32;
    fn ANativeWindow_getHeight(window: *mut ANativeWindow) -> i32;
    fn ANativeWindow_setBuffersGeometry(
        window: *mut ANativeWindow,
        width: i32,
        height: i32,
        format: i32,
    ) -> i32;
}

// ---------------------------------------------------------------------------
// EGL（运行期 dlopen 渲染层 EGL 实现：POJAVEXEC_EGL，兜底 libEGL.so）
// ---------------------------------------------------------------------------

pub mod egl {
    use super::*;

    pub type EGLDisplay = *mut c_void;
    pub type EGLConfig = *mut c_void;
    pub type EGLContext = *mut c_void;
    pub type EGLSurface = *mut c_void;

    pub const EGL_TRUE: u32 = 1;
    pub const EGL_NO_DISPLAY: EGLDisplay = ptr::null_mut();
    pub const EGL_NO_SURFACE: EGLSurface = ptr::null_mut();
    pub const EGL_NO_CONTEXT: EGLContext = ptr::null_mut();
    pub const EGL_DEFAULT_DISPLAY: *mut c_void = ptr::null_mut();
    pub const EGL_OPENGL_ES_API: u32 = 0x30A0;
    pub const EGL_OPENGL_API: u32 = 0x30A2;
    pub const EGL_SURFACE_TYPE: i32 = 0x1233;
    pub const EGL_WINDOW_BIT: i32 = 0x0004;
    pub const EGL_PBUFFER_BIT: i32 = 0x0001;
    pub const EGL_BLUE_SIZE: i32 = 0x1222;
    pub const EGL_GREEN_SIZE: i32 = 0x1223;
    pub const EGL_RED_SIZE: i32 = 0x1224;
    pub const EGL_ALPHA_SIZE: i32 = 0x1221;
    pub const EGL_DEPTH_SIZE: i32 = 0x1225;
    pub const EGL_RENDERABLE_TYPE: i32 = 0x1240;
    pub const EGL_OPENGL_ES2_BIT: i32 = 0x0004;
    pub const EGL_OPENGL_ES3_BIT: i32 = 0x0040;
    pub const EGL_NONE: i32 = 0x3038;
    pub const EGL_CONTEXT_CLIENT_VERSION: i32 = 0x3098;
    // EGL_KHR_create_context（桌面 GL 上下文：Zink/Mesa 真桌面 GL 路径）
    pub const EGL_CONTEXT_MAJOR_VERSION_KHR: i32 = 0x30FB;
    pub const EGL_CONTEXT_MINOR_VERSION_KHR: i32 = 0x30FC;
    pub const EGL_CONTEXT_OPENGL_PROFILE_MASK_KHR: i32 = 0x30FD;
    pub const EGL_CONTEXT_OPENGL_CORE_PROFILE_BIT: i32 = 0x0001;
    pub const EGL_NATIVE_VISUAL_ID: i32 = 0x302E;
    pub const EGL_BAD_SURFACE: i32 = 0x300D;
    pub const EGL_WIDTH: i32 = 0x3057;
    pub const EGL_HEIGHT: i32 = 0x3056;

    type EglMakeCurrentFn =
        unsafe extern "system" fn(EGLDisplay, EGLSurface, EGLSurface, EGLContext) -> u32;
    type EglDisplayFn = unsafe extern "system" fn(*mut c_void) -> EGLDisplay;
    type EglInitializeFn = unsafe extern "system" fn(EGLDisplay, *mut i32, *mut i32) -> u32;
    type EglChooseConfigFn =
        unsafe extern "system" fn(EGLDisplay, *const i32, *mut EGLConfig, i32, *mut i32) -> u32;
    type EglGetConfigsFn =
        unsafe extern "system" fn(EGLDisplay, *mut EGLConfig, i32, *mut i32) -> u32;
    type EglConfigAttribFn = unsafe extern "system" fn(EGLDisplay, EGLConfig, i32, *mut i32) -> u32;
    type EglBindApiFn = unsafe extern "system" fn(u32) -> u32;
    type EglCreateContextFn =
        unsafe extern "system" fn(EGLDisplay, EGLConfig, EGLContext, *const i32) -> EGLContext;
    type EglCreateWindowSurfaceFn =
        unsafe extern "system" fn(EGLDisplay, EGLConfig, *mut c_void, *const i32) -> EGLSurface;
    type EglCreatePbufferSurfaceFn =
        unsafe extern "system" fn(EGLDisplay, EGLConfig, *const i32) -> EGLSurface;
    type EglDestroyFn = unsafe extern "system" fn(EGLDisplay, EGLSurface) -> u32;
    type EglTerminateFn = unsafe extern "system" fn(EGLDisplay) -> u32;
    type EglSwapBuffersFn = unsafe extern "system" fn(EGLDisplay, EGLSurface) -> u32;
    type EglSwapIntervalFn = unsafe extern "system" fn(EGLDisplay, i32) -> u32;
    type EglQuerySurfaceFn = unsafe extern "system" fn(EGLDisplay, EGLSurface, i32, *mut i32) -> u32;
    type EglGetErrorFn = unsafe extern "system" fn() -> i32;

    #[derive(Clone, Copy)]
    pub struct EglApi {
        pub make_current: Option<EglMakeCurrentFn>,
        pub destroy_surface: Option<EglDestroyFn>,
        pub destroy_context: Option<EglDestroyFn>,
        pub terminate: Option<EglTerminateFn>,
        pub get_display: Option<EglDisplayFn>,
        pub initialize: Option<EglInitializeFn>,
        pub choose_config: Option<EglChooseConfigFn>,
        pub get_configs: Option<EglGetConfigsFn>,
        pub get_config_attrib: Option<EglConfigAttribFn>,
        pub bind_api: Option<EglBindApiFn>,
        pub create_context: Option<EglCreateContextFn>,
        pub create_window_surface: Option<EglCreateWindowSurfaceFn>,
        pub create_pbuffer_surface: Option<EglCreatePbufferSurfaceFn>,
        pub swap_buffers: Option<EglSwapBuffersFn>,
        pub swap_interval: Option<EglSwapIntervalFn>,
        pub query_surface: Option<EglQuerySurfaceFn>,
        pub get_error: Option<EglGetErrorFn>,
    }

    /// dlopen 渲染层 EGL 实现并解析符号（Pojav egl_loader.c 的 dlsym_EGL 语义）。
    pub fn load_api() -> Option<EglApi> {
        // host-sim：优先使用内置 EGL-on-GLX 仿真（POJAVEXEC_EGL=sim 显式选择，
        // 未配置时若系统无渲染层 EGL 也落到仿真）。
        #[cfg(all(feature = "host-sim", not(target_os = "android")))]
        {
            let configured = std::env::var("POJAVEXEC_EGL").unwrap_or_default();
            if configured.is_empty() || configured == "sim" {
                log("EGL 使用主机仿真实现（EGL-on-GLX，见 egl_sim.rs）");
                return Some(crate::egl_sim::api());
            }
        }

        extern "C" {
            fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
            fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
            fn dlerror() -> *const c_char;
        }
        const RTLD_LAZY: c_int = 1;
        const RTLD_LOCAL: c_int = 0;

        let configured = std::env::var("POJAVEXEC_EGL").unwrap_or_default();
        let path = if configured.is_empty() { "libEGL.so".to_string() } else { configured };
        let cpath = CString::new(path.clone()).ok()?;
        let handle = unsafe { dlopen(cpath.as_ptr(), RTLD_LAZY | RTLD_LOCAL) };
        if handle.is_null() {
            // v9：带 dlerror 详情（缺哪个 NEEDED 一眼可见，不再对着路径猜）
            let detail = unsafe {
                let p = dlerror();
                if p.is_null() {
                    "（无 dlerror 详情）".to_string()
                } else {
                    CStr::from_ptr(p).to_string_lossy().into_owned()
                }
            };
            log(&format!("EGL 库加载失败：{path} —— {detail}"));
            return None;
        }
        let mut api = EglApi {
            make_current: None,
            destroy_surface: None,
            destroy_context: None,
            terminate: None,
            get_display: None,
            initialize: None,
            choose_config: None,
            get_configs: None,
            get_config_attrib: None,
            bind_api: None,
            create_context: None,
            create_window_surface: None,
            create_pbuffer_surface: None,
            swap_buffers: None,
            swap_interval: None,
            query_surface: None,
            get_error: None,
        };
        macro_rules! get {
            ($field:ident, $name:literal, $ty:ty) => {{
                let cname = CString::new($name).unwrap();
                let p = unsafe { dlsym(handle, cname.as_ptr()) };
                if p.is_null() {
                    log(&concat!("EGL 符号缺失：", $name));
                    return None;
                }
                api.$field = Some(unsafe { std::mem::transmute::<*mut c_void, $ty>(p) });
            }};
        }
        get!(make_current, "eglMakeCurrent", EglMakeCurrentFn);
        get!(destroy_surface, "eglDestroySurface", EglDestroyFn);
        get!(destroy_context, "eglDestroyContext", EglDestroyFn);
        get!(terminate, "eglTerminate", EglTerminateFn);
        get!(get_display, "eglGetDisplay", EglDisplayFn);
        get!(initialize, "eglInitialize", EglInitializeFn);
        get!(choose_config, "eglChooseConfig", EglChooseConfigFn);
        get!(get_configs, "eglGetConfigs", EglGetConfigsFn);
        get!(get_config_attrib, "eglGetConfigAttrib", EglConfigAttribFn);
        get!(bind_api, "eglBindAPI", EglBindApiFn);
        get!(create_context, "eglCreateContext", EglCreateContextFn);
        get!(create_window_surface, "eglCreateWindowSurface", EglCreateWindowSurfaceFn);
        get!(create_pbuffer_surface, "eglCreatePbufferSurface", EglCreatePbufferSurfaceFn);
        get!(swap_buffers, "eglSwapBuffers", EglSwapBuffersFn);
        get!(swap_interval, "eglSwapInterval", EglSwapIntervalFn);
        get!(query_surface, "eglQuerySurface", EglQuerySurfaceFn);
        get!(get_error, "eglGetError", EglGetErrorFn);
        Some(api)
    }

    pub const STATE_ALIVE: i32 = 0;
    pub const STATE_NEW_WINDOW: i32 = 1;

    /// GL 渲染窗口（Pojav gl_render_window_t 语义）。
    /// 裸指针 Cell：RenderWindow 仅经由 windows/main_window 锁访问（渲染线程 +
    /// 壳层 UI 线程），字段访问点全部持锁 —— Send/Sync 手工声明是受控的。
    pub struct RenderWindow {
        pub state: std::cell::Cell<i32>,
        pub config: std::cell::Cell<EGLConfig>,
        pub context: std::cell::Cell<EGLContext>,
        pub surface: std::cell::Cell<EGLSurface>,
        pub format: std::cell::Cell<i32>,
        pub native_surface: std::cell::Cell<*mut ANativeWindow>,
        pub new_native_surface: std::cell::Cell<*mut ANativeWindow>,
    }

    unsafe impl Send for RenderWindow {}
    unsafe impl Sync for RenderWindow {}
}

pub use egl::RenderWindow;

/// EGL 句柄存 usize（raw 指针非 Send/Sync，无法进 static Mutex；使用点再回转）。
static EGL_STATE: Mutex<Option<(egl::EglApi, usize)>> = Mutex::new(None);

// ---------------------------------------------------------------------------
// GLFW Java 侧回调（internalChangeMonitorSize / internalWindowSizeChanged）
// ---------------------------------------------------------------------------

unsafe fn call_glfw_void(method_slot: &AtomicPtr<c_void>, label: &str, args: &[jvalue]) {
    let Some(env) = (unsafe { runtime_env() }) else { return };
    unsafe {
        let cls = state().rt_glfw_class.load(Ordering::Acquire) as jclass;
        let mid = method_slot.load(Ordering::Acquire) as jmethodID;
        if cls.is_null() || mid.is_null() {
            return;
        }
        let tbl = env_tbl(env);
        let Some(call) = tbl.CallStaticVoidMethodA else { return };
        if tbl.ExceptionCheck.unwrap()(env) != 0 {
            tbl.ExceptionClear.unwrap()(env);
        }
        call(env, cls, mid, args.as_ptr());
        if tbl.ExceptionCheck.unwrap()(env) != 0 {
            tbl.ExceptionClear.unwrap()(env);
            log(&format!("GLFW 回调异常已清除（{label}）"));
        }
    }
}

#[cfg(all(feature = "host-sim", not(target_os = "android")))]
fn s2_cb(name: &str) -> usize {
    let s = state();
    match name {
        "char" => s.cb_char.load(Ordering::Acquire),
        "curpos" => s.cb_cursor_pos.load(Ordering::Acquire),
        "key" => s.cb_key.load(Ordering::Acquire),
        "mouse" => s.cb_mouse.load(Ordering::Acquire),
        "scroll" => s.cb_scroll.load(Ordering::Acquire),
        _ => 0,
    }
}

fn update_monitor_size_call() {
    let s = state();
    let w = s.saved_w.load(Ordering::Acquire);
    let h = s.saved_h.load(Ordering::Acquire);
    let args = [jvalue { i: w }, jvalue { i: h }];
    unsafe {
        call_glfw_void(&s.m_monitor_size, "internalChangeMonitorSize", &args);
    }
}

fn update_window_size_call(window: usize) {
    let args = [jvalue { j: window as jlong }];
    unsafe {
        call_glfw_void(&state().m_window_size, "internalWindowSizeChanged", &args);
    }
}

// ---------------------------------------------------------------------------
// pojav* 函数表（jar 的 GLFW$Functions 逐名解析；ABI 与字节码核对）
// ---------------------------------------------------------------------------

/// `int pojavInit(void)` —— glfwInit。
/// 注意：jar 的 Functions 表经 dlsym 解析 camelCase 符号，必须用
/// #[export_name] 导出（Rust 默认 #[no_mangle] 只保留 snake_case 原名，
/// v0.1.6 修复「A required function is missing: pojavInit」崩溃根因）。
#[export_name = "pojavInit"]
pub extern "C" fn pojav_init() -> c_int {
    let mut ok = false;
    guard!({
        let s = state();
        let mut window = s.window.load(Ordering::Acquire);
        // v0.1.7 崩溃修复 ①：横屏旋转会先销毁再重建 Surface（surfaceDestroyed ->
        // surfaceCreated），glfwInit 若恰好在窗口空档被调用，之前直接以 0x0 窗口
        // 继续，后续 eglCreateWindowSurface 必败 —— 这里最多等 8s 再初始化。
        if window.is_null() {
            log("pojavInit：ANativeWindow 尚未就绪（等待壳层 Surface）");
            for i in 0..160 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                window = s.window.load(Ordering::Acquire);
                if !window.is_null() {
                    break;
                }
                if i % 40 == 39 {
                    log("pojavInit：仍在等待 Surface…（8s 窗口期已过 2s）");
                }
            }
        }
        let (mut w, mut h) = (0, 0);
        if !window.is_null() {
            ANativeWindow_acquire(window);
            w = ANativeWindow_getWidth(window);
            h = ANativeWindow_getHeight(window);
            s.saved_w.store(w, Ordering::Release);
            s.saved_h.store(h, Ordering::Release);
            ANativeWindow_setBuffersGeometry(window, w, h, WINDOW_FORMAT_RGBX_8888);
            s.upd_monitor.store(true, Ordering::Release);
            s.monitor_consumed.store(false, Ordering::Release);
        } else {
            log("pojavInit：等待 Surface 超时（8s），以无窗口模式继续");
        }
        let renderer_env = std::env::var("POJAV_RENDERER").unwrap_or_default();
        if renderer_env.starts_with("vulkan_zink") {
            // Pojav 的 vulkan_zink 走 Vulkan WSI 直连（VkSurface/VkSwapchain），
            // 本桥未实现该路径；Mesa 系渲染器（FCL/Zalith 的 Zink/Kopper pack）
            // 自带完整 EGL 实现（libEGL_mesa.so），经 POJAVEXEC_EGL 走本桥
            // 标准 EGL 窗口路径即可 —— 不再中断，继续 EGL 初始化。
            log("渲染桥：vulkan_zink（Vulkan WSI 直连）未启用，回退 EGL 路径");
        }
        s.renderer.store(RENDERER_GL4ES, Ordering::Release);
        if let Some(api) = egl::load_api() {
            let display = (api.get_display.unwrap())(egl::EGL_DEFAULT_DISPLAY);
            if display != egl::EGL_NO_DISPLAY
                && (api.initialize.unwrap())(display, ptr::null_mut(), ptr::null_mut()) == egl::EGL_TRUE
            {
                *EGL_STATE.lock().unwrap() = Some((api, display as usize));
                log("EGL 桥就绪（渲染层实现库）");
                ok = true;
            } else {
                let err = (api.get_error.unwrap())();
                log(&format!("EGL 初始化失败：{err:#06x}"));
            }
        }
        log(&format!("pojavInit：窗口 {w}x{h}"));
    });
    if ok { 1 } else { 1 } // 与 Pojav 一致恒返 1（失败细节已写日志，MC 会再报错）
}

/// `void* pojavCreateContext(void* share)` —— nglfwCreateContext。
#[export_name = "pojavCreateContext"]
pub extern "C" fn pojav_create_context(share: *mut c_void) -> *mut c_void {
    let mut result: *mut c_void = ptr::null_mut();
    guard!({
        let s = state();
        let guard = EGL_STATE.lock().unwrap();
        let Some((api, display)) = guard.as_ref() else {
            log("pojavCreateContext：EGL 未初始化");
            return;
        };
        let display = *display as egl::EGLDisplay;
        // v0.1.7 崩溃修复 ②：部分渲染层 EGL（gl4es 等）不支持 WINDOW|PBUFFER
        // 组合属性（eglChooseConfig 返回 EGL_FALSE/BAD_CONFIG，configs=0），
        // 之前直接失败导致 MC 拿不到上下文退出 —— 改为逐级放宽的回退阶梯。
        //
        // v0.1.9-c 实机日志修复：MobileGlues 的 EGL 实现只暴露 ES3 位 config，
        // 全 ES2 阶梯全部 EGL_BAD_MATCH(0x3004)/configs=0，MC 拿不到 context 后
        // Render thread 空指针 SIGSEGV —— 阶梯扩展为 ES2×4 + ES3×3 + 无约束，
        // 最后 eglGetConfigs 兜底取第一个 config；匹配后读 config 的
        // RENDERABLE_TYPE 位自适应决定 GLES 主版本。
        const A_ES2: i32 = egl::EGL_OPENGL_ES2_BIT;
        const A_ES3: i32 = egl::EGL_OPENGL_ES3_BIT;
        struct Ladder {
            attrs: [i32; 16],
            es_bit: i32,
        }
        let ladders = [
            // #0 ES2 完整需求：RGBA8888 + 深度 24 + WINDOW|PBUFFER
            Ladder {
                attrs: [
                    egl::EGL_BLUE_SIZE, 8,
                    egl::EGL_GREEN_SIZE, 8,
                    egl::EGL_RED_SIZE, 8,
                    egl::EGL_ALPHA_SIZE, 8,
                    egl::EGL_DEPTH_SIZE, 24,
                    egl::EGL_SURFACE_TYPE, egl::EGL_WINDOW_BIT | egl::EGL_PBUFFER_BIT,
                    egl::EGL_RENDERABLE_TYPE, A_ES2,
                    egl::EGL_NONE, 0,
                ],
                es_bit: A_ES2,
            },
            // #1 ES2 去 PBUFFER 位（窗口渲染主路径）
            Ladder {
                attrs: [
                    egl::EGL_BLUE_SIZE, 8,
                    egl::EGL_GREEN_SIZE, 8,
                    egl::EGL_RED_SIZE, 8,
                    egl::EGL_ALPHA_SIZE, 8,
                    egl::EGL_DEPTH_SIZE, 24,
                    egl::EGL_SURFACE_TYPE, egl::EGL_WINDOW_BIT,
                    egl::EGL_RENDERABLE_TYPE, A_ES2,
                    egl::EGL_NONE, 0,
                ],
                es_bit: A_ES2,
            },
            // #2 ES2 深度降为 16
            Ladder {
                attrs: [
                    egl::EGL_BLUE_SIZE, 8,
                    egl::EGL_GREEN_SIZE, 8,
                    egl::EGL_RED_SIZE, 8,
                    egl::EGL_ALPHA_SIZE, 8,
                    egl::EGL_DEPTH_SIZE, 16,
                    egl::EGL_SURFACE_TYPE, egl::EGL_WINDOW_BIT,
                    egl::EGL_RENDERABLE_TYPE, A_ES2,
                    egl::EGL_NONE, 0,
                ],
                es_bit: A_ES2,
            },
            // #3 ES2 仅要求可渲染 + 窗口位（颜色/深度交给 EGL 默认值）
            Ladder {
                attrs: [
                    egl::EGL_SURFACE_TYPE, egl::EGL_WINDOW_BIT,
                    egl::EGL_RENDERABLE_TYPE, A_ES2,
                    egl::EGL_NONE, 0,
                    egl::EGL_NONE, 0,
                    egl::EGL_NONE, 0,
                    egl::EGL_NONE, 0,
                    egl::EGL_NONE, 0,
                    egl::EGL_NONE, 0,
                ],
                es_bit: A_ES2,
            },
            // #4 ES3 完整需求（MobileGlues 主路径）
            Ladder {
                attrs: [
                    egl::EGL_BLUE_SIZE, 8,
                    egl::EGL_GREEN_SIZE, 8,
                    egl::EGL_RED_SIZE, 8,
                    egl::EGL_ALPHA_SIZE, 8,
                    egl::EGL_DEPTH_SIZE, 24,
                    egl::EGL_SURFACE_TYPE, egl::EGL_WINDOW_BIT | egl::EGL_PBUFFER_BIT,
                    egl::EGL_RENDERABLE_TYPE, A_ES3,
                    egl::EGL_NONE, 0,
                ],
                es_bit: A_ES3,
            },
            // #5 ES3 去 PBUFFER 位
            Ladder {
                attrs: [
                    egl::EGL_BLUE_SIZE, 8,
                    egl::EGL_GREEN_SIZE, 8,
                    egl::EGL_RED_SIZE, 8,
                    egl::EGL_ALPHA_SIZE, 8,
                    egl::EGL_DEPTH_SIZE, 24,
                    egl::EGL_SURFACE_TYPE, egl::EGL_WINDOW_BIT,
                    egl::EGL_RENDERABLE_TYPE, A_ES3,
                    egl::EGL_NONE, 0,
                ],
                es_bit: A_ES3,
            },
            // #6 ES3 仅可渲染 + 窗口位
            Ladder {
                attrs: [
                    egl::EGL_SURFACE_TYPE, egl::EGL_WINDOW_BIT,
                    egl::EGL_RENDERABLE_TYPE, A_ES3,
                    egl::EGL_NONE, 0,
                    egl::EGL_NONE, 0,
                    egl::EGL_NONE, 0,
                    egl::EGL_NONE, 0,
                    egl::EGL_NONE, 0,
                    egl::EGL_NONE, 0,
                ],
                es_bit: A_ES3,
            },
            // #7 空属性（默认 config）：attrib_list 只含 EGL_NONE 是 spec 允许的
            // 合规形态，任何实现都必须接受。v0.2.4 新增 —— 实机日志（MobileGlues）
            // 显示 #0-#6 连「SURFACE_TYPE+RENDERABLE_TYPE」最简集都以
            // EGL_BAD_ATTRIBUTE(0x3004) 拒绝，此前直接跌入 eglGetConfigs 兜底
            // 拿到无语义项 config（renderable=0x0000）。空属性档优先于盲目兜底。
            Ladder {
                attrs: [
                    egl::EGL_NONE, 0,
                    0, 0,
                    0, 0,
                    0, 0,
                    0, 0,
                    0, 0,
                    0, 0,
                    0, 0,
                ],
                es_bit: A_ES3,
            },
        ];
        let mut config: egl::EGLConfig = ptr::null_mut();
        let mut format: i32 = 0;
        let mut matched_es_bit: i32 = 0;
        let mut matched_tier: i32 = -1;
        for (tier, lad) in ladders.iter().enumerate() {
            let mut num_configs: i32 = 0;
            let rc = (api.choose_config.unwrap())(display, lad.attrs.as_ptr(), ptr::null_mut(), 0, &mut num_configs);
            if rc == egl::EGL_TRUE && num_configs > 0 {
                let mut got: i32 = 0;
                (api.choose_config.unwrap())(display, lad.attrs.as_ptr(), &mut config, 1, &mut got);
                (api.get_config_attrib.unwrap())(display, config, egl::EGL_NATIVE_VISUAL_ID, &mut format);
                matched_es_bit = lad.es_bit;
                matched_tier = tier as i32;
                if tier > 0 {
                    log(&format!("eglChooseConfig：使用回退阶梯 #{tier}（configs={num_configs}）"));
                }
                break;
            }
            log(&format!(
                "eglChooseConfig 阶梯 #{tier} 未命中：rc={rc} configs={num_configs} err={:#06x}",
                (api.get_error.unwrap())()
            ));
        }
        if config.is_null() {
            // v0.1.9-c 兜底：全阶梯未命中时 eglGetConfigs 取第一个 config。
            // v9 增强：先扫描全部 config，选第一个「可渲染 ES2/ES3 且支持窗口
            // 表面」的 —— 首个 config 常是 renderable=0 的无语义项
            //（实机日志：MobileGlues 兜底拿到 renderable=0x0000，后续渲染
            // 线程空指针 SIGSEGV），盲目取第一个就是踩雷。
            log("eglChooseConfig 全阶梯未命中，eglGetConfigs 兜底");
            let mut num_configs: i32 = 0;
            if (api.get_configs.unwrap())(display, ptr::null_mut(), 0, &mut num_configs)
                == egl::EGL_TRUE
                && num_configs > 0
            {
                let n = num_configs.min(64) as usize;
                let mut buf: Vec<egl::EGLConfig> = vec![ptr::null_mut(); n];
                let mut got: i32 = 0;
                (api.get_configs.unwrap())(display, buf.as_mut_ptr(), n as i32, &mut got);
                let mut picked: Option<(egl::EGLConfig, i32)> = None;
                let mut scanned: Vec<String> = Vec::new();
                for cfg in buf.iter().take(got.max(0) as usize) {
                    if cfg.is_null() {
                        continue;
                    }
                    let mut renderable: i32 = 0;
                    let mut surface_bits: i32 = 0;
                    (api.get_config_attrib.unwrap())(display, *cfg, egl::EGL_RENDERABLE_TYPE, &mut renderable);
                    (api.get_config_attrib.unwrap())(display, *cfg, egl::EGL_SURFACE_TYPE, &mut surface_bits);
                    scanned.push(format!("{:#x}/{:#x}", renderable, surface_bits));
                    if renderable & (egl::EGL_OPENGL_ES3_BIT | egl::EGL_OPENGL_ES2_BIT) != 0
                        && surface_bits & egl::EGL_WINDOW_BIT != 0
                        && picked.is_none()
                    {
                        picked = Some((*cfg, renderable));
                    }
                }
                match picked {
                    Some((cfg, renderable)) => {
                        config = cfg;
                        matched_es_bit = renderable;
                        log(&format!(
                            "eglGetConfigs 兜底选中可渲染 config（共 {} 个：{}）",
                            scanned.len(),
                            scanned.join(",")
                        ));
                    }
                    None => {
                        config = buf[0];
                        (api.get_config_attrib.unwrap())(display, config, egl::EGL_RENDERABLE_TYPE, &mut matched_es_bit);
                        log(&format!(
                            "eglGetConfigs 兜底无可渲染 config，取第一个（共 {} 个：{}）",
                            scanned.len(),
                            scanned.join(",")
                        ));
                    }
                }
                (api.get_config_attrib.unwrap())(display, config, egl::EGL_NATIVE_VISUAL_ID, &mut format);
                matched_tier = -2;
            }
        }
        if config.is_null() {
            let err = (api.get_error.unwrap())();
            log(&format!("eglChooseConfig 失败：{err:#06x}（含 eglGetConfigs 兜底仍无 config）"));
            return;
        }

        let renderer_env = std::env::var("POJAV_RENDERER").unwrap_or_default();
        let desktop_gl = renderer_env.contains("desktopgl");
        let bind = if desktop_gl {
            (api.bind_api.unwrap())(egl::EGL_OPENGL_API)
        } else {
            (api.bind_api.unwrap())(egl::EGL_OPENGL_ES_API)
        };
        if bind != egl::EGL_TRUE {
            log("eglBindAPI 失败");
        }

        // 共享上下文（share 句柄查表；两分支共用）
        let share_ctx = if share.is_null() {
            egl::EGL_NO_CONTEXT
        } else {
            let windows = s.windows.lock().unwrap();
            match windows.get(&(share as usize)) {
                Some(w) => w.context.get(),
                None => egl::EGL_NO_CONTEXT,
            }
        };
        // GLES 主版本：LIBGL_ES 显式指定优先；否则按匹配 config 的
        // RENDERABLE_TYPE 位自适应（ES3 config 用 3，否则 2）—— MobileGlues
        // 等仅暴露 ES3 config 的渲染层由此自动落对，不再硬编码 renderer 名。
        // 桌面 GL（Zink/Kopper）：走 EGL_KHR_create_context 主/次版本阶梯，
        // MC 1.17+ 需要核心 3.2+，从 4.6 逐级降档尝试。
        // v9 兜底：桌面 GL 全阶梯未命中时不再直接放弃（返回空句柄会让 LWJGL
        // 拿 NULL 函数指针，Render thread pc=0 SIGSEGV）—— 回退 GLES 上下文
        //（zink 也支持 ES3.1），至少拿到可渲染路径并留下完整日志。
        let (context, es_label): (egl::EGLContext, String) = {
            let mut got = egl::EGL_NO_CONTEXT;
            let mut label = String::new();
            if desktop_gl {
                const PROFILES: [(i32, i32); 3] = [(4, 6), (3, 3), (3, 2)];
                for (major, minor) in PROFILES {
                    let attrs = [
                        egl::EGL_CONTEXT_MAJOR_VERSION_KHR,
                        major,
                        egl::EGL_CONTEXT_MINOR_VERSION_KHR,
                        minor,
                        egl::EGL_CONTEXT_OPENGL_PROFILE_MASK_KHR,
                        egl::EGL_CONTEXT_OPENGL_CORE_PROFILE_BIT,
                        egl::EGL_NONE,
                        0,
                    ];
                    let ctx = (api.create_context.unwrap())(display, config, share_ctx, attrs.as_ptr());
                    if ctx != egl::EGL_NO_CONTEXT {
                        got = ctx;
                        label = format!("GL{major}.{minor} core");
                        break;
                    }
                    let err = (api.get_error.unwrap())();
                    log(&format!("桌面 GL {major}.{minor} 上下文创建失败：{err:#06x}，降档重试"));
                }
            }
            if got == egl::EGL_NO_CONTEXT {
                if desktop_gl {
                    log("桌面 GL 全阶梯未命中（设备 Vulkan 驱动或 Mesa pack 异常），回退 GLES 上下文");
                }
                let es = std::env::var("LIBGL_ES")
                    .ok()
                    .and_then(|v| v.trim().parse::<i32>().ok())
                    .filter(|v| (1..=3).contains(v))
                    .unwrap_or(if matched_es_bit & egl::EGL_OPENGL_ES3_BIT != 0 { 3 } else { 2 });
                let ctx_attrs = [egl::EGL_CONTEXT_CLIENT_VERSION, es, egl::EGL_NONE];
                let context =
                    (api.create_context.unwrap())(display, config, share_ctx, ctx_attrs.as_ptr());
                if context == egl::EGL_NO_CONTEXT {
                    let err = (api.get_error.unwrap())();
                    log(&format!("eglCreateContext 失败：{err:#06x}（client_version={es}）"));
                    return;
                }
                (context, format!("GLES{es}"))
            } else {
                (got, label)
            }
        };
        let bundle = Arc::new(RenderWindow {
            state: std::cell::Cell::new(egl::STATE_ALIVE),
            config: std::cell::Cell::new(config),
            context: std::cell::Cell::new(context),
            surface: std::cell::Cell::new(ptr::null_mut()),
            format: std::cell::Cell::new(format),
            native_surface: std::cell::Cell::new(ptr::null_mut()),
            new_native_surface: std::cell::Cell::new(ptr::null_mut()),
        });
        let handle = Arc::as_ptr(&bundle) as usize;
        s.windows.lock().unwrap().insert(handle, bundle);
        log(&format!(
            "pojavCreateContext：窗口 {handle:#x}（{es_label}，阶梯#{matched_tier}，renderable={matched_es_bit:#06x}）"
        ));
        result = handle as *mut c_void;
    });
    result
}

thread_local! {
    static CURRENT_WINDOW: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// `void* pojavGetCurrentContext(void)`。
#[export_name = "pojavGetCurrentContext"]
pub extern "C" fn pojav_get_current_context() -> *mut c_void {
    let cur = CURRENT_WINDOW.with(|c| c.get());
    if cur == 0 { ptr::null_mut() } else { cur as *mut c_void }
}

/// Pojav gl_bridge.c gl_swap_surface：换绑新 Surface 或 1x1 pbuffer 兜底。
unsafe fn gl_swap_surface(bundle: &Arc<RenderWindow>, display: egl::EGLDisplay, api: &egl::EglApi) {
    let old_native = bundle.native_surface.get();
    if !old_native.is_null() {
        ANativeWindow_release(old_native);
        bundle.native_surface.set(ptr::null_mut());
    }
    if !bundle.surface.get().is_null() {
        (api.destroy_surface.unwrap())(display, bundle.surface.get());
        bundle.surface.set(ptr::null_mut());
    }
    let new_native = bundle.new_native_surface.get();
    if !new_native.is_null() {
        bundle.new_native_surface.set(ptr::null_mut());
        bundle.native_surface.set(new_native);
        ANativeWindow_acquire(new_native);
        ANativeWindow_setBuffersGeometry(new_native, 0, 0, bundle.format.get());
        let surface =
            (api.create_window_surface.unwrap())(display, bundle.config.get(), new_native as *mut c_void, ptr::null());
        bundle.surface.set(surface);
        if surface.is_null() {
            let err = (api.get_error.unwrap())();
            log(&format!("eglCreateWindowSurface 失败：{err:#06x}"));
        }
    } else {
        log("无新 Surface，切 1x1 pbuffer 兜底");
        let pbuf_attrs = [egl::EGL_WIDTH, 1, egl::EGL_HEIGHT, 1, egl::EGL_NONE];
        bundle
            .surface
            .set((api.create_pbuffer_surface.unwrap())(display, bundle.config.get(), pbuf_attrs.as_ptr()));
    }
}

/// `void pojavMakeCurrent(void* window)`。
#[export_name = "pojavMakeCurrent"]
pub extern "C" fn pojav_make_current(window: *mut c_void) {
    guard!({
        let s = state();
        let handle = window as usize;
        if window.is_null() {
            let guard = EGL_STATE.lock().unwrap();
            if let Some((api, display)) = guard.as_ref() {
                let display = *display as egl::EGLDisplay;
                (api.make_current.unwrap())(display, egl::EGL_NO_SURFACE, egl::EGL_NO_SURFACE, egl::EGL_NO_CONTEXT);
            }
            CURRENT_WINDOW.with(|c| c.set(0));
            return;
        }
        let Some(bundle) = s.windows.lock().unwrap().get(&handle).cloned() else { return };
        let guard = EGL_STATE.lock().unwrap();
        let Some((api, display)) = guard.as_ref() else { return };
        let display = *display as egl::EGLDisplay;
        let mut first_main = false;
        {
            let mut main_slot = s.main_window.lock().unwrap();
            if main_slot.is_none() {
                *main_slot = Some(handle);
                first_main = true;
                bundle.new_native_surface.set(s.window.load(Ordering::Acquire));
                log(&format!("主窗口 bundle = {handle:#x}"));
            }
        }
        if bundle.surface.get().is_null() {
            gl_swap_surface(&bundle, display, api);
        }
        if (api.make_current.unwrap())(display, bundle.surface.get(), bundle.surface.get(), bundle.context.get())
            == egl::EGL_TRUE
        {
            CURRENT_WINDOW.with(|c| c.set(handle));
        } else {
            let err = (api.get_error.unwrap())();
            log(&format!("eglMakeCurrent 失败：{err:#06x}（首帧={first_main}）"));
            if first_main {
                bundle.new_native_surface.set(ptr::null_mut());
                gl_swap_surface(&bundle, display, api);
                s.main_window.lock().unwrap().take();
            }
        }
    });
}

/// `void pojavSwapBuffers(void* window)`。
#[export_name = "pojavSwapBuffers"]
pub extern "C" fn pojav_swap_buffers(_window: *mut c_void) {
    guard!({
        let guard = EGL_STATE.lock().unwrap();
        let Some((api, display)) = guard.as_ref() else { return };
        let display = *display as egl::EGLDisplay;
        let handle = CURRENT_WINDOW.with(|c| c.get());
        if handle == 0 {
            return;
        }
        let Some(bundle) = state().windows.lock().unwrap().get(&handle).cloned() else { return };
        if bundle.state.get() == egl::STATE_NEW_WINDOW {
            (api.make_current.unwrap())(display, egl::EGL_NO_SURFACE, egl::EGL_NO_SURFACE, egl::EGL_NO_CONTEXT);
            gl_swap_surface(&bundle, display, api);
            (api.make_current.unwrap())(display, bundle.surface.get(), bundle.surface.get(), bundle.context.get());
            bundle.state.set(egl::STATE_ALIVE);
        }
        if !bundle.surface.get().is_null()
            && (api.swap_buffers.unwrap())(display, bundle.surface.get()) == 0
            && (api.get_error.unwrap())() == egl::EGL_BAD_SURFACE
        {
            log("EGL surface 失效（surfaceChanged），等待新窗口");
            (api.make_current.unwrap())(display, egl::EGL_NO_SURFACE, egl::EGL_NO_SURFACE, egl::EGL_NO_CONTEXT);
            bundle.new_native_surface.set(ptr::null_mut());
            gl_swap_surface(&bundle, display, api);
            (api.make_current.unwrap())(display, bundle.surface.get(), bundle.surface.get(), bundle.context.get());
        }
    });
}

/// `void pojavSwapInterval(int)`。
#[export_name = "pojavSwapInterval"]
pub extern "C" fn pojav_swap_interval(interval: c_int) {
    guard!({
        let mut interval = interval;
        if std::env::var("FORCE_VSYNC").map(|v| v == "true").unwrap_or(false) {
            interval = 1;
        }
        let guard = EGL_STATE.lock().unwrap();
        if let Some((api, display)) = guard.as_ref() {
            let display = *display as egl::EGLDisplay;
            (api.swap_interval.unwrap())(display, interval);
        }
    });
}

/// `void pojavSetWindowHint(int, int)` —— hint 全部 Java 侧消化，无需动作。
#[export_name = "pojavSetWindowHint"]
pub extern "C" fn pojav_set_window_hint(_hint: c_int, _value: c_int) {}

/// `void pojavTerminate(void)`。
#[export_name = "pojavTerminate"]
pub extern "C" fn pojav_terminate() {
    guard!({
        let s = state();
        s.input_ready.store(false, Ordering::Release);
        let guard = EGL_STATE.lock().unwrap();
        if let Some((api, display)) = guard.as_ref() {
            let display = *display as egl::EGLDisplay;
            (api.make_current.unwrap())(display, egl::EGL_NO_SURFACE, egl::EGL_NO_SURFACE, egl::EGL_NO_CONTEXT);
            let mut windows = s.windows.lock().unwrap();
            for (_, bundle) in windows.iter() {
                if !bundle.surface.get().is_null() {
                    (api.destroy_surface.unwrap())(display, bundle.surface.get());
                }
                (api.destroy_context.unwrap())(display, bundle.context.get());
                let native = bundle.native_surface.get();
                if !native.is_null() {
                    ANativeWindow_release(native);
                }
            }
            windows.clear();
            s.main_window.lock().unwrap().take();
            (api.terminate.unwrap())(display);
        }
        drop(guard);
        *EGL_STATE.lock().unwrap() = None;
        log("pojavTerminate 完成");
    });
}

/// `void pojavStartPumping(void)`。
#[export_name = "pojavStartPumping"]
pub extern "C" fn pojav_start_pumping() {
    let s = state();
    {
        let cursor = *s.cursor.lock().unwrap();
        let mut last = s.cursor_last.lock().unwrap();
        if *last != cursor {
            *last = cursor;
            s.upd_mouse.store(true, Ordering::Release);
        }
    }
    if s.upd_monitor.load(Ordering::Acquire) {
        update_monitor_size_call();
        s.monitor_consumed.store(true, Ordering::Release);
    }
    let mut queue = s.queue.lock().unwrap();
    let mut pump = s.pump_buf.lock().unwrap();
    pump.clear();
    std::mem::swap(&mut *queue, &mut *pump);
}

/// `void pojavPumpEvents(void* window)` —— 对每个窗口派发本帧事件。
#[export_name = "pojavPumpEvents"]
pub extern "C" fn pojav_pump_events(window: *mut c_void) {
    guard!({
        #[cfg(all(feature = "host-sim", not(target_os = "android")))]
        {
            use std::sync::atomic::AtomicUsize;
            static PUMP_N: AtomicUsize = AtomicUsize::new(0);
            let n = PUMP_N.fetch_add(1, Ordering::Relaxed);
            if n < 3 || n % 600 == 0 {
                log(&format!(
                    "[diag] pump#{n} cb_slot: char={} curpos={} key={} mouse={} scroll={}",
                    s2_cb("char"), s2_cb("curpos"), s2_cb("key"), s2_cb("mouse"), s2_cb("scroll")
                ));
            }
        }
        let s = state();
        if s.upd_mouse.load(Ordering::Acquire) {
            let cursor = *s.cursor.lock().unwrap();
            // v0.2.7 统一派发：直调优先；Android 不回落闭包（见 pump_dispatch 注释）
            unsafe {
                pump_dispatch(
                    "cursor-pos",
                    &s.cb_cursor_pos_obj,
                    &s.mid_cursor_pos,
                    &s.cb_cursor_pos,
                    &[jvalue { j: window as jlong }, jvalue { d: cursor.0 }, jvalue { d: cursor.1 }],
                    |cb| {
                        let f: unsafe extern "system" fn(*mut c_void, f64, f64) =
                            std::mem::transmute(cb);
                        unsafe { f(window, cursor.0, cursor.1) };
                    },
                );
            }
        }
        if s.upd_monitor.load(Ordering::Acquire) {
            update_window_size_call(window as usize);
        }
        let events: Vec<Event> = std::mem::take(&mut *s.pump_buf.lock().unwrap());
        for ev in events {
            match ev.typ {
                EVENT_TYPE_CHAR => {
                    unsafe {
                        pump_dispatch(
                            "char",
                            &s.cb_char_obj,
                            &s.mid_char,
                            &s.cb_char,
                            &[jvalue { j: window as jlong }, jvalue { i: ev.i1 }],
                            |cb| {
                                let f: unsafe extern "system" fn(*mut c_void, u32) =
                                    std::mem::transmute(cb);
                                unsafe { f(window, ev.i1 as u32) };
                            },
                        );
                    }
                }
                EVENT_TYPE_CHAR_MODS => {
                    unsafe {
                        pump_dispatch(
                            "char-mods",
                            &s.cb_char_mods_obj,
                            &s.mid_char_mods,
                            &s.cb_char_mods,
                            &[jvalue { j: window as jlong }, jvalue { i: ev.i1 }, jvalue { i: ev.i2 }],
                            |cb| {
                                let f: unsafe extern "system" fn(*mut c_void, u32, i32) =
                                    std::mem::transmute(cb);
                                unsafe { f(window, ev.i1 as u32, ev.i2) };
                            },
                        );
                    }
                }
                EVENT_TYPE_CURSOR_ENTER => {
                    unsafe {
                        pump_dispatch(
                            "cursor-enter",
                            &s.cb_cursor_enter_obj,
                            &s.mid_cursor_enter,
                            &s.cb_cursor_enter,
                            &[jvalue { j: window as jlong }, jvalue { z: ev.i1 as u8 }],
                            |cb| {
                                let f: unsafe extern "system" fn(*mut c_void, i32) =
                                    std::mem::transmute(cb);
                                unsafe { f(window, ev.i1) };
                            },
                        );
                    }
                }
                EVENT_TYPE_KEY => {
                    unsafe {
                        pump_dispatch(
                            "key",
                            &s.cb_key_obj,
                            &s.mid_key,
                            &s.cb_key,
                            &[
                                jvalue { j: window as jlong },
                                jvalue { i: ev.i1 },
                                jvalue { i: ev.i2 },
                                jvalue { i: ev.i3 },
                                jvalue { i: ev.i4 },
                            ],
                            |cb| {
                                let f: unsafe extern "system" fn(*mut c_void, i32, i32, i32, i32) =
                                    std::mem::transmute(cb);
                                unsafe { f(window, ev.i1, ev.i2, ev.i3, ev.i4) };
                            },
                        );
                    }
                }
                EVENT_TYPE_MOUSE_BUTTON => {
                    unsafe {
                        pump_dispatch(
                            "mouse-button",
                            &s.cb_mouse_obj,
                            &s.mid_mouse,
                            &s.cb_mouse,
                            &[
                                jvalue { j: window as jlong },
                                jvalue { i: ev.i1 },
                                jvalue { i: ev.i2 },
                                jvalue { i: ev.i3 },
                            ],
                            |cb| {
                                let f: unsafe extern "system" fn(*mut c_void, i32, i32, i32) =
                                    std::mem::transmute(cb);
                                unsafe { f(window, ev.i1, ev.i2, ev.i3) };
                            },
                        );
                    }
                }
                EVENT_TYPE_SCROLL => {
                    unsafe {
                        pump_dispatch(
                            "scroll",
                            &s.cb_scroll_obj,
                            &s.mid_scroll,
                            &s.cb_scroll,
                            &[jvalue { j: window as jlong }, jvalue { d: ev.dx }, jvalue { d: ev.dy }],
                            |cb| {
                                let f: unsafe extern "system" fn(*mut c_void, f64, f64) =
                                    std::mem::transmute(cb);
                                unsafe { f(window, ev.dx, ev.dy) };
                            },
                        );
                    }
                }
                _ => {}
            }
        }
    });
}

/// `void pojavStopPumping(void)`。
#[export_name = "pojavStopPumping"]
pub extern "C" fn pojav_stop_pumping() {
    let s = state();
    s.pump_buf.lock().unwrap().clear();
    s.upd_mouse.store(false, Ordering::Release);
    if s.upd_monitor.load(Ordering::Acquire) && s.monitor_consumed.load(Ordering::Acquire) {
        s.upd_monitor.store(false, Ordering::Release);
        s.monitor_consumed.store(false, Ordering::Release);
    }
}

// ---------------------------------------------------------------------------
// 输入派发（Pojav critical_send_* 语义，队列模式恒开）
// ---------------------------------------------------------------------------

fn dispatch_event(ev: Event) {
    let s = state();
    match ev.typ {
        EVENT_TYPE_CURSOR_POS => {
            // Pojav 语义：isCursorEntered 联动（无 enter 回调时 grab 下自行置位）
            if !s.cursor_entered.load(Ordering::Acquire) {
                if s.cb_cursor_enter.load(Ordering::Acquire) != 0 {
                    s.cursor_entered.store(true, Ordering::Release);
                    s.queue
                        .lock()
                        .unwrap()
                        .push(Event::ints(EVENT_TYPE_CURSOR_ENTER, 1, 0, 0, 0));
                } else if s.grabbing.load(Ordering::Acquire) {
                    s.cursor_entered.store(true, Ordering::Release);
                }
            }
            *s.cursor.lock().unwrap() = (ev.dx, ev.dy);
        }
        EVENT_TYPE_KEY => {
            let buf = s.key_buf.load(Ordering::Acquire);
            if !buf.is_null() {
                let key = ev.i1 as i64;
                if key >= 32 {
                    let idx = (key - 32) as usize;
                    if idx < KEY_BUF_SIZE {
                        unsafe { *buf.add(idx) = ev.i3 as u8 };
                    }
                }
            }
            s.queue.lock().unwrap().push(ev);
        }
        EVENT_TYPE_MOUSE_BUTTON => {
            let buf = s.mouse_buf.load(Ordering::Acquire);
            if !buf.is_null() {
                let btn = ev.i1.clamp(0, MOUSE_BUF_SIZE as i32 - 1) as usize;
                unsafe { *buf.add(btn) = ev.i2 as u8 };
            }
            s.queue.lock().unwrap().push(ev);
        }
        EVENT_TYPE_WINDOW_SIZE | EVENT_TYPE_FRAMEBUFFER_SIZE => {
            s.saved_w.store(ev.i1, Ordering::Release);
            s.saved_h.store(ev.i2, Ordering::Release);
            s.upd_monitor.store(true, Ordering::Release);
            s.monitor_consumed.store(false, Ordering::Release);
        }
        _ => {
            s.queue.lock().unwrap().push(ev);
        }
    }
}

/// 解析 nativeSendData 的字符串载荷（壳层 Kotlin 与本桥的约定协议）：
/// 空格分隔 token，整型/浮点按事件类型解析。
fn parse_send_data(typ: i32, data: &str) -> Option<Event> {
    let mut it = data.split_whitespace();
    let mut next_i = |it: &mut std::str::SplitWhitespace| it.next().and_then(|t| t.parse::<i64>().ok());
    let mut next_f = |it: &mut std::str::SplitWhitespace| it.next().and_then(|t| t.parse::<f64>().ok());
    match typ {
        EVENT_TYPE_CHAR => Some(Event::ints(EVENT_TYPE_CHAR, next_i(&mut it)? as i32, 0, 0, 0)),
        EVENT_TYPE_CHAR_MODS => Some(Event::ints(
            EVENT_TYPE_CHAR_MODS,
            next_i(&mut it)? as i32,
            next_i(&mut it)? as i32,
            0,
            0,
        )),
        EVENT_TYPE_CURSOR_ENTER => Some(Event::ints(EVENT_TYPE_CURSOR_ENTER, next_i(&mut it)? as i32, 0, 0, 0)),
        EVENT_TYPE_CURSOR_POS => {
            let dx = next_f(&mut it)?;
            let dy = next_f(&mut it)?;
            Some(Event { typ: EVENT_TYPE_CURSOR_POS, i1: 0, i2: 0, i3: 0, i4: 0, dx, dy })
        }
        EVENT_TYPE_KEY => Some(Event::ints(
            EVENT_TYPE_KEY,
            next_i(&mut it)? as i32,
            next_i(&mut it)? as i32,
            next_i(&mut it)? as i32,
            next_i(&mut it)? as i32,
        )),
        EVENT_TYPE_MOUSE_BUTTON => Some(Event::ints(
            EVENT_TYPE_MOUSE_BUTTON,
            next_i(&mut it)? as i32,
            next_i(&mut it)? as i32,
            next_i(&mut it)? as i32,
            0,
        )),
        EVENT_TYPE_SCROLL => {
            let dx = next_f(&mut it)?;
            let dy = next_f(&mut it)?;
            Some(Event { typ: EVENT_TYPE_SCROLL, i1: 0, i2: 0, i3: 0, i4: 0, dx, dy })
        }
        EVENT_TYPE_WINDOW_SIZE | EVENT_TYPE_FRAMEBUFFER_SIZE => Some(Event::ints(
            typ,
            next_i(&mut it)? as i32,
            next_i(&mut it)? as i32,
            0,
            0,
        )),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// jar 契约：CallbackBridge 四个 native（符号名严格按 Java_ 前缀）
// ---------------------------------------------------------------------------

#[no_mangle]
pub unsafe extern "system" fn Java_org_lwjgl_glfw_CallbackBridge_nativeSendData(
    env: *mut JNIEnv,
    _class: jclass,
    _focus: jboolean,
    typ: jint,
    data: jstring,
) {
    guard!({
        if let Some(text) = (unsafe { jstring_to_rust(env, data) }) {
            if let Some(ev) = parse_send_data(typ, &text) {
                dispatch_event(ev);
            }
        }
    });
}

#[no_mangle]
pub unsafe extern "system" fn Java_org_lwjgl_glfw_CallbackBridge_nativeSetInputReady(
    _env: *mut JNIEnv,
    _class: jclass,
    ready: jboolean,
) -> jboolean {
    state().input_ready.store(ready != 0, Ordering::Release);
    1 // 队列模式恒开（新 jar 只走 Start/Pump/StopPumping）
}

/// 剪贴板：经 ART 侧 QuartzBridge.accessAndroidClipboard(I,String)String 转发。
#[no_mangle]
pub unsafe extern "system" fn Java_org_lwjgl_glfw_CallbackBridge_nativeClipboard(
    env: *mut JNIEnv,
    _class: jclass,
    action: jint,
    src: jbyteArray,
) -> jstring {
    let mut result: jstring = ptr::null_mut();
    guard!({
        let s = state();
        let Some(art) = (unsafe { art_env() }) else {
            log("剪贴板桥未初始化（ART env 缺失）");
            return;
        };
        let art_cls = s.art_class.load(Ordering::Acquire) as jclass;
        let mid = s.m_clipboard.load(Ordering::Acquire) as jmethodID;
        if art_cls.is_null() || mid.is_null() {
            return;
        }
        unsafe {
            let tbl = env_tbl(env);
            let atbl = env_tbl(art);
            // 读 payload（runtime env 的 byte[]）
            let mut payload: jstring = ptr::null_mut();
            if !src.is_null() {
                let len = (tbl.GetArrayLength.unwrap())(env, src);
                let elems = (tbl.GetByteArrayElements.unwrap())(env, src, ptr::null_mut());
                if !elems.is_null() && len > 0 {
                    let bytes = std::slice::from_raw_parts(elems as *const u8, len as usize);
                    if let Ok(text) = String::from_utf8(bytes.to_vec()) {
                        if let Some(js) = rust_to_jstring(art, &text) {
                            payload = js;
                        }
                    }
                    (tbl.ReleaseByteArrayElements.unwrap())(env, src, elems, 0);
                }
            }
            let args = [jvalue { l: payload as jobject }];
            let out = (atbl.CallStaticObjectMethodA.unwrap())(art, art_cls, mid, args.as_ptr());
            if atbl.ExceptionCheck.unwrap()(art) != 0 {
                atbl.ExceptionClear.unwrap()(art);
                return;
            }
            // ART 返回 String -> runtime env jstring
            if let Some(text) = (unsafe { jstring_to_rust(art, out) }) {
                if let Some(js) = (unsafe { rust_to_jstring(env, &text) }) {
                    result = js;
                }
            }
        }
    });
    result
}

/// 鼠标 grab 状态变化（游戏线程）→ 通知 ART 侧 UI 切换输入映射。
#[no_mangle]
pub unsafe extern "system" fn Java_org_lwjgl_glfw_CallbackBridge_nativeSetGrabbing(
    _env: *mut JNIEnv,
    _class: jclass,
    grabbing: jboolean,
) {
    guard!({
        let s = state();
        s.grabbing.store(grabbing != 0, Ordering::Release);
        if grabbing == 0 {
            s.cursor_entered.store(false, Ordering::Release);
        }
        if let Some(art) = (unsafe { art_env() }) {
            let art_cls = s.art_class.load(Ordering::Acquire) as jclass;
            let mid = s.m_on_grab.load(Ordering::Acquire) as jmethodID;
            if !art_cls.is_null() && !mid.is_null() {
                let atbl = env_tbl(art);
                let arg = jvalue { z: grabbing };
                (atbl.CallStaticVoidMethodA.unwrap())(art, art_cls, mid, &arg as *const jvalue);
                if atbl.ExceptionCheck.unwrap()(art) != 0 {
                    atbl.ExceptionClear.unwrap()(art);
                }
            }
        }
    });
}

// ---------------------------------------------------------------------------
// jar 契约：GLFW native 方法
// ---------------------------------------------------------------------------

/// 【v0.2.5 直调修复】设备实机（bionic liblwjgl 的 libffi 闭包）首次触发回调即
/// SIGSEGV pc=0（两次崩溃同指纹：pc=0、lr=匿名段+0x99a8、参数 (window,0,1,0)=
/// 左键按下 = 标题屏第一下触摸）；同一 jar + 同一桥在主机沙箱复现同链路却完全
/// 稳定 —— 闭包蹦床在设备环境不可用。对策：注册回调时经 jar 自带的
/// `Callback.get(蹦床指针)` 反查 Java 回调对象并缓存全局引用 + 各接口
/// `invoke` methodID；pump 派发改走 `CallVoidMethodA`（A 变体，无 varargs）
/// 直接调 `GLFWxxCallbackI.invoke`，libffi 闭包整段绕开。解析失败自动回退
/// 旧闭包路径（不同 jar 版本的兼容安全网）。

/// Callback.get(J) 静态方法 ID + Callback 类全局引用（惰性解析一次）。
unsafe fn ensure_cb_get_mid(env: EnvPtr) -> jmethodID {
    let s = state();
    let cached = s.cb_get_mid.load(Ordering::Acquire) as jmethodID;
    if !cached.is_null() {
        return cached;
    }
    let tbl = unsafe { env_tbl(env) };
    let cls = (tbl.FindClass.unwrap())(env, c"org/lwjgl/system/Callback".as_ptr());
    if cls.is_null() {
        tbl.ExceptionClear.unwrap()(env);
        return ptr::null_mut();
    }
    let mid = (tbl.GetStaticMethodID.unwrap())(
        env,
        cls,
        c"get".as_ptr(),
        c"(J)Lorg/lwjgl/system/CallbackI;".as_ptr(),
    );
    if mid.is_null() {
        tbl.ExceptionClear.unwrap()(env);
    }
    let gcls = (tbl.NewGlobalRef.unwrap())(env, cls);
    (tbl.DeleteLocalRef.unwrap())(env, cls);
    s.cb_get_mid.store(mid as *mut c_void, Ordering::Release);
    s.cb_cbcls.store(gcls as *mut c_void, Ordering::Release);
    mid
}

/// 接口 invoke methodID（每类型解析一次）。iface 如 "org/lwjgl/glfw/GLFWMouseButtonCallbackI"。
unsafe fn ensure_invoke_mid(
    env: EnvPtr,
    iface: &str,
    sig: &str,
    slot: &AtomicPtr<c_void>,
) -> jmethodID {
    let cached = slot.load(Ordering::Acquire) as jmethodID;
    if !cached.is_null() {
        return cached;
    }
    let tbl = unsafe { env_tbl(env) };
    let cls_name = CString::new(iface).unwrap();
    let cls = (tbl.FindClass.unwrap())(env, cls_name.as_ptr());
    if cls.is_null() {
        tbl.ExceptionClear.unwrap()(env);
        return ptr::null_mut();
    }
    let sig_c = CString::new(sig).unwrap();
    let mid = (tbl.GetMethodID.unwrap())(env, cls, c"invoke".as_ptr(), sig_c.as_ptr());
    if mid.is_null() {
        tbl.ExceptionClear.unwrap()(env);
    }
    (tbl.DeleteLocalRef.unwrap())(env, cls);
    if !mid.is_null() {
        slot.store(mid as *mut c_void, Ordering::Release);
    }
    mid
}

/// 注册线程（持 env）：蹦床指针 -> Callback.get 反查 Java 对象 -> 全局引用存 obj_slot。
/// ptr=0 时清空对象槽。任何失败都静默降级（pump 见空对象槽自动回退闭包路径）。
unsafe fn register_callback_object(
    env: EnvPtr,
    obj_slot: &AtomicPtr<c_void>,
    mid_slot: &AtomicPtr<c_void>,
    ptr: jlong,
    iface: &str,
    sig: &str,
) {
    let tbl = unsafe { env_tbl(env) };
    let old = obj_slot.swap(ptr::null_mut(), Ordering::AcqRel);
    if !old.is_null() {
        (tbl.DeleteGlobalRef.unwrap())(env, old as jobject);
    }
    if ptr == 0 {
        return;
    }
    let get_mid = unsafe { ensure_cb_get_mid(env) };
    if get_mid.is_null() {
        log("Callback.get 未解析（回调走旧闭包路径）");
        return;
    }
    let s = state();
    let cls = s.cb_cbcls.load(Ordering::Acquire) as jclass;
    if cls.is_null() {
        return;
    }
    let args = [jvalue { j: ptr }];
    let obj = (tbl.CallStaticObjectMethodA.unwrap())(env, cls, get_mid, args.as_ptr());
    if tbl.ExceptionCheck.unwrap()(env) != 0 {
        tbl.ExceptionClear.unwrap()(env);
        log("Callback.get 调用异常（回调走旧闭包路径）");
        return;
    }
    if obj.is_null() {
        return;
    }
    let gref = (tbl.NewGlobalRef.unwrap())(env, obj);
    (tbl.DeleteLocalRef.unwrap())(env, obj);
    if gref.is_null() {
        return;
    }
    let mid = unsafe { ensure_invoke_mid(env, iface, sig, mid_slot) };
    if mid.is_null() {
        log(&format!("invoke methodID 未解析（{iface}，回调走旧闭包路径）"));
        (tbl.DeleteGlobalRef.unwrap())(env, gref);
        return;
    }
    obj_slot.store(gref as *mut c_void, Ordering::Release);
    log(&format!("回调直调就绪（{iface}）"));
}

/// pump 派发：直调 invoke（CallVoidMethodA，无 varargs）。返回 false = 回退闭包路径。
unsafe fn invoke_direct(
    obj_slot: &AtomicPtr<c_void>,
    mid_slot: &AtomicPtr<c_void>,
    args: &[jvalue],
) -> bool {
    let obj = obj_slot.load(Ordering::Acquire) as jobject;
    let mid = mid_slot.load(Ordering::Acquire) as jmethodID;
    if obj.is_null() || mid.is_null() {
        return false;
    }
    let Some(env) = (unsafe { runtime_env() }) else { return false };
    let tbl = unsafe { env_tbl(env) };
    let Some(call) = tbl.CallVoidMethodA else { return false };
    if tbl.ExceptionCheck.unwrap()(env) != 0 {
        tbl.ExceptionClear.unwrap()(env);
    }
    call(env, obj, mid, args.as_ptr());
    if tbl.ExceptionCheck.unwrap()(env) != 0 {
        tbl.ExceptionClear.unwrap()(env);
        log("Java 回调异常已清除（直调路径）");
    }
    true
}

/// pump 事件派发统一入口（v0.2.7 收口）：优先 v0.2.5 直调路径（CallVoidMethodA
/// 调 Java 回调对象）。直调任一环未就绪时：
/// - Android：**丢弃事件**（记一次日志）—— 设备实机 tombstone 两次同指纹
///   （pc=0，lr=匿名执行段+0x99a8 = libjnidispatch/libffi 静态蹦床区）证明
///   设备环境 libffi 闭包调用必崩；旧回退名义上是「降级」，实际是「复活已知
///   崩溃路径」。输入事件丢一帧无碍，进程崩溃则整局游戏没了。
/// - host-sim：回落闭包路径（沙箱 libffi 正常，模拟测试依赖它）。
unsafe fn pump_dispatch(
    label: &str,
    obj_slot: &AtomicPtr<c_void>,
    mid_slot: &AtomicPtr<c_void>,
    cb_slot: &AtomicUsize,
    args: &[jvalue],
    fallback: impl FnOnce(usize),
) {
    let direct = unsafe { invoke_direct(obj_slot, mid_slot, args) };
    if direct {
        return;
    }
    let cb = cb_slot.load(Ordering::Acquire);
    if cb == 0 {
        return;
    }
    #[cfg(target_os = "android")]
    {
        log(&format!(
            "{label} 直调未就绪，事件丢弃（Android 不回落 libffi 闭包：设备实机 pc=0）"
        ));
    }
    #[cfg(not(target_os = "android"))]
    {
        fallback(cb);
    }
}


/// 回调槽位写入 State.cb_*（pojav_pump_events 同源读取）。
/// v0.2.3 修复：此前宏写入模块级 static、泵读 State 字段，两处存储互不连通，
/// 游戏注册的全部 GLFW 回调（光标/按键/鼠标/滚轮）实际落空 —— 实机表现为
/// 触摸/虚拟按键全无响应（2026-11-24 实测根因）。
macro_rules! jni_callback_setter {
    ($symbol:ident, $slot:ident, $obj_slot:ident, $mid_slot:ident, $iface:literal, $sig:literal) => {
        #[no_mangle]
        pub unsafe extern "system" fn $symbol(
            env: *mut JNIEnv,
            _class: jclass,
            _window: jlong,
            callback: jlong,
        ) -> jlong {
            let old = state().$slot.swap(callback as usize, Ordering::AcqRel) as jlong;
            guard!({
                // v0.2.5：注册即反查 Java 对象 + invoke methodID（失败降级闭包路径）
                unsafe {
                    register_callback_object(
                        env,
                        &state().$obj_slot,
                        &state().$mid_slot,
                        callback,
                        $iface,
                        $sig,
                    )
                };
            });
            old
        }
    };
}

jni_callback_setter!(
    Java_org_lwjgl_glfw_GLFW_nglfwSetCharCallback,
    cb_char, cb_char_obj, mid_char,
    "org/lwjgl/glfw/GLFWCharCallbackI", "(JI)V"
);
jni_callback_setter!(
    Java_org_lwjgl_glfw_GLFW_nglfwSetCharModsCallback,
    cb_char_mods, cb_char_mods_obj, mid_char_mods,
    "org/lwjgl/glfw/GLFWCharModsCallbackI", "(JII)V"
);
jni_callback_setter!(
    Java_org_lwjgl_glfw_GLFW_nglfwSetCursorEnterCallback,
    cb_cursor_enter, cb_cursor_enter_obj, mid_cursor_enter,
    "org/lwjgl/glfw/GLFWCursorEnterCallbackI", "(JZ)V"
);
jni_callback_setter!(
    Java_org_lwjgl_glfw_GLFW_nglfwSetCursorPosCallback,
    cb_cursor_pos, cb_cursor_pos_obj, mid_cursor_pos,
    "org/lwjgl/glfw/GLFWCursorPosCallbackI", "(JDD)V"
);
jni_callback_setter!(
    Java_org_lwjgl_glfw_GLFW_nglfwSetKeyCallback,
    cb_key, cb_key_obj, mid_key,
    "org/lwjgl/glfw/GLFWKeyCallbackI", "(JIIII)V"
);
jni_callback_setter!(
    Java_org_lwjgl_glfw_GLFW_nglfwSetMouseButtonCallback,
    cb_mouse, cb_mouse_obj, mid_mouse,
    "org/lwjgl/glfw/GLFWMouseButtonCallbackI", "(JIII)V"
);
jni_callback_setter!(
    Java_org_lwjgl_glfw_GLFW_nglfwSetScrollCallback,
    cb_scroll, cb_scroll_obj, mid_scroll,
    "org/lwjgl/glfw/GLFWScrollCallbackI", "(JDD)V"
);

#[no_mangle]
pub unsafe extern "system" fn Java_org_lwjgl_glfw_GLFW_nglfwSetShowingWindow(
    _env: *mut JNIEnv,
    _class: jclass,
    window: jlong,
) {
    state().showing.store(window, Ordering::Release);
}

#[no_mangle]
pub unsafe extern "system" fn Java_org_lwjgl_glfw_GLFW_nglfwGetCursorPos(
    env: *mut JNIEnv,
    _class: jclass,
    _window: jlong,
    xpos: jobject,
    ypos: jobject,
) {
    guard!({
        let cursor = *state().cursor.lock().unwrap();
        let tbl = env_tbl(env);
        if !xpos.is_null() {
            let p = (tbl.GetDirectBufferAddress.unwrap())(env, xpos);
            if !p.is_null() {
                *(p as *mut f64) = cursor.0;
            }
        }
        if !ypos.is_null() {
            let p = (tbl.GetDirectBufferAddress.unwrap())(env, ypos);
            if !p.is_null() {
                *(p as *mut f64) = cursor.1;
            }
        }
    });
}

#[no_mangle]
pub unsafe extern "system" fn Java_org_lwjgl_glfw_GLFW_nglfwGetCursorPosA(
    _env: *mut JNIEnv,
    _class: jclass,
    _window: jlong,
    xpos: *mut jni_sys::jdouble,
    ypos: *mut jni_sys::jdouble,
) {
    guard!({
        let cursor = *state().cursor.lock().unwrap();
        if !xpos.is_null() {
            *xpos = cursor.0;
        }
        if !ypos.is_null() {
            *ypos = cursor.1;
        }
    });
}

#[no_mangle]
pub unsafe extern "system" fn Java_org_lwjgl_glfw_GLFW_glfwSetCursorPos(
    _env: *mut JNIEnv,
    _class: jclass,
    _window: jlong,
    x: jni_sys::jdouble,
    y: jni_sys::jdouble,
) {
    let s = state();
    *s.cursor.lock().unwrap() = (x, y);
    *s.cursor_last.lock().unwrap() = (x, y);
}

/// PojavRendererInit.nativeInitGl4esInternals —— MobileGlues/ANGLE 不需要，
/// 导出 no-op 保持 jar 兼容（仅 gl4es 路径真正使用）。
#[no_mangle]
pub unsafe extern "system" fn Java_org_lwjgl_opengl_PojavRendererInit_nativeInitGl4esInternals(
    _env: *mut JNIEnv,
    _class: jclass,
    _provider: jobject,
) {
}

/// VK_getVulkanDriverHandle —— Vulkan/Zink 路径待 M2，当前返回 0。
#[no_mangle]
pub unsafe extern "system" fn Java_org_lwjgl_vulkan_VK_getVulkanDriverHandle(
    _env: *mut JNIEnv,
    _class: jclass,
) -> jlong {
    0
}

// ---------------------------------------------------------------------------
// JNI_OnLoad：OpenJDK 加载本库（jar 的 System.loadLibrary("pojavexec")）时调用。
// 绝不 RegisterNatives —— 旧 C 桥在此注册 jar 不存在的 nativeSetUseInputStackQueue
// 触发 RegisterNatives NoSuchMethodError（gladiolus 发布包 jar↔so 错位，退出码 255 根因）。
// 仅缓存 GLFW 类与按键缓冲地址（本线程正处于 GLFW.<clinit>，FindClass 经 AppClassLoader 可达）。
//
// v0.1.10 修复（双副本分裂脑）：壳层与游戏 JVM 曾各加载一份本库（runtime_dir
// 与 natives_dir 两个路径 = bionic 两份独立全局状态），Surface 挂在壳层副本、
// pojavInit 在 JVM 副本里等窗口到天荒地老。统一路径后（壳层先 dlopen 同一文件），
// libart 的 System.loadLibrary 发现库已加载会**跳过 JNI_OnLoad** —— rt_vm/GLFW
// 缓存永远不会被设置。故提供 quartz_bridge_notify_rt_vm 由壳层在 CreateJavaVM
// 成功后显式调用；JNI_OnLoad（旧路径时序）与本函数共用同一实现且幂等。
// ---------------------------------------------------------------------------

static RT_VM_NOTIFIED: AtomicBool = AtomicBool::new(false);

/// 缓存 OpenJDK（runtime）侧 GLFW 类 / 回调方法 ID / 直读缓冲地址。幂等。
unsafe fn notify_runtime_vm(vm: VmPtr) {
    if RT_VM_NOTIFIED.swap(true, Ordering::SeqCst) {
        return;
    }
    let s = state();
    s.rt_vm.store(vm, Ordering::Release);
    if let Some(env) = (unsafe { runtime_env() }) {
        let glfw_name = CString::new("org/lwjgl/glfw/GLFW").unwrap();
        let m1 = CString::new("internalChangeMonitorSize").unwrap();
        let sig1 = CString::new("(II)V").unwrap();
        let m2 = CString::new("internalWindowSizeChanged").unwrap();
        let sig2 = CString::new("(J)V").unwrap();
        let f1 = CString::new("keyDownBuffer").unwrap();
        let f2 = CString::new("mouseDownBuffer").unwrap();
        let sig_buf = CString::new("Ljava/nio/ByteBuffer;").unwrap();
        unsafe {
            let tbl = env_tbl(env);
            let cls = (tbl.FindClass.unwrap())(env, glfw_name.as_ptr());
            if cls.is_null() {
                tbl.ExceptionClear.unwrap()(env);
            } else {
                s.rt_glfw_class
                    .store((tbl.NewGlobalRef.unwrap())(env, cls) as *mut c_void, Ordering::Release);
                s.m_monitor_size.store(
                    (tbl.GetStaticMethodID.unwrap())(env, cls, m1.as_ptr(), sig1.as_ptr()) as *mut c_void,
                    Ordering::Release,
                );
                s.m_window_size.store(
                    (tbl.GetStaticMethodID.unwrap())(env, cls, m2.as_ptr(), sig2.as_ptr()) as *mut c_void,
                    Ordering::Release,
                );
                let key_fid =
                    (tbl.GetStaticFieldID.unwrap())(env, cls, f1.as_ptr(), sig_buf.as_ptr());
                if !key_fid.is_null() {
                    let buf = (tbl.GetStaticObjectField.unwrap())(env, cls, key_fid);
                    if !buf.is_null() {
                        let addr = (tbl.GetDirectBufferAddress.unwrap())(env, buf);
                        s.key_buf.store(addr as *mut u8, Ordering::Release);
                    }
                }
                let mouse_fid =
                    (tbl.GetStaticFieldID.unwrap())(env, cls, f2.as_ptr(), sig_buf.as_ptr());
                if !mouse_fid.is_null() {
                    let buf = (tbl.GetStaticObjectField.unwrap())(env, cls, mouse_fid);
                    if !buf.is_null() {
                        let addr = (tbl.GetDirectBufferAddress.unwrap())(env, buf);
                        s.mouse_buf.store(addr as *mut u8, Ordering::Release);
                    }
                }
                log("JNI_OnLoad（OpenJDK）：GLFW 桥缓存就绪");
            }
        }
    }
}

#[no_mangle]
pub unsafe extern "system" fn JNI_OnLoad(vm: *mut JavaVM, _reserved: *mut c_void) -> jint {
    guard!({
        notify_runtime_vm(vm);
    });
    JNI_VERSION_1_4
}

/// 壳层在 CreateJavaVM 成功后显式注入 runtime VM（JNI_OnLoad 被 libart 抑制时
/// 的兜底；见上方说明）。返回 1 表示已通知。
#[no_mangle]
pub unsafe extern "C" fn quartz_bridge_notify_rt_vm(vm: VmPtr) -> c_int {
    let mut ok = 0;
    guard!({
        notify_runtime_vm(vm);
        ok = 1;
    });
    ok
}

// ---------------------------------------------------------------------------
// C ABI 控制面（壳层经 dlopen+dlsym 调用）
// ---------------------------------------------------------------------------

/// 壳层注入 ART JavaVM + QuartzBridge 全局引用与两个方法 ID。
#[no_mangle]
pub unsafe extern "C" fn quartz_bridge_init(
    vm: VmPtr,
    bridge_class: jobject,
    m_on_grab: jmethodID,
    m_clipboard: jmethodID,
) {
    let s = state();
    s.art_vm.store(vm, Ordering::Release);
    s.art_class.store(bridge_class as *mut c_void, Ordering::Release);
    s.m_on_grab.store(m_on_grab as *mut c_void, Ordering::Release);
    s.m_clipboard.store(m_clipboard as *mut c_void, Ordering::Release);
    log("quartz_bridge_init：ART 侧已注入");
}

/// Surface 交接：ANativeWindow_fromSurface + 主窗口 NEW_WINDOW 标记。
#[no_mangle]
pub unsafe extern "C" fn quartz_bridge_setup_surface(env: EnvPtr, surface: jobject) -> c_int {
    let mut rc: c_int = 0;
    guard!({
        let window = ANativeWindow_fromSurface(env, surface);
        if !window.is_null() {
            let s = state();
            let old = s.window.swap(window, Ordering::AcqRel);
            let w = ANativeWindow_getWidth(window);
            let h = ANativeWindow_getHeight(window);
            s.saved_w.store(w, Ordering::Release);
            s.saved_h.store(h, Ordering::Release);
            s.upd_monitor.store(true, Ordering::Release);
            s.monitor_consumed.store(false, Ordering::Release);
            // 已有主窗口：走 NEW_WINDOW 换绑流程（Pojav gl_setup_window 语义）
            let main_handle = *s.main_window.lock().unwrap();
            if let Some(handle) = main_handle {
                if let Some(bundle) = s.windows.lock().unwrap().get(&handle).cloned() {
                    bundle.new_native_surface.set(window);
                    bundle.state.set(egl::STATE_NEW_WINDOW);
                    log(&format!(
                        "setupSurface：主窗口 {handle:#x} 标记 NEW_WINDOW（{w}x{h}）"
                    ));
                }
            }
            if !old.is_null() && old != window {
                ANativeWindow_release(old);
            }
            log(&format!(
                "setupSurface：窗口 {w}x{h} 已挂载（主窗口={})",
                main_handle.map(|h| format!("{h:#x}")).unwrap_or_else(|| "无".into())
            ));
            rc = 1;
        } else {
            log("setupSurface：ANativeWindow_fromSurface 返回 NULL（Surface 无效或已销毁）");
        }
    });
    rc
}

/// Surface 销毁：释放 ANativeWindow（渲染线程下一帧回退 pbuffer）。
#[no_mangle]
pub unsafe extern "C" fn quartz_bridge_release_surface() {
    guard!({
        let s = state();
        let old = s.window.swap(ptr::null_mut(), Ordering::AcqRel);
        if !old.is_null() {
            ANativeWindow_release(old);
            log("releaseSurface：窗口已释放（主进程 surfaceDestroyed）");
        }
    });
}

#[no_mangle]
pub extern "C" fn quartz_bridge_window_ready() -> c_int {
    if state().window.load(Ordering::Acquire).is_null() { 0 } else { 1 }
}

#[no_mangle]
pub extern "C" fn quartz_bridge_send_cursor_pos(x: f32, y: f32) {
    let s = state();
    if !s.input_ready.load(Ordering::Acquire) {
        return;
    }
    dispatch_event(Event { typ: EVENT_TYPE_CURSOR_POS, i1: 0, i2: 0, i3: 0, i4: 0, dx: x as f64, dy: y as f64 });
}

#[no_mangle]
pub extern "C" fn quartz_bridge_send_mouse_button(button: c_int, action: c_int, mods: c_int) {
    let s = state();
    if !s.input_ready.load(Ordering::Acquire) {
        return;
    }
    dispatch_event(Event::ints(EVENT_TYPE_MOUSE_BUTTON, button, action, mods, 0));
}

#[no_mangle]
pub extern "C" fn quartz_bridge_send_key(key: c_int, scancode: c_int, action: c_int, mods: c_int) {
    let s = state();
    if !s.input_ready.load(Ordering::Acquire) {
        return;
    }
    dispatch_event(Event::ints(EVENT_TYPE_KEY, key, scancode, action, mods));
}

#[no_mangle]
pub extern "C" fn quartz_bridge_send_char(codepoint: c_int, mods: c_int) {
    let s = state();
    if !s.input_ready.load(Ordering::Acquire) {
        return;
    }
    if mods != 0 && s.cb_char_mods.load(Ordering::Acquire) != 0 {
        dispatch_event(Event::ints(EVENT_TYPE_CHAR_MODS, codepoint, mods, 0, 0));
    } else {
        dispatch_event(Event::ints(EVENT_TYPE_CHAR, codepoint, 0, 0, 0));
    }
}

#[no_mangle]
pub extern "C" fn quartz_bridge_send_scroll(dx: f64, dy: f64) {
    let s = state();
    if !s.input_ready.load(Ordering::Acquire) {
        return;
    }
    dispatch_event(Event { typ: EVENT_TYPE_SCROLL, i1: 0, i2: 0, i3: 0, i4: 0, dx, dy });
}

#[no_mangle]
pub extern "C" fn quartz_bridge_send_screen_size(w: c_int, h: c_int) {
    let s = state();
    s.saved_w.store(w, Ordering::Release);
    s.saved_h.store(h, Ordering::Release);
    s.upd_monitor.store(true, Ordering::Release);
    s.monitor_consumed.store(false, Ordering::Release);
}

#[no_mangle]
pub extern "C" fn quartz_bridge_is_grabbed() -> c_int {
    if state().grabbing.load(Ordering::Acquire) { 1 } else { 0 }
}

#[no_mangle]
pub extern "C" fn quartz_bridge_api_version() -> c_int {
    1
}
