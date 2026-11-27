# Quartz v0.2.19 —— ZL2 同款机制的完全体：MediaTek 小米系统 libvulkan 包装层兼容 + RimDroid 救场链移植

## 直接回答：v0.2.18 还不是一模一样，差在这一口

launch-1795769865 日志实锤的死因：

```
stage=zl2-loader-load:dlopen failed: library "libgpud_sys.so" not found:
needed by /data/user/0/com.quartz.launcher/files/jres/qzvulkan_000.so
```

你这台设备（xaga，天玑 8100，Mali-G610）的 **系统 `/system/lib64/libvulkan.so` 本身是个包装层** —— 它的 DT_NEEDED 直接挂着 `libgpud_sys.so`，而这块库的本体在 **`/system_ext/lib64/`**。普通 app（包括 ZL2 跑 zink 的常规路径）走系统默认链接器命名空间，天然看得到 /system_ext；Quartz v0.2.18 的 driver_namespace 搜索路径只有 `/system/lib64` + vendor/odm —— **permitted 允许了 /system_ext，搜索路径却没写它**，于是全新 loader 实例在第一步 dlopen 就死，zink 根本没得到机会。

这不是猜测：RimDroid/ValDroid/StormDroid（Unity 上安卓项目）的源码注释原文就是同款事故 —— *"The loader too can need vendor libraries (libgpud_sys.so on a MediaTek Xiaomi, 2026-10-07)"*，VectorJet/Mali-G77 的测试代码也实锤了 `libgpud_sys.so` 位于 `/system_ext/lib64/`。

## v0.2.19 改了什么（全部是已实证的移植，非自研）

1. **命名空间搜索路径补全**（zl2_linker.rs）：追加 `/system_ext/lib64`、`/system_ext/lib64/ndk`、`/system/lib64/ndk`、`/vendor/lib64/ndk`、`/odm/lib64/ndk` —— libgpud_sys.so 从搜索路径直达，首载即成。
2. **RimDroid vkshim 救场链全量移植**（liblinkernsbypass 的 Rust 复刻）：
   - 首载失败 → 建 **default_copy**（SHARED + parent=null + caller=libdl 实体地址，得到 default 命名空间的可用副本）→ `__loader_android_link_namespaces_all_libs` 全量连回 → 重试；
   - all_libs 符号缺失的旧 ROM：按 dlerror 解析缺失 SONAME（就是日志里那个 `library "X" not found`）定向 `__loader_android_link_namespaces` 补位 —— 该 API 全版本必有，救场永不落空；
   - 仍失败 → `__loader_android_get_exported_namespace("sphal")` 厂商库命名空间同法连回再重试一次。
   - 胜出档位随探针报告落日志：`QUARTZ_VK_PROBE:OK:zl2-ns search-path|...` / `default-copy|...` / `sphal|...`，一眼看出走的是哪一档。
3. **探针诊断加厚**：厂商 ICD 按规范只导出 `vk_icdGetInstanceProcAddr`（你这台的 mali/mt6895 blob 即如此，v0.2.18 日志的 `stage=dlsym-vkGetInstanceProcAddr` 就是它），现在会用 ICD 接口入口把探针跑完，日志给出「ICD 实际可用设备数」证据 —— 胜负语义不变（ICD-only 句柄对 mesa 不可消费），但下次日志能直接判断厂商 blob 通道的含金量。

## 测试判据（启动日志）

- `QUARTZ_VK_PROBE:OK:zl2-ns search-path|devices=N:gpu0=Mali-G...`（或 `default-copy|` / `sphal|`）= 全新 loader 实例装上了，厂商 Mali ICD 被发现；
- 随后 `渲染层 EGL 预检通过：zink`，游戏以桌面 GL（zink → 系统 loader → Mali blob）进世界；
- 若探针仍败，`stage=zl2-loader-load:救场链全败（...）` 会带上每一级的 dlerror 原文，回传一行即可定位到步。

## 交付验证（四 bug 收口）

- 控件错乱 / 虚拟鼠标 / 分辨率 DPI：已收口（v0.2.14/15）。
- zink：v0.2.15 安全网 → v0.2.16 回退自愈 → v0.2.17 驱动供给 → v0.2.18 ZL2 同款机制 → **v0.2.19 机制完全体（本版）**。
