---
title: 架构设计
description: Quartz 的分层、五端矩阵与三条设计铁律
---

> 一句话：把 Pojav/FCL/ZL2/Prism/Amethyst 五家十年踩出来的 C/C++ 资产当作可插拔积木，
> 用 Rust 重写恰好被每家重复写了三遍的「启动器大脑」，五端只写壳。

## 分层

```text
┌────────────── 一套 UI（五端同构，Svelte 5 + 系统 WebView）──────────────┐
│  Android WebView │ iOS WKWebView │ macOS/Win WebView │ 鸿蒙 ArkWeb      │
│  ▲ ui-pack：版本化、签名、灰度、热更、秒级回滚                            │
├──────────────────────────────────────────────────────────────────────┤
│  绑定层：Tauri 2（Android/iOS/桌面进程内）│ 鸿蒙：ArkTS + NAPI-RS        │
├──────────── Quartz Core（Rust，纯逻辑，零平台依赖）─────────────────────┤
│  model/rules   元数据模型 + Mojang 规则引擎                             │
│  meta          版本清单 → 下载计划                                     │
│  download      并发下载（SHA1/256/512 校验、断点跳过、进度事件）          │
│  instance      实例体系（隔离目录 + 共享 assets/libraries 仓库）         │
│  jvmops        LaunchPlan 装配（五端统一契约）                          │
│  auth          离线 UUID / 微软设备码 + XBL / authlib                   │
│  modkit        .mrpack 整合包 / Fabric-Quilt-NeoForge 元数据            │
│  renderbus     渲染路由（gl4es/ANGLE/Zink/MetalANGLE/Dozen 矩阵）      │
│  inputbus      输入协议（触摸→虚拟指针/视角状态机）                     │
│  uipack        manifest 校验/灰度（FNV-1a）/回滚指针/staging            │
├──────────── 平台 HAL（Rust，按 target 编译）───────────────────────────┤
│  desktop: 子进程 java │ android/ios/harmony: 进程内 JVM（JNI_CreateJavaVM）│
├──── C/C++ 运行时资产（不重写！可插拔 pack 随包/在线分发）────────────────┤
│  JVM pack（OpenJDK 8/17/21）│ 渲染 pack │ LWJGL3 移植 / GLFW-shim       │
└────────────────────────── 游戏进程：MC Java ───────────────────────────┘
```

## 三条设计铁律

1. **不重写 C/C++ 组件**。gl4es、ANGLE、Zink、LWJGL 移植是十年社区积累，Rust 只做编排；
   运行时资产以 pack 形式复用（Pojav/Amethyst/AMCL 已验证其许可与可用性）。
2. **移动端进程内 JVM，桌面端子进程**。Android/iOS/鸿蒙沿用 `JNI_CreateJavaVM`
   进程内模型（Amethyst 已验证）；macOS ARM 与 Windows ARM 用子进程，崩溃隔离、实现简单。
3. **核心与壳的契约只有一份**。UI 允许热更（Web 资源），command 契约向后兼容；
   核心能力先在 quartz-core 实现，再由壳层暴露。

## 五端矩阵（JVM）

| 平台 | JVM 来源 | 进程模型 | 风险 |
| --- | --- | --- | --- |
| Android 8+ | Pojav 系 OpenJDK 8/17/21 | 进程内 | 低 |
| iOS 14+ | 静态链接 OpenJDK（Amethyst 路线） | 进程内 | 高（政策） |
| macOS ARM64 | Azul Zulu aarch64 | 子进程 | 低 |
| Windows ARM64 | OpenJDK 主线 windows-aarch64 | 子进程 | 中 |
| 鸿蒙 NEXT | musl 交叉构建 OpenJDK 17/21 | 进程内 | 中 |

## 竞品对照（调研摘要）

| 项目 | 现状 | 对 Quartz 的输入 |
| --- | --- | --- |
| PojavLauncher | Android 主仓 2025-09 归档 → Amethyst-Android 接棒 | JVM 移植、渲染 pack、进程内模型 |
| FCL | Pojav 二开，Modrinth/CurseForge 原生集成 | 内容层范式 |
| ZalithLauncher2 | Compose UI 重写，核心仍用 Pojav | 证明「壳易换、核心是壁垒」 |
| Prism Launcher | 桌面 C++/Qt 标杆 | 实例体系 / 内容 API 范式 |
| Theseus (Modrinth App) | Tauri 2 + Rust 核心生产级（桌面） | Rust 核心可行性先例 |
| Amethyst-iOS | Pojav_iOS 继任 | iOS 静态 JVM + JIT entitlement 路线 |
| AMCL / HomoLauncher | 鸿蒙 NEXT 原生启动器（GPL-3.0） | 鸿蒙 musl OpenJDK 打包可行性 |

## 许可边界

- 复用 Pojav 系核心代码 → 本项目必须 GPL-3.0（当前 workspace 选择）；
- 只复用其**构建产物**（JRE / 渲染器 pack，各自许可）则核心许可待社区定案；
- 不分发游戏本体；微软认证使用项目自有 Azure client_id。
