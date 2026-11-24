# Quartz v0.2.5 — 标题屏首次触摸闪退修复（libffi 闭包路径整体绕开）

> 现象：进到 Minecraft 标题屏后第一次触摸即闪退（崩溃 19s，两次完全复现）。
> 证据指纹（logcat 两次一致）：SIGSEGV pc=0（调用空函数指针）、调用方=匿名执行段
> +0x99a8、寄存器 x1/x2/x3=(0,1,0)=「鼠标左键按下」回调参数。
> 时间线与指纹吻合：pump 派发第一个输入回调（用户第一次触摸）。

## 根因定位过程（本次新增主机复现环境）

1. 主机复现环境（scripts/sim）：Mojang 1.20.4 全流水线 + 设备同款 gladiolus 合并
   jar + 我们的显示桥（host 构建）+ EGL-on-GLX 仿真（Xvfb/llvmpipe）+ 注入器模拟
   「用户触摸标题屏」。复现链路与设备完全同构：
   桥 dispatch_event → 事件队列 → pojavPumpEvents → LWJGL 回调 → MC。
2. 主机复现结论：同一 jar + 同一桥，鼠标/光标/滚轮回调多轮派发 **零崩溃** ——
   派发逻辑本身无错，崩溃为设备 bionic 环境特有。
3. 设备侧差异锁定：liblwjgl.so 的 libffi 闭包（RWX 匿名段蹦床 + CallVoidMethod
   上调）在 bionic/Android15 实机上首次触发即跳空指针。对照设备崩溃指纹
   （lr=匿名段内、blr 到 0），闭包蹦床路径成立。

## 修复（v0.2.5 核心）

**回调直调：绕开 libffi 闭包，pump 直接经 JNI 调用 Java 回调对象。**

- 注册回调（nglfwSet*Callback）时，用 jar 自带的
  `org.lwjgl.system.Callback.get(蹦床指针)` 反查 Java 回调对象（global ref），
  并解析各接口的 `invoke` methodID（7 类回调签名逐一按 jar 字节码核对）。
- pojavPumpEvents 派发改走 `CallVoidMethodA`（A 变体，无 varargs，aarch64 安全）
  直接调 `GLFWxxCallbackI.invoke` —— **libffi 闭包 / 匿名蹦床整段不再执行**。
- 任一解析失败（不同 jar 版本）自动回退旧闭包路径，兼容性不降级。

## 诊断增强（v0.2.5）

- SIGSEGV pc=0 专诊处理器：若仍出现「调用空函数指针」崩溃，:game 进程在退出前
  输出完整 31 寄存器现场 + 全量 /proc/self/maps（匿名执行段身份可一锤定音）；
  pc≠0 的 SIGSEGV（HotSpot 隐式空检查/守卫页）原样转发 HotSpot 处理器，语义不变。

## 主机复现工具（本次沉淀）

- `scripts/sim/run_device_jar_sim.sh`：设备 jar + 桥（host 构建）+ Mojang natives
  一键复现；`scripts/sim/injector/`：触摸注入器（走 jar 自己的 nativeSendData）。
- 桥新增 `host-sim` feature（ANativeWindow 桩 + EGL-on-GLX 仿真），仅沙箱使用，
  Android 构建不受影响。
