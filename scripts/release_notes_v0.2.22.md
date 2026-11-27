# v0.2.22 —— zink 装载机制与 ZL2 2.8.2 完全 1:1（用户指令「跟 ZL2 完全一样」）

## 蓝本实锤（本版全部结论来自 ZL2 2.8.2 实物源码 + libEGL_mesa.so 反汇编）

1. **ZL2 的 turnip/namespace 旁路不存在于编译产物**：`driver_helper.c`
   （loadTurnipVulkan/checkAdrenoGraphics）与 `nsbypass.c`（driver_namespace
   旁路）整段包在 `#ifdef ADRENO_POSSIBLE` 内，而 2.8.2 源码里该宏被注释
   （`//#define ADRENO_POSSIBLE`）。ZL2 zink 的 Vulkan 装载 = **无条件
   `dlopen("libvulkan.so", RTLD_LAZY|RTLD_LOCAL)` 默认命名空间**。
2. **磁盘文件名就是 `libzink_dri.so`**（ZL2 APK lib/arm64-v8a 原名；其
   DT_SONAME=libgallium_dri.so 只是链接器内部标识，与磁盘名无关）。
3. **驱动发现链**（反汇编 libEGL_mesa.so / Mesa 23.0.4 补丁版
   `loader_open_driver_lib`）：search_path_vars =
   `{LIBGL_DRIVERS_PATH, LD_LIBRARY_PATH}`（rela 实锤），逐目录尝试
   `dir/tls/<name>_dri.so` → `dir/<name>_dri.so` → **`dir/lib<name>_dri.so`
   （该构建新增格式）**，driver_name=zink（MESA_LOADER_DRIVER_OVERRIDE）→
   命中 `libzink_dri.so`。ZL2 运行时 = `LD_LIBRARY_PATH`（Launcher.kt
   getLibraryPath：lwjgl-natives:/system:/vendor:/vendor/hw:/system_ext:
   renderer-plugin:native-lib）供搜索。
4. **ZL2 无 libcutils shim**：自带 libcutils.so 同样无 DT_SONAME，靠
   LD_LIBRARY_PATH 按文件名解析正常工作。

## v0.2.21 → v0.2.22 改动（四处非 1:1 全部纠正）

1. **zl2_linker 模块整体删除**（v0.2.18~21 移植的 nsbypass/loadTurnipVulkan/
   RimDroid 三级救场链，642 行）—— ZL2 编译产物里根本没有这条链路；
   turnip namespace 通道、VK_ICD_FILENAMES 设置、ICD JSON 改写一并删除。
2. **load_vulkan 1:1 重写**：唯一路径 = 默认命名空间 dlopen("libvulkan.so")
   + VULKAN_PTR 无条件落位（NULL 也照设）；DRIVER_PATH 保留为 ZL2
   GameLauncher.kt:189 同款「仅设不读」语义（turnip 包 = 驱动插件等位物）；
   POJAV_ZINK_PREFER_SYSTEM_DRIVER 开关读取降为日志证据（ZL2 产物中无行为差）。
3. **落盘名对齐 ZL2**：解包白名单 libzink_dri.so → libzink_dri.so（不再改名
   libgallium_dri.so）；预载清单同步；包代数 rev 2→3（老用户自动重装）。
4. **libcutils shim 删除**（ensure_zink_cutils_shim / libcutils_shim.c /
   libqzcutils.so）—— 不再用自研 shim 覆盖 ZL2 原版库。
5. **装载环境对齐 ZL2**：LD_LIBRARY_PATH 改 ZL2 getLibraryPath() 同款顺序
   （lwjgl-natives:/system/lib64:/vendor/lib64:/vendor/lib64/hw:/system_ext/lib64:
   渲染层目录:runtime_dir）；POJAVEXEC_EGL 改裸名（ZL2 同款）+
   SDL_EGL_LIBRARY 绝对路径（ZL2 同款）；LIBGL_DRIVERS_PATH 不再设置
   （ZL2 不设，loader 自然落到 LD_LIBRARY_PATH）。

## 保留（Quartz 侧诊断，不改变 ZL2 等价行为）

- 渲染层依赖预载两遍法（等价 ZL2 的 Java dlopen(libglxshim) + 失败日志点名）。
- log-only Vulkan 设备枚举探针（不参与任何胜负）。
- fork 隔离 EGL 预检安全网（真失败 → QUARTZ_RENDERER_EGL_FAIL → 回退链）。

## 验收判据（不变）

日志两行齐 = 天玑通路通了：
- `QUARTZ_VK_PROBE:OK:system ... devices=N:gpu0=Mali-...`
- `渲染层 EGL 预检通过：zink`
