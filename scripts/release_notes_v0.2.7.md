# Quartz v0.2.7 — 「天天闪退」修复（信号拦截彻底移除 + 渲染层 EGL 预检自动回退）

> 现象：v0.2.6 实机两连崩 ——
> ① MobileGlues 启动到标题屏（约 30 秒，配方/进度加载完、窗口尺寸同步 ×2 后）原生崩溃，日志记录 `sig=0x4 pc=0xb8 sp=0 fault=0x80001204`；
> ② 切 Zink 渲染层后**启动即崩**（11 秒）：`EGL 初始化失败 0x3001` → `IllegalStateException: no OpenGL context` → 退出码 255。

## 根因一：v0.2.6 残留的 SIGILL/SIGABRT/SIGBUS/SIGFPE 拦截器（信号拦截整套移除）

v0.2.6 只移除了 SIGSEGV 专诊处理器，其余致命信号拦截保留——实机日志证明这正是标题屏崩溃源：

1. **采集结构体错位**：`SigMContext` 首字段 `fault_address` 在 kernel arm64
   sigcontext（regs[31], sp, pc, pstate, __reserved）里**不存在**（那是 x86 布局），
   整体移位 8 字节：记录的 `fault` 实为 x0、`lr` 实为 sp、`pc` 实为 pstate……
   `pc=0xb8` 其实是合法 PSTATE 值，`sp=0` 是被错位读到的寄存器——全部是垃圾。
2. **拦截吞 tombstone**：SIGILL 走本拦截即 `_exit`，debuggerd 不产报告；
   而 SIGSEGV（不拦）的 tombstone 反而完整可用（pc=0、lr=匿名段+0x99a8 指纹）。
   aarch64 HotSpot 依赖 SIGSEGV/SIGILL 原生处理链（隐式空检查/栈守卫），
   任何插桩即抢道误杀。
3. **HyperOS 实测**：:game 崩溃走 debuggerd 不影响启动器 UI（binder death 恢复，
   2026-11-24 19:10 tombstone 验证）——拦截器唯一「价值」只剩吞证据。

**修复**：致命信号全部交还原生链 → debuggerd tombstone（logcat crash 缓冲由
gameProcessGone 兜底采集进 launch 日志），下轮崩溃可精确符号化。

## 根因二：Zink（Mesa）在该设备 eglInitialize 必败，此前带病进 JVM 撞 255

实机日志（17:35）：预载 5 件套全部 pass1 成功 → `EGL 初始化失败 0x3001`
（EGL_NOT_INITIALIZED，Mesa/Zink 平台初始化失败，设备 Vulkan/DRM 环境不可达）
→ MC 拿不到 GL 上下文 → LWJGL `IllegalStateException` → 退出码 255。
用户视角就是「选 Zink → 直接崩溃」的无限闪退循环。

**修复（渲染层双保险）**：

1. **EGL 预检（probe_renderer_egl）**：:game 进程在 CreateJavaVM **之前**，
   按桥接层同款语义 dlopen 渲染层 EGL 实现 + eglGetDisplay + eglInitialize
   试跑一次。失败即干净退出（错误行带 `QUARTZ_RENDERER_EGL_FAIL` 标记），
   不再带病 CreateJavaVM。
2. **自动回退（maybe_fallback_renderer_after_exit）**：主进程 finish_game_exit
   扫描游戏退出日志中的失败标记（兼容桥接层迟败路径的「EGL 初始化失败」行），
   自动把选中渲染层回退到 MobileGlues（实机验证可启动到标题屏的完整 GLES3
   转译层）并在 UI 日志写明原因。幂等：已是 MobileGlues 则不动作。

## 保留的既有修复（本版包含）

- v0.2.5 回调直调 JNI（libffi 闭包路径绕开；v0.2.7 收口：Android 不回落闭包，
  直调未就绪时丢事件不崩溃）
- v0.2.4 JVM -D 属性硬回写、EGL 空属性阶梯、zink 残缺包自动重装
- v0.2.3 输入回调槽位修复 + 虚拟按键布局重排（触摸/控件错位修复随本版生效）
- v0.2.6 SIGSEGV 不插桩（HotSpot 隐式空检查依赖原生处理链）

## 升级指引

从 v0.2.6 Release 直接覆盖安装本版 APK。若当前渲染层选了 Zink，首次启动会
自动回退到 MobileGlues（UI 日志可见回退说明）。
