# 路线图（对应立项书 M0–M3）

## M0 核心可跑（1–3 月）—— 当前阶段

### 已完成（本次基础）

- [x] Cargo workspace：quartz-core / quartz-cli / quartz-ffi 三 crate，clippy 零告警
- [x] model：piston-meta 模型（manifest / 版本 JSON / 资产索引）+ Maven 坐标路径
- [x] rules：Mojang 规则引擎（os/arch/features，末条匹配生效语义）+ 5 项单测
- [x] meta：清单获取与解析、库/客户端/资产索引 → 下载任务推导
- [x] download：并发执行器（SHA-1/256/512 校验、断点跳过、重试、进度事件、失败即中止）
- [x] instance：实例体系（隔离目录 + 共享仓库布局，instances.json 原子写）
- [x] jvmops：LaunchPlan 装配（现代/旧版 JVM 参数路由、natives classifier 选择、
      `${var}` 展开器、natives jar 提取）——五端统一契约
- [x] auth：离线 UUID（Java nameUUIDFromBytes 语义）+ 微软设备码/XBL/XSTS 全链路模型
- [x] modkit：.mrpack 解析（env.client 过滤）+ Fabric/Quilt/NeoForge 元数据入口
- [x] renderbus：渲染 pack 目录与选择矩阵（五端路由 + 性能红线）+ 3 项单测
- [x] inputbus：InputEvent 协议 + 触摸→指针/视角状态机（含 grab 卡键保护）+ 5 项单测
- [x] platform：HAL 契约 + 桌面子进程实现 + 移动端占位
- [x] uipack：manifest 解析/校验（schema/版本/路径安全）+ 灰度判定（FNV-1a）+ 回滚指针
      + SHA-256 staging 任务生成 —— L2 热更协议核心侧落地
- [x] quartz-cli：qz plan / versions / renderers / instances / mrpack / uuid（全部离线可跑）
- [x] quartz-ffi：C-ABI 稳定边界样例（qz_version / offline_uuid / renderer_select / plan_summary）
- [x] Tauri 2 壳 + Web UI 脚手架（4 个 command，浏览器降级桩，桌面可 dev）
- [x] CLI：qz uipack-check 演示命令
- [x] LICENSE（GPL-3.0-or-later 官方文本）+ CI（fmt/clippy/test）+ 三份设计文档

### M0 剩余

- [x] 两周 POC-A：Tauri 2 Android 壳 + core 跑通真实下载（piston-meta 在线）
- [x] Android 完整文件权限：targetSdk 28 + READ/WRITE_EXTERNAL_STORAGE 运行时授权，
      游戏根目录授权后切换 /storage/emulated/0/Quartz/files，旧数据后台迁移
- [x] M0 启动链路：JRE17 pack 下载/解包/校验（Pojav android-openjdk，aarch64 原生 ELF）
      + LaunchPlan 装配 + JVM 子进程启动 + launch://log / launch://exit 事件 + 启动控制台
- [ ] 移动端画面输出：Pojav LWJGL 移植（自定义 GLFW → Android Surface 桥）—— M1 首项
- [ ] 两周 POC-B：鸿蒙 ArkWeb 薄壳加载同一 ui-pack + NAPI 桥接（方案 A 最后一处不确定点）
- [ ] WoA 验证：OpenJDK windows-aarch64 自构建 + 启动 1.20.4
- [ ] UniFFI 绑定生成（Kotlin/Swift）接入 CI

## M1 双移动端（3–6 月）

- [ ] Android 端到端：JRE pack 分发（复用 Amethyst 运行时资产）+ 进程内 JVM 启动 vanilla
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

- 无 JIT/加速路径的平台一律不出正式版（性能红线）
- 热更只改 UI 与内容，不触碰 command 契约（合规红线）
- 不分发游戏本体；MS OAuth 走官方流程（合规红线）
