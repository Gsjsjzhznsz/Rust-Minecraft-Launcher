# Quartz v0.2.6 — 「直接崩溃」闪退修复（SIGSEGV 专诊处理器整体移除）

> 现象：安装 v0.2.5 后游戏**启动即崩**（MC 连 main 都没进，标题屏都看不到）。
> 两次独立启动（MobileGlues 渲染器 + Mesa 渲染器）完全复现。

## 根因（v0.2.5 自身引入的回归）

**v0.2.5 新增的「SIGSEGV pc=0 专诊处理器」误杀 JVM。**

证据链（两份 launch-1795537959/1795537938 日志 + 官方 tombstone 交叉比对）：

1. **崩溃点被日志夹死**：崩溃宣告紧贴「JVM 属性硬回写完成」之后，而
   `调用 net.minecraft.client.main.Main main` 行（stderr 无缓冲、必然先落盘）
   在两份日志中**均缺失** —— 崩溃发生在 FindClass(主类) 的类加载阶段。
2. **HotSpot/aarch64 依赖 SIGSEGV**：隐式空检查、栈守卫、安全点轮询都是
   信号驱动的**正常** JVM 行为。fresh 崩溃的 fault=0x80001204 恰为
   堆基址 0x800000000 + 0x1204 —— 典型隐式空检查指纹（本应被 JVM 处理器
   转成 NullPointerException，程序继续运行）。
3. **专诊处理器读到了坏 ucontext**：两次独立启动的「寄存器现场」逐位一致
   （x17=0xc13165c、x23/x24=0x71c6e430d0、sp=0x7a0ab1b40、fault=0x80001204）；
   真实崩溃现场跨进程不可能逐位相同（ASLR 必然不同）→ 处理器把类加载期
   第一次隐式空检查误判为「pc=0 调空指针」→ dump 后 `_exit` → 闪退。
4. **pc=0 分支与 fault≠0 自相矛盾**：跳转地址为 0 时指令取指 fault 必为 0；
   fault≠0 证明 pc 读值不可信，pc==0 分支判定整体失效。

## 修复（v0.2.6）

**SIGSEGV 一律不插桩，完整交还原生信号链（sigchain → HotSpot
javaSignalHandler）。**

- 移除 `install_segv_diag()` 两处安装点（JVM 创建前 + 创建后）与
  `quartz_on_segv_diag` 处理器本体。
- SIGABRT/SIGBUS/SIGFPE/SIGILL 拦截保留（HotSpot 写完 hs_err 后 abort 的
  干净退出路径不受影响）。
- 真实 pc=0 崩溃（如 v0.2.5 已修的触摸 libffi 闭包路径若复发）的取证改走
  debuggerd tombstone —— v0.2.3/v0.2.4 实测可靠（0x99a8 闭包指纹即由
  tombstone 定位），且随 SIGSEGV 交还原生链自动恢复。

## 保留的 v0.2.5 修复（不受影响）

- 触摸回调直调 JNI（`Callback.get(蹦床指针)` 反查 Java 回调对象 +
  `CallVoidMethodA` 直调 `GLFWxxCallbackI.invoke`，绕开设备上不可用的
  libffi 闭包蹦床）—— 已核实 LWJGL 3.3.x 存在
  `public static <T extends CallbackI> T get(long functionPointer)`
  （擦除签名 `(J)Lorg/lwjgl/system/CallbackI;`，与桥内 GetStaticMethodID
  描述符一致）。
- v0.2.4 的 JVM -D 属性硬回写、EGL 空属性阶梯、zink 残缺包自动重装。
