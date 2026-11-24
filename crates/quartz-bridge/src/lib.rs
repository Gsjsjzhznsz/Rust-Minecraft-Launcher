//! Quartz 游戏显示桥（Rust 重构 PojavLauncher 的 C 桥层 libpojavexec.so）。
//!
//! 接口契约以 gladiolus 官方 `lwjgl-glfw-classes.jar` 的字节码为准（反编译核对）：
//! - `org.lwjgl.glfw.GLFW$Functions`：函数指针表，经 LWJGL
//!   `apiGetFunctionAddress(lib, name)` 解析以下 C 符号（本文件逐一导出）：
//!   pojavInit / pojavCreateContext / pojavGetCurrentContext / pojavMakeCurrent /
//!   pojavTerminate / pojavSetWindowHint / pojavSwapBuffers / pojavSwapInterval /
//!   pojavPumpEvents / pojavStartPumping / pojavStopPumping
//! - `org.lwjgl.glfw.CallbackBridge`：4 个 native 方法
//!   nativeSendData(ZILjava/lang/String;)V / nativeSetInputReady(Z)Z /
//!   nativeClipboard(I[B)Ljava/lang/String; / nativeSetGrabbing(Z)V
//! - `GLFW` 的 7 个回调注册 native（nglfwSet*Callback）+ nglfwSetShowingWindow +
//!   nglfwGetCursorPos(A) + glfwSetCursorPos
//!
//! 与 Pojav C 版的关键差异（v0.1.5 修复 gladiolus 发布包 jar↔so 版本错位崩溃）：
//! - 不做任何 RegisterNatives（旧 so 的 JNI_OnLoad 注册 jar 不存在的
//!   nativeSetUseInputStackQueue 导致 RegisterNatives NoSuchMethodError -> 退出码 255）
//! - 事件常驻队列模式（新 jar 的 glfwPollEvents 恒走 Start/Pump/StopPumping）
//! - 两 JVM（ART 壳层 / OpenJDK 游戏）通过显式 C ABI init 传递，不依赖 environ 指针
//!   环境变量传递技巧；本库只被游戏侧 OpenJDK dlopen，ART 侧经壳层 dlsym 调 C ABI
//!
//! 仅 Android 目标有效；其他平台编译为空库（保持 workspace 可测试性）。

#[cfg(not(target_os = "android"))]
pub fn placeholder() {}

#[cfg(any(target_os = "android", feature = "host-sim"))]
mod bridge;

#[cfg(any(target_os = "android", feature = "host-sim"))]
pub use bridge::*;

// 主机仿真 EGL（EGL-on-GLX）：见 egl_sim.rs 头注（bridge::egl::load_api 内部经 super::egl_sim 使用）
#[cfg(all(feature = "host-sim", not(target_os = "android")))]
mod egl_sim;

// ---------------------------------------------------------------------------
// 主机仿真桩（仅 host-sim）：ANativeWindow 经环境变量给出 X11 Window XID，
// EGL 走宿主 Mesa（POJAVEXEC_EGL=/usr/lib/x86_64-linux-gnu/libEGL.so）。
// 用途：scripts/sim 沙箱复现「鼠标回调 → libffi 闭包 → CallVoidMethod」链路。
// ---------------------------------------------------------------------------

#[cfg(all(feature = "host-sim", not(target_os = "android")))]
pub mod host_stubs {
    use std::ffi::{c_int, c_void};

    fn env_u32(key: &str, def: u32) -> u32 {
        std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(def)
    }

    /// 返回 QUARTZ_SIM_XWINDOW 指定的 X11 Window（无则 0 = 无窗口路径）。
    #[no_mangle]
    pub extern "C" fn ANativeWindow_fromSurface(_env: *mut c_void, _surface: *mut c_void) -> *mut c_void {
        let xid = env_u32("QUARTZ_SIM_XWINDOW", 0);
        if xid != 0 { eprintln!("[host-stub] ANativeWindow_fromSurface -> XID {xid}"); }
        xid as *mut c_void
    }

    #[no_mangle]
    pub extern "C" fn ANativeWindow_acquire(_window: *mut c_void) {}

    #[no_mangle]
    pub extern "C" fn ANativeWindow_release(_window: *mut c_void) {}

    #[no_mangle]
    pub extern "C" fn ANativeWindow_getWidth(_window: *mut c_void) -> c_int {
        env_u32("QUARTZ_SIM_W", 1280) as c_int
    }

    #[no_mangle]
    pub extern "C" fn ANativeWindow_getHeight(_window: *mut c_void) -> c_int {
        env_u32("QUARTZ_SIM_H", 720) as c_int
    }

    #[no_mangle]
    pub extern "C" fn ANativeWindow_setBuffersGeometry(
        _window: *mut c_void,
        _w: c_int,
        _h: c_int,
        _format: c_int,
    ) -> c_int {
        0
    }
}
