# Quartz v0.2.17 —— 骁龙 zink 复活：内置 Turnip 开源 Vulkan 驱动（Mesa KGSL）

## 一句话
你要的 zink 在骁龙上回来了：**v0.2.17 起，选 zink 的骁龙设备会自动下载我们自建的 Turnip 开源 Vulkan 驱动（免 root，Mesa 26.3.1 KGSL 后端），zink 从此有真驱动可用**——这正是 ZL2/FCL 的 zink 能跑的根本原因（他们靠的从来不是厂商 Adreno 驱动）。

## 为什么骁龙之前不行（v0.2.15/16 日志复盘）
1. 厂商 Adreno blob 在普通应用进程里不可用：ICD 直连 dlsym 拿不到入口符号（stage=dlsym-vkGetInstanceProcAddr），系统 loader 路径在隔离子进程里 SIGSEGV（v0.2.15 起被 fork 隔离关进笼子，不再带走游戏进程）——这是 Android 厂商驱动的进程边界，不是我们的 bug。
2. `/dev/dri/renderD*` 不存在（无 DRM 节点），zink 无法走 DRI 设备发现。
3. v0.2.16 的保命网按设计工作：zink 探测全败 → 秒级干净回退 MobileGlues → 游戏照常进世界（你的日志已实证：mobileglues 在线补装 → 完整进入存档）。

## v0.2.17 的解法
- **CI 从 mesa-26.3.1 源码自建 Turnip**（`scripts/build_turnip.sh`）：
  - `-Dfreedreno-kmds=kgsl`（免 root 直接 ioctl `/dev/kgsl-3d0`，Termux/omarchy 同款已实证路径）
  - `-Dplatforms=android`（Android WSI，配合 zink 的呈现路径；假链接库仅链接期使用，部署时用系统真库）
  - 补丁 `scripts/mesa-turnip-plain-gpa.patch`：mesa 的 Android ICD version script 默认只导出 HAL `HMI`（这就是 FCL/ZL2 预编译 turnip「没有标准入口」的根源），我们同时导出 `vk_icdGetInstanceProcAddr` + `vkGetInstanceProcAddr`，让 zink 的 VULKAN_PTR 直连消费端（IronizedZink 只 dlsym 后者）无需任何 shim 直接可用。
  - 产物 `renderer-turnip-aarch64.zip`（约 3.5MB：libvulkan_freedreno.so + libc++_shared.so + freedreno_icd.json）随本 Release 发布，**运行时在线下载，不内置 APK**（延续渲染层 pack 设计原则）。
- **探针链升级**：厂商 ICD → **Turnip 包**（新）→ 系统 loader。Turnip 探测同样 fork 隔离（崩溃只损失隔离子进程）；胜出后 VULKAN_PTR 落位 + 包内 ICD json 落给 loader 兑底通道（双保险）。
- **KGSL 门控**：仅 `/dev/kgsl-3d0` 存在（Adreno）的 zink 启动会预取驱动包；Mali 等设备不下载不探测，行为与 v0.2.16 完全一致（zink 在 Mali 上无开源驱动可用，MobileGlues 仍是最优解）。
- 启动日志判据：`QUARTZ_VK_PROBE:OK:icd .../renderers/turnip/libvulkan_freedreno.so ... gpu0=Turnip (Adreno ...)` = zink 已拿到真驱动。

## 交付验证（四 bug 收口）
- 控件错乱 / 虚拟鼠标 / 分辨率 DPI：已收口（v0.2.14/15）。
- zink：v0.2.15 安全网（不崩）→ v0.2.16 回退自愈（可玩）→ **v0.2.17 驱动供给（骁龙可真跑 zink）**。

## 说明
- Mali 设备（如 Mali-G57）：panvk/Turnip 均不支持，zink 无现实路径，MobileGlues 是正解。
- Turnip 对 Adreno 6xx/7xx/8xx 全系支持（mesa 26.3.1）。若个别设备 KGSL 节点被 SELinux 收紧，探测会失败并自动回退 MobileGlues（日志有信号号详情可回传）。
