# Quartz v0.2.21 —— 「zink 下载失败」修复：包↔清单契约接缝

## 死因（一行说清）

v0.2.20 的 CI 打包脚本把 zip 里的 `libzink_dri.so` 改名成了 `libgallium_dri.so`，而 App 端提取白名单还在找 `libzink_dri.so` —— 解包器对白名单外条目静默跳过，zink 的 gallium 驱动（libgallium_dri.so，12.5MB 那件）**永远没落盘**，装完校验发现预载清单缺件 → 报「渲染层安装不完整」→ 你看到的就是「zink 下载失败」。这是 v0.2.20 制品↔清单的接缝错位，100% 复现，与设备、网络无关（v0.2.17 起的 turnip 包走空白名单全提取模式，所以此前没炸）。

## v0.2.21 改动（三处，全部对着死因）

1. **CI 制品恢复 ZL2 原名（更彻底的一模一样）**：`build_zinkpack.sh` 不再改名 —— zip 条目与 ZL2 2.8.2 官方 APK 的 `lib/arm64-v8a/` **逐文件同名同字节**（libEGL_mesa / libglapi / libglxshim / **libzink_dri.so（原名）** / libcutils / libc++_shared，六件）。落盘改名（libzink_dri.so → libgallium_dri.so，对齐 DT_SONAME）归 App 解包白名单执行 —— 这是 v0.2.19 前 APK 布局契约的原样。
2. **白名单严格校验**：`extract_zip_libs` / `extract_apk_libs` 加命中簿记 —— 非空白名单缺任一条目，下载环节当场硬失败并**点名缺失文件**，不再静默跳过拖到装完才误报。
3. **装后校验报错列明细**：缺什么列什么（此前固定报「缺少 libEGL_mesa.so」，本次事故实为缺 libgallium_dri.so，误报误导）。

## 对你手上的 v0.2.20 意味着什么

下载直链是 `releases/latest/download/renderer-zink-aarch64.zip` —— v0.2.21 发布后该直链自动指向新 zip（ZL2 原名布局），**v0.2.20 的 App 白名单恰好匹配新 zip**：老 APK 重下 zink 即可，不装新包也能好。v0.2.21 额外带来严格校验与报错明细两道防线。

## 验收判据（与 v0.2.20 一致）

装好/重下后选 zink 启动，日志三行齐 = 通：

```
[quartz] zink：VULKAN_PTR=系统 loader（ZL2 兜底通道：默认命名空间 dlopen libvulkan.so）
QUARTZ_VK_PROBE:OK:system devices=N:gpu0=Mali-G…
[quartz] 渲染层 EGL 预检通过：zink（EGL 4.x，surfaceless 平台）
```

## 工程记录

- 回归防线测试：`zip_extract_strict_whitelist_fails_on_missing_entry`（复刻 v0.2.20 事故形态：清单要 libzink_dri.so 而包内没有 → 必须 Err 点名）；`zink_pack_has_source_and_preload` 把 CI 制品 ↔ App 清单六条契约逐条钉死，CI 侧再动制品名这里先红。
- quartz-core 测试全绿；CI zinkpack 步骤打印 `unzip -l`（六件 ZL2 原名清单）作发布物指纹证据。
- ZL2 一模一样状态不变：制品源仍为 ZL2 2.8.2 官方 APK 字节级提取（sha256 锁定 532503ad…），装载机制仍为 ZL2 load_vulkan 1:1（v0.2.20 已收口）。
