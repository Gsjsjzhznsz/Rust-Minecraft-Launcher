---
title: 快速上手
description: 从零到进入一张 Minecraft 世界
---

第一次使用 Quartz Launcher，五步即可进入游戏。

## 1. 登录账号

首次打开应用会进入登录页，两种方式任选：

- **离线模式**：输入任意用户名即可。UUID 与 Java 官方
  `nameUUIDFromBytes("OfflinePlayer:<name>")` 算法完全一致，
  单人世界与多数离线服务器可直接使用。
- **微软账户**：设备码登录，全程官方 OAuth 流程（见[账号与登录](/Rust-Minecraft-Launcher/guides/account/)）。

## 2. 下载一个版本

进入「下载游戏」页：

1. 版本列表来自 Mojang piston-meta 清单（支持正式版 / 快照 / Beta / Alpha 过滤与搜索）。
2. 选中版本点「下载」，进度按阶段展示：版本清单 → 版本 JSON → 客户端 jar →
   依赖库 → 资源索引 → 资源文件。
3. 已下载的文件自动跳过（SHA-1 校验），中断后重试不从头再来。

## 3.（可选）创建实例

「实例」页可以为不同玩法建立隔离目录：每个实例独立的 saves / mods /
resourcepacks，共享同一份 assets / libraries 仓库，不浪费空间。
不创建实例也可以直接启动，文件写入标准 `.minecraft` 布局。

## 4. 启动

回到主页，选中版本点「启动」：

- 运行时会自动就位：缺少对应 Java 版本时，应用自动下载并解包 Pojav 系
  OpenJDK pack（8 / 17 / 21，aarch64 原生）。
- 渲染层自动按版本路由：1.17+ 使用 Zink (Vulkan) 档，详见[渲染层选择](/Rust-Minecraft-Launcher/guides/renderers/)。
- 启动后进入「控制台」可实时查看 `launch://log` 输出，游戏退出会给出退出码与日志尾部。

## 5. 装点内容（可选）

「内容」页集中管理 mods / 资源包 / 光影 / 存档 / 截图：

- Fabric / Quilt 系装载器可直接在实例上安装（版本列表在线获取）。
- 整合包（`.mrpack`）导入后自动落进新实例（按路线图 M2 完善编排）。

## 出问题了？

- 启动失败先看主页「诊断」横幅与日志页的 `latest.log` 尾部。
- 资源文件校验失败可在「下载游戏」页一键「修复资产」。
- 崩溃信息（tombstone / hs_err）会被自动回收，方便反馈定位。
