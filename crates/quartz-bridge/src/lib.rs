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

#[cfg(target_os = "android")]
mod bridge;

#[cfg(target_os = "android")]
pub use bridge::*;
