---
title: 安装
description: 下载并安装 Quartz Launcher Android APK
---

Quartz Launcher 目前提供 **Android（arm64-v8a）** 安装包，由 CI 从源码云端构建并发布到 GitHub Releases。

## 系统要求

| 项目 | 要求 |
| --- | --- |
| 系统 | Android 8.0（Oreo）及以上 |
| 架构 | arm64-v8a |
| 存储 | 建议 4GB 以上剩余空间（游戏本体 + 资源约 1–2GB，另有 JVM / 渲染层 pack） |
| 网络 | 下载游戏文件需要访问 Mojang / 镜像源 |

## 下载 APK

从 [GitHub Releases](https://github.com/Gsjsjzhznsz/Rust-Minecraft-Launcher/releases) 获取最新版：

- 每个版本都经过 CI 全新构建（Rust 桥 + hook jar + Tauri Android 壳 + 签名收尾），
  Release 资产名形如 `Quartz-v0.2.8.apk`。
- 也可以在仓库的 Actions 页面下载任意一次构建的 `Quartz-debug-apk` Artifact（30 天保留期）。

## 安装步骤

1. 下载 APK 后，系统可能提示「未知来源应用」—— 允许本次安装即可。
2. **从旧版本升级前，请先卸载旧版**：构建密钥变更会导致安装失败（提示签名不一致）。
3. 首次启动后，建议在应用内完成**存储授权**（见下节），否则游戏数据保存在应用私有目录，卸载即丢失。

## 存储授权（重要）

Quartz 提供两种存储模式：

- **应用私有目录**（默认）：无需任何授权，但卸载应用会连带丢失全部游戏数据。
- **公共目录**（推荐）：授予完整文件权限后，游戏根目录切换到
  `/storage/emulated/0/Quartz/files`，可用文件管理器直接浏览，卸载应用不丢数据；
  切换时旧数据会自动后台迁移。

路径与剩余空间可在「设置」页随时确认；资产布局异常时可用「重建存储」一键修复。
