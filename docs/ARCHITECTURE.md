# Quartz 架构设计 v0.1

> 一句话：把 Pojav/FCL/ZL2/Prism/Amethyst 五家十年踩出来的 C/C++ 资产当作可插拔积木，
> 用 Rust 重写恰好被每家重复写了三遍的「启动器大脑」，五端只写壳。
> UI 路线为方案 A：Tauri 2 / 系统 WebView，支持 ui-pack 实时更新（见 UI-PACK.md）。

## 1. 分层

```
┌────────────────── 一套 UI（五端同构，Web 技术栈）──────────────────┐
│  Android WebView │ iOS WKWebView │ macOS/Win WebView │ 鸿蒙 ArkWeb │
│  ▲ ui-pack：版本化、签名、CDN 分发、热更、灰度、秒级回滚             │
├────────────────────────────────────────────────────────────────────┤
│  绑定层：                                                           │
│  Android/iOS：Tauri 2（进程内，系统 WebView）                        │
│  鸿蒙：ArkTS 薄壳 + ArkWeb + NAPI-RS 调 Rust                        │
│  桌面：Tauri 2（macOS ARM / Windows ARM / Linux）                   │
├──────────────── Quartz Core（Rust，纯逻辑，零平台依赖）─────────────┤
│  model/rules    元数据模型 + 规则引擎                                │
│  meta           版本清单 → 下载计划                                  │
│  download       并发下载（SHA1 校验 / 断点跳过 / 进度事件）          │
│  instance       实例体系（隔离目录 + 共享 assets/libraries 仓库）    │
│  jvmops         JVM 启动计划装配（五端统一契约 LaunchPlan）          │
│  auth           离线 UUID / 微软设备码 + XBL 链路                    │
│  modkit         .mrpack 整合包 / Fabric-Quilt-NeoForge 元数据       │
│  renderbus      渲染路由（gl4es/ANGLE/Zink/MetalANGLE/Dozen 矩阵）  │
│  inputbus       输入协议（触摸→虚拟指针/视角状态机）                 │
│  uipack         ui-pack 清单/校验/灰度/回滚（L2 热更核心侧）         │
├──────────────── 平台 HAL（Rust，按 target 编译）────────────────────┤
│  desktop: 子进程 java │ android/ios/harmony: 进程内 JVM（宿主壳启动）│
├──────── C/C++ 运行时资产（不重写！可插拔 pack 随包/在线分发）────────┤
│  JVM pack    OpenJDK 8/17/21 移植版（复用 Pojav 系构建产物）         │
│  渲染 pack   gl4es / ANGLE / Zink / MetalANGLE / MoltenVK / Mesa    │
│  桥接        移植版 LWJGL3 / GLFW-shim / Caciocavallo                │
└────────────────────────── 游戏进程：MC Java ────────────────────────┘
```

三条设计铁律（每条来自竞品教训）：

1. **不重写 C/C++ 组件**。gl4es、ANGLE、Zink、LWJGL 移植是十年社区积累，
   Rust 只做编排；运行时资产以 pack 形式复用（Pojav/Amethyst/AMCL 已验证其许可与可用性）。
2. **移动端进程内 JVM，桌面端子进程**。Android/iOS/鸿蒙沿用 `JNI_CreateJavaVM`
   进程内模型（Amethyst 已验证）；macOS ARM 与 Windows ARM 用子进程，崩溃隔离、实现简单。
3. **核心与壳的契约只有一份**。UI 允许热更（Web 资源），command 契约向后兼容；
   核心能力先在 quartz-core 实现，再由壳层暴露。

## 2. 五端矩阵（调研结论冻结）

### JVM

| 平台 | JVM 来源 | 进程模型 | JIT | 风险 |
| --- | --- | --- | --- | --- |
| Android 8+ | Pojav 系 OpenJDK 8/17/21 | 进程内 | 有 | 低 |
| iOS 14+ | 静态链接 OpenJDK（Amethyst 路线） | 进程内 | 侧载 entitlement（TrollStore/AltStore） | 高（政策） |
| macOS ARM64 | Azul Zulu aarch64 | 子进程 | 有 | 低 |
| Windows ARM64 | OpenJDK 主线 windows-aarch64 移植（JDK16+），发行版供货核实中，退路 x64 仿真 | 子进程 | 有 | 中 |
| 鸿蒙 NEXT | musl 交叉构建 OpenJDK 17/21（AMCL 已验证） | 进程内 | 有 | 中 |

### 渲染

| pack | 原理 | 适用 | 对应 core 类型 |
| --- | --- | --- | --- |
| gl4es | GL2.1→GLES2 | ≤1.16 / 低配兜底 | `Profile20 + Gles2` |
| ANGLE | GLES3→Vulkan/Metal/D3D | 1.17+ 基础档 | `Profile32 + Gles3` |
| Zink | GL4.6→Vulkan | 1.17++ / Sodium / 光影 | `Profile46 + Vulkan` |
| MetalANGLE | GL→Metal | iOS 主力 | `Profile32 + Metal` |
| Mesa Dozen | GL→D3D12 | Windows on ARM 主力 | `Profile46 + D3d12` |
| native-gl | 桌面驱动 GL | macOS ARM / Linux | `Profile46 + NativeGl` |

选择矩阵已实现于 `crates/quartz-core/src/renderbus.rs` 并有单测覆盖；
`1.17+ 需 GL3.2` 的版本路由（含快照/远古版本命名）由 `required_profile()` 决定。

## 3. 关键流程

### 3.1 启动一次游戏

```
UI: 选择实例 → invoke("plan_offline"/未来 plan)
core: 实例配置 + 版本 JSON + 规则过滤 → LaunchPlan
      （JVM 参数以版本 JSON arguments.jvm 为准，启动器只补缺 -Xmx/-cp）
HAL: 桌面 → Command(java, jvm_args…, main_class, game_args)
      移动 → LaunchPlan 交宿主壳的 JNI_CreateJavaVM 封装层
renderbus: 所需 GL 档位（required_profile）→ pack 选择 → 注入 natives/渲染 so
```

### 3.2 实时更新（方案 A 的核心优势）

三层解耦：
- **L1 内容实时**：公告/整合包榜单/服务器列表走 server-driven JSON，五端同步刷新；
- **L2 UI 热更**：ui-pack（版本化 Web 资源）从 CDN 拉取，校验后原子替换，改界面不发版；
- **L3 核心**：Rust core 与 JVM/渲染 pack 走常规发版，仅 pack 支持在线更新。

iOS 合规边界：上架包内嵌完整可用 UI，热更只改界面与内容（Apple 3.3.2 红线内）。

## 4. 目录结构

```
crates/quartz-core     启动器大脑（本仓库核心，37 项单测）
crates/quartz-cli      离线可跑的 CLI（qz plan / renderers / instances / mrpack / uuid / uipack-check）
crates/quartz-ffi      C-ABI 稳定边界样例（生产绑定走 UniFFI / NAPI-RS）
apps/shell-tauri       Tauri 2 壳 + Web UI（桌面可运行；移动端 M1 启用）
docs/                  架构 / 路线图 / ui-pack 协议
LICENSE                GPL-3.0-or-later
```

## 5. 许可边界

- 若复用 Pojav 系核心代码 → 本项目必须 GPL-3.0（当前 workspace 选择）；
- 只复用其**构建产物**（JRE/渲染器 pack，各自许可）则核心可保持宽松许可，待社区定案；
- 微软认证必须用项目自有 Azure client_id（`auth::microsoft::DEFAULT_CLIENT_ID` 留空即拒绝登录）；
- 不分发游戏本体；离线模式标注灰色地带。

## 6. 竞品对照（调研摘要）

| 项目 | 现状 | 对 Quartz 的输入 |
| --- | --- | --- |
| PojavLauncher | Android 主仓 2025-09 归档 → Amethyst-Android 接棒 | JVM 移植、渲染 pack、进程内模型 |
| FCL | Pojav 二开，Modrinth/CurseForge 原生集成 | 内容层范式 |
| ZalithLauncher2 | Compose UI 重写，核心仍用 Pojav | 证明「壳易换、核心是壁垒」 |
| Prism Launcher | 桌面 C++/Qt 标杆 | 实例体系 / 内容 API 范式 |
| Theseus (Modrinth App) | Tauri 2 + Rust 核心生产级（桌面） | Rust 核心可行性先例 |
| Amethyst-iOS | Pojav_iOS 继任 | iOS 静态 JVM + JIT entitlement 路线 |
| AMCL / HomoLauncher | 鸿蒙 NEXT 原生启动器（GPL-3.0） | 鸿蒙 musl OpenJDK 打包可行性 |
