//! 主机仿真 EGL 实现（EGL-on-GLX 桥接，仅 host-sim 构建参与）。
//!
//! 用途：沙箱复现设备「鼠标回调 → libffi 闭包 → CallVoidMethod」链路时，
//! 给显示桥提供真实可用的 EGL：底层 Xvfb + X11 + GLX（系统 libGL，
//! llvmpipe 软件渲染提供 GL 4.5 core）。MC 的 GL 函数由 liblwjgl_opengl
//! 经 glXGetProcAddress(libGL.so.1) 解析（与桌面 LWJGL 同源），上下文由
//! 本 shim 经 glXCreateNewContext 建立 —— 语义与设备上「渲染层自带 EGL」一致。
//!
//! 仅实现显示桥用到的 17 个入口；属性查询走内部表，不做通用 EGL。

#![cfg(all(feature = "host-sim", not(target_os = "android")))]

use std::ffi::{c_int, c_void, CStr, CString};
use std::sync::atomic::{AtomicPtr, AtomicU32, Ordering};
use std::sync::Mutex;

// ---- X11 / GLX 外部符号（构建期链 -lX11 -lGL） ----
#[repr(C)]
struct XVisualInfo {
    _opaque: [u8; 0],
}
#[repr(C)]
struct Display {
    _opaque: [u8; 0],
}

extern "C" {
    fn XOpenDisplay(name: *const u8) -> *mut Display;
    fn XCloseDisplay(d: *mut Display) -> c_int;
    fn XDefaultScreen(d: *mut Display) -> c_int;
    fn XRootWindow(d: *mut Display, screen: c_int) -> u64;
    fn XCreateSimpleWindow(
        d: *mut Display,
        parent: u64,
        x: c_int,
        y: c_int,
        w: u32,
        h: u32,
        border: u32,
        border_px: u64,
        bg: u64,
    ) -> u64;
    fn XMapWindow(d: *mut Display, w: u64) -> c_int;
    fn glXChooseFBConfig(
        d: *mut Display,
        screen: c_int,
        attrs: *const c_int,
        nitems: *mut c_int,
    ) -> *mut *mut c_void;
    fn glXCreateNewContext(
        d: *mut Display,
        config: *mut c_void,
        render_type: c_int,
        share: *mut c_void,
        direct: c_int,
    ) -> *mut c_void;
    fn glXCreateWindow(d: *mut Display, config: *mut c_void, win: u64, attrs: *const c_int) -> u64;
    fn glXCreatePbuffer(d: *mut Display, config: *mut c_void, attrs: *const c_int) -> u64;
    fn glXMakeContextCurrent(d: *mut Display, draw: u64, read: u64, ctx: *mut c_void) -> c_int;
    fn glXSwapBuffers(d: *mut Display, drawable: u64) -> ();
    fn glXDestroyPbuffer(d: *mut Display, pbuf: u64) -> ();
    fn glXDestroyWindow(d: *mut Display, win: u64) -> ();
    fn glXDestroyContext(d: *mut Display, ctx: *mut c_void) -> ();
}

// GLX 常量
const GLX_DOUBLEBUFFER: c_int = 5;
const GLX_RGBA_TYPE: c_int = 0x8014;
const GLX_PBUFFER_WIDTH: c_int = 0x8041;
const GLX_PBUFFER_HEIGHT: c_int = 0x8042;
const GLX_PBUFFER_LARGEST: c_int = 0x8043;
const GLX_NONE: c_int = 0;

// EGL 常量（与 bridge::egl 对齐）
const EGL_TRUE: u32 = 1;
const EGL_SUCCESS: i32 = 0x3000;

/// Surface 句柄 → (drawable, 宽, 高, 是否 pbuffer)
#[derive(Clone, Copy)]
struct SimSurface {
    drawable: u64,
    width: i32,
    height: i32,
    pbuffer: bool,
}

static DISPLAY: AtomicPtr<Display> = AtomicPtr::new(std::ptr::null_mut());
static XWIN: AtomicU32 = AtomicU32::new(0);
static SCREEN: AtomicU32 = AtomicU32::new(0);
static FB_CONFIGS: Mutex<Vec<usize>> = Mutex::new(Vec::new());
static SURFACES: Mutex<Vec<(usize, SimSurface)>> = Mutex::new(Vec::new());
static CONTEXTS: Mutex<Vec<usize>> = Mutex::new(Vec::new());
static CONFIG_SERIAL: AtomicU32 = AtomicU32::new(1);

fn ensure_display() -> *mut Display {
    let cur = DISPLAY.load(Ordering::Acquire);
    if !cur.is_null() {
        return cur;
    }
    unsafe {
        let d = XOpenDisplay(std::ptr::null());
        if d.is_null() {
            eprintln!("[egl-sim] XOpenDisplay 失败（DISPLAY 未设置或 Xvfb 未启动）");
            return std::ptr::null_mut();
        }
        let screen = XDefaultScreen(d);
        SCREEN.store(screen as u32, Ordering::Release);
        // 自建一个 1280x720 的 X 窗口（QUARTZ_SIM_W/H 可覆盖）
        let w = std::env::var("QUARTZ_SIM_W")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1280u32);
        let h = std::env::var("QUARTZ_SIM_H")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(720u32);
        let win = XCreateSimpleWindow(d, XRootWindow(d, screen), 0, 0, w, h, 0, 0, 0xff000000);
        XMapWindow(d, win);
        XWIN.store(win as u32, Ordering::Release);
        eprintln!("[egl-sim] X 显示就绪 screen={screen} window={win}（{w}x{h}）");
        DISPLAY.store(d, Ordering::Release);
        d
    }
}

unsafe extern "C" fn sim_get_display(_nat: *mut c_void) -> *mut c_void {
    ensure_display() as *mut c_void
}

unsafe extern "C" fn sim_initialize(d: *mut c_void, maj: *mut i32, mnr: *mut i32) -> u32 {
    if d.is_null() {
        return 0;
    }
    if !maj.is_null() {
        *maj = 1;
    }
    if !mnr.is_null() {
        *mnr = 5;
    }
    EGL_TRUE
}

unsafe extern "C" fn sim_choose_config(
    d: *mut c_void,
    _attrs: *const c_int,
    configs: *mut *mut c_void,
    _size: c_int,
    n: *mut c_int,
) -> u32 {
    if d.is_null() {
        return 0;
    }
    let screen = SCREEN.load(Ordering::Acquire) as c_int;
    let fbattrs = [GLX_DOUBLEBUFFER, 1, GLX_NONE, 0];
    let mut count: c_int = 0;
    let list = glXChooseFBConfig(d as *mut Display, screen, fbattrs.as_ptr(), &mut count);
    if list.is_null() || count <= 0 {
        if !n.is_null() {
            *n = 0;
        }
        return 0;
    }
    let mut fb = FB_CONFIGS.lock().unwrap();
    fb.clear();
    for i in 0..count as usize {
        fb.push(*list.add(i) as usize);
    }
    let serial = CONFIG_SERIAL.fetch_add(1, Ordering::AcqRel);
    if !n.is_null() {
        *n = 1;
    }
    if !configs.is_null() {
        // 返回一个合成的 config 句柄（串号），实际 FBConfig 用句柄查表
        *configs = serial as *mut c_void;
    }
    EGL_TRUE
}

fn config_to_fbconfig(cfg: *mut c_void) -> Option<*mut c_void> {
    let idx = (cfg as usize).wrapping_sub(1);
    let fb = FB_CONFIGS.lock().unwrap();
    fb.get(idx).map(|p| *p as *mut c_void)
}

unsafe extern "C" fn sim_get_config_attrib(
    _d: *mut c_void,
    _cfg: *mut c_void,
    attr: c_int,
    val: *mut i32,
) -> u32 {
    // RENDERABLE_TYPE=ES3|OpenGL(0x2)|WINDOW|PBUFFER 等，宽泛给值满足桥的判断
    if val.is_null() {
        return 0;
    }
    *val = match attr {
        0x1240 => 0x2 | 0x0040,    // RENDERABLE_TYPE: OpenGL bit | ES3 bit
        0x1233 => 0x0004 | 0x0001, // SURFACE_TYPE: WINDOW | PBUFFER
        0x302E => 0,               // NATIVE_VISUAL_ID
        0x3025 => 1,               // BUFFER_SIZE
        0x3028 => 1,               // DEPTH_SIZE
        0x302B => 0,               // ALPHA_SIZE
        _ => 1,
    };
    EGL_TRUE
}

unsafe extern "C" fn sim_bind_api(_api: u32) -> u32 {
    EGL_TRUE
}

unsafe extern "C" fn sim_create_context(
    _d: *mut c_void,
    cfg: *mut c_void,
    _share: *mut c_void,
    _attrs: *const c_int,
) -> *mut c_void {
    let d = ensure_display();
    if d.is_null() {
        return std::ptr::null_mut();
    }
    let Some(fb) = config_to_fbconfig(cfg) else {
        return std::ptr::null_mut();
    };
    let ctx = glXCreateNewContext(
        d as *mut Display,
        fb,
        GLX_RGBA_TYPE,
        std::ptr::null_mut(),
        1,
    );
    if !ctx.is_null() {
        CONTEXTS.lock().unwrap().push(ctx as usize);
    }
    ctx
}

unsafe extern "C" fn sim_create_window_surface(
    _d: *mut c_void,
    cfg: *mut c_void,
    _native: *mut c_void,
    _attrs: *const c_int,
) -> *mut c_void {
    let d = ensure_display();
    if d.is_null() {
        return std::ptr::null_mut();
    }
    let Some(fb) = config_to_fbconfig(cfg) else {
        return std::ptr::null_mut();
    };
    let win = XWIN.load(Ordering::Acquire) as u64;
    if win == 0 {
        return std::ptr::null_mut();
    }
    let glxwin = glXCreateWindow(d as *mut Display, fb, win, std::ptr::null());
    if glxwin == 0 {
        return std::ptr::null_mut();
    }
    let w = std::env::var("QUARTZ_SIM_W")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1280i32);
    let h = std::env::var("QUARTZ_SIM_H")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(720i32);
    let handle = glxwin as usize;
    SURFACES.lock().unwrap().push((
        handle,
        SimSurface {
            drawable: glxwin,
            width: w,
            height: h,
            pbuffer: false,
        },
    ));
    handle as *mut c_void
}

unsafe extern "C" fn sim_create_pbuffer_surface(
    _d: *mut c_void,
    cfg: *mut c_void,
    attrs: *const c_int,
) -> *mut c_void {
    let d = ensure_display();
    if d.is_null() {
        return std::ptr::null_mut();
    }
    let Some(fb) = config_to_fbconfig(cfg) else {
        return std::ptr::null_mut();
    };
    let mut width = 1i32;
    let mut height = 1i32;
    if !attrs.is_null() {
        let mut i = 0;
        loop {
            let a = *attrs.add(i);
            if a == GLX_NONE {
                break;
            }
            let v = *attrs.add(i + 1);
            if a == GLX_PBUFFER_WIDTH {
                width = v;
            }
            if a == GLX_PBUFFER_HEIGHT {
                height = v;
            }
            i += 2;
        }
    }
    let pbattrs = [
        GLX_PBUFFER_WIDTH,
        width,
        GLX_PBUFFER_HEIGHT,
        height,
        GLX_PBUFFER_LARGEST,
        0,
        GLX_NONE,
        0,
    ];
    let pbuf = glXCreatePbuffer(d as *mut Display, fb, pbattrs.as_ptr());
    if pbuf == 0 {
        return std::ptr::null_mut();
    }
    let handle = pbuf as usize;
    SURFACES.lock().unwrap().push((
        handle,
        SimSurface {
            drawable: pbuf,
            width,
            height,
            pbuffer: true,
        },
    ));
    eprintln!("[egl-sim] pbuffer surface {width}x{height} 就绪");
    handle as *mut c_void
}

fn surface_info(surface: *mut c_void) -> Option<SimSurface> {
    let handle = surface as usize;
    SURFACES
        .lock()
        .unwrap()
        .iter()
        .find(|(h, _)| *h == handle)
        .map(|(_, s)| SimSurface { ..s.clone() })
}

unsafe extern "C" fn sim_make_current(
    _d: *mut c_void,
    draw: *mut c_void,
    read: *mut c_void,
    ctx: *mut c_void,
) -> u32 {
    let d = ensure_display();
    if d.is_null() {
        return 0;
    }
    let draw_id = surface_info(draw).map(|s| s.drawable).unwrap_or(0);
    let read_id = surface_info(read).map(|s| s.drawable).unwrap_or(0);
    let rc = glXMakeContextCurrent(d as *mut Display, draw_id, read_id, ctx);
    if rc != 0 {
        eprintln!(
            "[egl-sim] make_current drawable={draw_id:#x} ctx={:#x}",
            ctx as usize
        );
    } else {
        eprintln!("[egl-sim] make_current 失败 drawable={draw_id:#x}");
    }
    if rc != 0 {
        EGL_TRUE
    } else {
        0
    }
}

unsafe extern "C" fn sim_swap_buffers(_d: *mut c_void, surface: *mut c_void) -> u32 {
    let d = ensure_display();
    let Some(s) = surface_info(surface) else {
        return EGL_TRUE;
    };
    if !s.pbuffer {
        glXSwapBuffers(d as *mut Display, s.drawable);
    }
    EGL_TRUE
}

unsafe extern "C" fn sim_swap_interval(_d: *mut c_void, _i: i32) -> u32 {
    EGL_TRUE
}

unsafe extern "C" fn sim_query_surface(
    _d: *mut c_void,
    surface: *mut c_void,
    attr: i32,
    val: *mut i32,
) -> u32 {
    let Some(s) = surface_info(surface) else {
        return 0;
    };
    if val.is_null() {
        return 0;
    }
    *val = match attr {
        0x3057 => s.width,  // EGL_WIDTH
        0x3056 => s.height, // EGL_HEIGHT
        0x3099 => 1,        // EGL_LATCHED? -> generic ok
        _ => 0,
    };
    EGL_TRUE
}

unsafe extern "C" fn sim_destroy_surface(_d: *mut c_void, surface: *mut c_void) -> u32 {
    let d = ensure_display();
    if d.is_null() {
        return EGL_TRUE;
    }
    let handle = surface as usize;
    let mut tabs = SURFACES.lock().unwrap();
    if let Some(pos) = tabs.iter().position(|(h, _)| *h == handle) {
        let s = tabs.remove(pos).1;
        if s.pbuffer {
            glXDestroyPbuffer(d as *mut Display, s.drawable);
        } else {
            glXDestroyWindow(d as *mut Display, s.drawable);
        }
    }
    EGL_TRUE
}

unsafe extern "C" fn sim_destroy_context(_d: *mut c_void, ctx: *mut c_void) -> u32 {
    let d = ensure_display();
    if !d.is_null() && !ctx.is_null() {
        glXDestroyContext(d as *mut Display, ctx);
    }
    EGL_TRUE
}

unsafe extern "C" fn sim_terminate(d: *mut c_void) -> u32 {
    if !d.is_null() {
        XCloseDisplay(d as *mut Display);
        DISPLAY.store(std::ptr::null_mut(), Ordering::Release);
    }
    EGL_TRUE
}

unsafe extern "C" fn sim_get_error() -> i32 {
    EGL_SUCCESS
}

/// 供 bridge::egl::load_api 在 host-sim 下直接取函数表（不 dlopen）。
/// 这些入口的 ABI 与 bridge::egl 类型定义一一对应。
pub fn api() -> super::egl::EglApi {
    unsafe {
        super::egl::EglApi {
            make_current: Some(std::mem::transmute(sim_make_current as *const c_void)),
            destroy_surface: Some(std::mem::transmute(sim_destroy_surface as *const c_void)),
            destroy_context: Some(std::mem::transmute(sim_destroy_context as *const c_void)),
            terminate: Some(std::mem::transmute(sim_terminate as *const c_void)),
            get_display: Some(std::mem::transmute(sim_get_display as *const c_void)),
            initialize: Some(std::mem::transmute(sim_initialize as *const c_void)),
            choose_config: Some(std::mem::transmute(sim_choose_config as *const c_void)),
            get_configs: None,
            get_config_attrib: Some(std::mem::transmute(sim_get_config_attrib as *const c_void)),
            bind_api: Some(std::mem::transmute(sim_bind_api as *const c_void)),
            create_context: Some(std::mem::transmute(sim_create_context as *const c_void)),
            create_window_surface: Some(std::mem::transmute(
                sim_create_window_surface as *const c_void,
            )),
            create_pbuffer_surface: Some(std::mem::transmute(
                sim_create_pbuffer_surface as *const c_void,
            )),
            swap_buffers: Some(std::mem::transmute(sim_swap_buffers as *const c_void)),
            swap_interval: Some(std::mem::transmute(sim_swap_interval as *const c_void)),
            query_surface: Some(std::mem::transmute(sim_query_surface as *const c_void)),
            get_error: Some(std::mem::transmute(sim_get_error as *const c_void)),
        }
    }
}
