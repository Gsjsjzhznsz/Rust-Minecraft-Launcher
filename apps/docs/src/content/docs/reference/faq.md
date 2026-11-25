---
title: 常见问题
description: 安装、兼容性与合规相关的高频问题
---

## 和 PojavLauncher / FCL 是什么关系？

Quartz 不重复造轮子：复用 Pojav 系验证过的 C/C++ 运行时资产（OpenJDK 移植、
渲染 pack、LWJGL 移植），但「启动器大脑」——元数据、下载编排、实例、JVM 装配、
认证、渲染路由——全部用 Rust 重写（quartz-core），UI 是五端同构的 Web 层。
若未来复用 Pojav 系核心代码，项目将遵循 GPL-3.0 传染要求。

## 支持哪些设备和系统？

当前 APK 面向 **Android 8.0+，arm64-v8a**。iOS / macOS (ARM) / Windows (ARM) /
鸿蒙 NEXT 按路线图在 M1–M3 逐步落地。

## 支持哪些 Minecraft 版本？

理论上覆盖 piston-meta 清单中的全部 Java 版本（正式版 / 快照 / Beta / Alpha），
实际可用性取决于渲染层档位：1.17+ 需要 GL 3.2+，走 Zink (Vulkan)；
更老版本走 GLES2 兜底。

## 网页演示版能真的玩游戏吗？

不能。演示版只是 UI 层 + 内置模拟后端，用来展示界面与交互；
下载、启动等动作都是模拟动画。真正玩游戏请安装 APK。

## 安装 APK 提示失败 / 签名不一致？

从旧版本升级前先卸载旧版。构建密钥变更后，Android 会拒绝在旧版之上覆盖安装。

## 1.17+ 启动后白屏 / 黑屏？

1. 确认「设置 → 渲染层」为 Zink (Vulkan) 且已安装完成；
2. 仍异常可切换 ANGLE (Vulkan) 兼容档；
3. 查看主页诊断横幅与日志页 `latest.log` 尾部，反馈时附上。

## 游戏文件放在哪里？

- 未授权存储：应用私有目录（卸载即丢，不推荐）；
- 已授权：`/storage/emulated/0/Quartz/files`（公共目录，卸载不丢，可被文件管理器浏览）。

## 会分发游戏本体吗？

不会。Quartz 只下载并管理 Mojang 官方分发的游戏文件，这属于合规红线；
微软 OAuth 也只走官方流程与项目自有 client_id。

## iOS 版什么时候有？

iOS 需要 Amethyst 路线的静态 JVM + JIT entitlement（TrollStore / AltStore 侧载），
属于路线图 M1，政策风险较高，无 JIT 的平台不会出正式版（性能红线）。
