---
title: 路线图
description: M0–M3 里程碑与持续红线
---

## M0 核心可跑 —— 当前阶段（基本完成）

- [x] Cargo workspace：quartz-core / quartz-cli / quartz-ffi，clippy 零告警
- [x] model / rules / meta / download：piston-meta 模型、规则引擎、并发下载
      （SHA-1 校验、断点跳过、进度事件）
- [x] instance / jvmops：实例体系 + LaunchPlan 装配（五端统一契约）
- [x] auth：离线 UUID + 微软设备码/XBL/XSTS 全链路模型
- [x] modkit / renderbus / inputbus：.mrpack 解析、渲染路由矩阵、输入协议
- [x] uipack：manifest 校验 / 灰度（FNV-1a）/ 回滚指针 / staging 任务
- [x] POC-A：Tauri 2 Android 壳 + core 真实下载链路
- [x] Android 完整文件权限 + 公共目录迁移
- [x] M0 启动链路：JRE pack 下载/解包/校验 + JVM 进程内启动 + launch 事件 + 控制台
- [x] v0.2.8：Svelte 5 组件化 UI（2026 Aurora 视觉），40 command / 12 事件契约
- [ ] 移动端画面输出：Pojav LWJGL 移植（自定义 GLFW → Android Surface 桥）—— M1 首项
- [ ] POC-B：鸿蒙 ArkWeb 薄壳加载同一 ui-pack + NAPI 桥接
- [ ] WoA 验证：OpenJDK windows-aarch64 自构建 + 启动 1.20.4

## M1 双移动端（3–6 月）

- [ ] Android 端到端：JRE pack 分发 + 进程内 JVM 启动 vanilla
- [ ] iOS 侧载版：静态 JVM + JIT 方案矩阵（TrollStore/AltStore/SideStore）+ MetalANGLE
- [ ] SwiftUI/Tauri iOS 壳 + macOS ARM 桌面端全功能（Zulu 子进程）

## M2 桌面 + 内容（6–9 月）

- [ ] Windows ARM64 壳（Dozen 渲染 pack）
- [ ] Forge/NeoForge/OptiFine 安装器编排；Fabric/Quilt 元数据接入
- [ ] Modrinth + CurseForge 双 API；mrpack 导入 → 实例
- [ ] 实例目录兼容 Prism / 官方 .minecraft 导入

## M3 鸿蒙 + 生态（9–12 月）

- [ ] ArkUI/ArkWeb 壳 + NAPI-RS 绑定 + musl OpenJDK 17/21 pack
- [ ] 渲染器/JRE pack 在线仓库（清单 + 签名 + 分平台分发）
- [ ] ui-pack 正式上线（灰度/回滚/遥测）

## 持续红线

- 无 JIT/加速路径的平台一律不出正式版（**性能红线**）
- 热更只改 UI 与内容，不触碰 command 契约（**合规红线**）
- 不分发游戏本体；MS OAuth 走官方流程（**合规红线**）
