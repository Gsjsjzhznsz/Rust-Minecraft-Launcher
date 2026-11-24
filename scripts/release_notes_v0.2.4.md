# Quartz v0.2.4 — 崩溃根因修复与实机诊断增强

> 前置说明：v0.2.3 的触摸/按键修复（GLFW 回调槽位断裂 + 虚拟按键布局重排）已由 CI 构建，
> 实机截图（21:31）显示的「控件错位」仍是旧版 APK —— 请安装本版 APK 后再验证。
> 与 Pojav 官方 gladiolus jar 的契约已逐字节核对（Functions 11/11 符号吻合、
> nglfwSet*Callback 回调为裸 C 函数指针语义，与官方 input_bridge_v3.c 一致）。

## 修复

### 1. zink 渲染层「EGL 初始化失败 0x3001 → 退出码 255」（启动即崩）
- **根因**：设备上的 zink 包是旧版提取的残缺包 —— 白名单后补的 `libc++_shared.so`
  不在包内，预载链断裂（`libcutils → libglapi → libgallium_dri → libEGL_mesa` 全线
  无法 dlopen）。已核对 IronizedZink v1.1.0 APK：`libc++_shared.so`（9.2MB）确实在包内。
- **修复**：`renderers::is_installed` 完整性校验扩展到全部预载清单 —— 残缺包在启动前
  被判定未安装，`ensure_renderer` 自动在线重装补齐。

### 2. JVM 系统属性丢失（LWJGL/JNA 路径全落空）
- **证据**：实机日志 `[quartz][diag] JVM 属性 java.class.path = <null>（未生效！）`，
  且崩溃报告 JVM Flags 列表无 -D 项。LWJGL 反复报
  「Incompatible Java and native library versions detected / Failed to load a library」。
- **根因**：设备 OpenJDK 17 构建（"17-internal"）在 ART 并存场景下 CreateJavaVM
  丢弃 -D 系统属性；AppClassLoader 在创建期已缓存 classpath（主类能加载），
  受害的是「运行期才读属性」的库：LWJGL natives 解析、JNA 引导库、Netty workdir。
- **修复**：调 main 前经 JNI 调 `System.setProperty` 把全部 -D 属性无条件硬回写
  （幂等，已生效时无害重写）——LWJGL/JNA 从此拿到正确路径。

### 3. MobileGlues 路径 EGL config 选档失败（阶梯全 BAD_ATTRIBUTE）
- **证据**：实机日志 `eglChooseConfig 阶梯 #0..#6 未命中（0x3004）`，连
  「SURFACE_TYPE + RENDERABLE_TYPE」最简集都被拒，跌入 eglGetConfigs 兜底拿到
  renderable=0x0000 的无语义项 config。
- **修复**：阶梯新增 #7 空属性档（attrib_list 仅 EGL_NONE —— spec 合规形态，
  任何实现必须接受），优先于盲目兜底。

### 4. 崩溃证据链补全（定位 ~19s Render 线程崩溃）
- 致命信号拦截升级 SA_SIGINFO：崩溃瞬间输出 `sig/pc/lr/sp/fault/x0/x1` +
  `dladdr(lr)` 模块名与偏移（区分 JIT 匿名段 vs 具名 so）—— 下次崩溃日志可直接
  读出崩在哪个模块。
- 主进程崩溃回收新增 HotSpot `hs_err_*` 抓取（15 分钟内新文件，前 80 行入启动日志）
  —— SIGABRT 家族崩溃的完整现场不再丢失。

## 已核对的外部契约（本轮验证）
- Pojav gladiolus `lwjgl-glfw-classes.jar`：Functions 表需要 11 个 pojav* 符号，
  quartz-bridge 全部导出，无缺口（pc=0 崩溃不是缺符号所致）。
- 官方 `input_bridge_v3.c`：`nglfwSet*Callback` 存裸函数指针并在泵里直接调用
  —— quartz-bridge v0.2.3 的回调实现与官方语义一致。

## 升级说明
- 安装本版 APK 后，首次启动 zink 渲染层会自动重装修复残缺包（日志可见
  「首次使用渲染层 zink：在线获取…」）。
- 若 MobileGlues 路径仍出现崩溃，启动日志将包含 `[quartz][fatal]` 寄存器现场与
  `[hs_err]` 完整报告 —— 请整份日志回传。
