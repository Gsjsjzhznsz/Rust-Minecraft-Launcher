# Quartz v0.2.20 —— 「跟 ZL2 一模一样」：制品 + 装载机制双 1:1

## 之前差在哪（三处，全部钉死）

你这句「还不是一模一样」是对的。逐项侦查 ZL2（ZalithLauncher2）源码 + 官方 APK 实物比对，差异有三：

1. **制品源不是 ZL2 的**：zink 包此前取自 IronizedZink v1.1.0（第三方重打包）。2026-11-27 sha256 比对：其 Mesa 五件套（libEGL_mesa / libglapi / libglxshim / libzink_dri / libcutils）与 ZL2 2.8.2 官方 arm64 APK 内置的**逐字节相同**，但 `libc++_shared.so` 不同 —— IZ 是 9.3MB 胖 NDK 构建，ZL2 是官方 NDK r25c 构建（1.0MB）。
2. **Vulkan 装载机制不是 ZL2 的**：v0.2.15~19 是「探针编队（厂商 ICD 直连 / 厂商自动发现 / 验证制 loader 兑底）→ 全败不设 VULKAN_PTR → ZINK_SKIP_PRECHECK fail-fast → 自动回退 mobileglues」。ZL2 的 `load_vulkan()`（egl_bridge.c）根本不是这套：Turnip（仅 Adreno，`checkAdrenoGraphics()` 先行，Mali 直接 NULL）→ 兜底**默认命名空间普通 `dlopen("libvulkan.so")`** → **VULKAN_PTR 无条件落位（句柄为 NULL 也照设）** → 成败交给 Mesa EGL 初始化实战。我们的探针门禁在你这台天玑上把 zink 挡死在 EGL 之前 —— 你上传的 game 日志就是铁证：全程 mobileglues，zink 连预检都没进（探针全败即 fail-fast，连日志都懒得留给你）。
3. **env 配方缺项**：ZL2 `GameLauncher.kt` 的 `needsZinkDriverOverride` 分支设 `MESA_GL_VERSION_OVERRIDE=4.6`、`MESA_GLSL_VERSION_OVERRIDE=460`、`LIB_MESA_NAME=libglxshim.so`；我们 v0.2.13 曾以「虚报驱动能力」为由删掉版本覆盖 —— 实锤 ZL2 现行代码就是设，一模一样原则恢复。

## v0.2.20 改动（全部对齐 ZL2，非自研）

1. **制品 1:1**：CI 新增 `scripts/build_zinkpack.sh` —— 从 ZL2 官方 Release APK（2.8.2，sha256 锁定 `532503ad…`）字节级提取 Mesa 全套出 `renderer-zink-aarch64.zip`（与 turnip 包同款双名直链发布）；`libzink_dri.so` 落盘改名 `libgallium_dri.so` 对齐 SONAME（ZL2 内置该库 DT_SONAME 即 libgallium_dri.so）。包代数标记 `.pack-rev=2`：旧 IronizedZink 装包自动判失效，启动前自动重装。
2. **装载机制 1:1**：`load_vulkan` 三步 —— ① Turnip zl2 通道仅 Adreno（保留 v0.2.18/19 移植的 zl2_linker，含救场链），`POJAV_ZINK_PREFER_SYSTEM_DRIVER` 开关同款；② 兜底默认命名空间 `dlopen("libvulkan.so")`；③ VULKAN_PTR 无条件落位。探针编队与 fail-fast **删除**；设备枚举降级为纯日志证据（`devices=N` 照旧落日志，不参与任何胜负）。DRIVER_PATH 同款落位（ZL2 GameLauncher.kt initEnv 同款）。
3. **env 配方 1:1**：launch_env 补 `MESA_GL_VERSION_OVERRIDE=4.6` / `MESA_GLSL_VERSION_OVERRIDE=460` / `LIB_MESA_NAME=libglxshim.so`。

## 天玑通路（你这台 xaga 为什么这次真的走 ZL2 的路）

Mali 设备无 `/dev/kgsl-3d0` → turnip 包不安装 → Turnip 通道直接跳过 → **默认命名空间 dlopen 系统 libvulkan**。你这台小米的系统 libvulkan 是包装层（DT_NEEDED 挂 `libgpud_sys.so`，本体在 `/system_ext/lib64`）—— 默认命名空间天然看得见 /system_ext，这正是 ZL2 在同款设备上跑通 zink 的机制。v0.2.15 起的 fork 隔离 EGL 预检保留为安全网（只拦真失败，ZL2 成功路径原样保留）。

## 验收判据

装后选 zink 启动，看日志：

```
[quartz] zink：VULKAN_PTR=系统 loader（ZL2 兜底通道：默认命名空间 dlopen libvulkan.so）
QUARTZ_VK_PROBE:OK:system devices=N:gpu0=Mali-G…
[quartz] 渲染层 EGL 预检通过：zink（EGL 4.x，surfaceless 平台）
```

三行齐了 = zink 真跑起来了。若 EGL 预检仍失败，回退链照旧兜底，日志会带 Mesa 诊断（MESA_DEBUG + logcat 采集）与设备枚举证据，回传一行即可定位。

## 工程记录

- quartz-core 72 测试全绿（zink 包源/rev/launch_env 断言随本版更新）；aarch64-linux-android check/clippy 零新增告警（8 条全部为 v0.2.19 存量，逐条对照确认）。
- 修死亡通道遗留：load_sphal_library / open_icd_handle / vendor_icd_candidates 三个死函数随探针编队删除。
- zink 包体积 14.5MB（6 so）；Turnip 驱动包维持 CI 自建（社区预编译按 vulkan-android.sym 只导 HMI 入口，VULKAN_PTR 消费端不可用 —— renderers.rs 注释存证）。
