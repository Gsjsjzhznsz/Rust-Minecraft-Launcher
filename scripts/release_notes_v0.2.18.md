# Quartz v0.2.18 —— ZL2 代码直移植：linker namespace 旁路（骁龙 zink 决战版）

## 一句话
按你的要求，把 ZL2 跑通 zink 的核心代码（`nsbypass.c` + `loadTurnipVulkan`）**原样移植**进 Quartz（Rust 实现）：自建 driver_namespace 绕开链接器命名空间白名单 —— ZL2/FCL 的 zink 在骁龙上为什么能跑，Quartz 现在就是同一套机制。

## 为什么 v0.2.17 还是不行（launch-1795764440 复盘）
Turnip 驱动包其实已在位（`renderers/turnip/`），探针把三条加载通道全部试了一遍，全被**链接器命名空间边界**拒之门外：

1. 普通 dlopen（应用 clns）：turnip 的 DT_NEEDED `libhardware.so` 不在应用可见的公共库白名单里 → `library "libhardware.so" not found`；
2. sphal 通道（系统 loader 加载厂商 ICD 的同款通道）：sphal 的 permitted 路径不含应用数据目录 → `not accessible for the namespace sphal`；
3. 系统 loader 通道：隔离子进程内依旧 SIGSEGV（既有 libvulkan 实例在此进程形态下 ICD 发现落空），fork 隔离按设计把崩溃关进笼子。

三条都是命名空间边界，不是驱动问题 —— Turnip 包（Mesa 26.3.1 KGSL）本身从没得到过装载机会。

## v0.2.18 移植内容（对照 ZalithLauncher2 源码逐行复刻）
1. **`nsbypass.c` → `zl2_linker.rs`**：裸 `__loader_dlopen`（dlsym 优先，ZL2 PLT 走位兑底）→ 载 `ld-android.so` → `__loader_android_create_namespace` 建 **driver_namespace**（搜索路径 `/system/lib64` + vendor hw + 驱动目录，permitted `/system:/data:/vendor:/apex:/system_ext` —— ZL2 原值）。
2. **`loadTurnipVulkan` → 同链**：turnip `libvulkan_freedreno.so` **先进命名空间**（它的 `libhardware.so` 等 DT_NEEDED 全部从 `/system/lib64` 按名解析 —— 裸命名空间不经 libnativeloader 公共库过滤，clns「not found」死因根治）；再把 **DT_SONAME 改写为唯一值的全新 `libvulkan.so` 实例** 载入同一命名空间（绕开 bionic 按 SONAME 去重；新实例自持 ICD 发现 —— 系统 loader SIGSEGV 死因根治）。
3. **VULKAN_PTR 指向该全新 loader 实例**：zink（IronizedZink 消费端）从中取 `vkGetInstanceProcAddr` → `vkCreateInstance` 在命名空间内命中先行落位的 Turnip（Adreno KGSL）设备。
4. **探针保持 fork 隔离**：通道失败只带走隔离子进程；胜出后父进程重放同一链自持实例（命名空间/句柄不跨 fork 存活）；仍失败则全败 → 干净回退 MobileGlues，与 v0.2.16 一致。
5. **兜底加厚**：厂商 ICD 自动发现变体（同一命名空间让厂商 blob 也再试一次），v0.2.16/17 原有通道全部保留在后。

## 测试判据（启动日志）
- `QUARTZ_VK_PROBE:OK:zl2-ns devices=N:gpu0=Turnip Adreno (TM) ...` = zink 已拿到真驱动；
- 随后应看到 `渲染层 EGL 预检通过：zink`，游戏以桌面 GL（zink→Turnip）进世界。

## 若仍失败
探针链每一级都有 `stage=` 标签（zl2-ld-android / zl2-ns-api / zl2-create-ns / zl2-icd-load / zl2-soname / zl2-loader-load / probe），回传 launch 日志一行即可定位到步。

## 交付验证（四 bug 收口）
- 控件错乱 / 虚拟鼠标 / 分辨率 DPI：已收口（v0.2.14/15）。
- zink：v0.2.15 安全网 → v0.2.16 回退自愈 → v0.2.17 驱动供给 → **v0.2.18 ZL2 同款启动机制（本版）**。
