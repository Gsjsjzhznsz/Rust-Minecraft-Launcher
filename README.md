# Quartz Launcher

[![CI](https://github.com/Gsjsjzhznsz/Rust-Minecraft-Launcher/actions/workflows/ci.yml/badge.svg)](https://github.com/Gsjsjzhznsz/Rust-Minecraft-Launcher/actions/workflows/ci.yml)
[![Pages](https://github.com/Gsjsjzhznsz/Rust-Minecraft-Launcher/actions/workflows/pages.yml/badge.svg)](https://github.com/Gsjsjzhznsz/Rust-Minecraft-Launcher/actions/workflows/pages.yml)

**业界第一个 Rust 核心 + 五端同构的 Minecraft: Java Edition 启动器（立项代号 Quartz）。**

方案 A 架构：Tauri 2 / 系统 WebView UI（ui-pack 可实时热更）+ Rust 启动器核心
+ 各端原生游戏表面。目标平台：Android / iOS / macOS (ARM) / Windows (ARM) / 鸿蒙 NEXT。

设计依据来自五家竞品的调研：PojavLauncher（已归档→Amethyst）、FCL、ZalithLauncher2、
Prism Launcher、Amethyst-iOS（Pojav_iOS 继任），以及 Rust 侧先例 Modrinth App（Theseus）
与鸿蒙侧 AMCL/HomoLauncher 的可行性验证。详见 [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)。

## 在线站点（GitHub Pages，每日自动跟随 main 重建）

- **文档站**：<https://gsjsjzhznsz.github.io/Rust-Minecraft-Launcher/>
  （Astro + Starlight + starlight-theme-md3，源码在 `apps/docs/`）
- **UI 网页演示**：<https://gsjsjzhznsz.github.io/Rust-Minecraft-Launcher/demo/>
  （Svelte 5 UI + 内置模拟后端，仅供演示 —— 下载/启动均为模拟动画，真实功能请用 APK；
  源码在 `apps/shell-tauri/ui/`，由 `VITE_DEMO=1` 构建开关注入 mock）
  支持 21 种设计语言实时切换（五组：玻璃与光 / 产品语言 / 极简理性 / 质感趣味 / 极客霓虹；
  主打玻璃拟态 Glassmorphism），配色取材 [Style Atlas](https://weepwood.github.io/style-atlas/)
  各风格实测值，Token 契约见 [docs/UI-THEMES.md](docs/UI-THEMES.md)

## 当前状态（M0 基础已就位）

- `crates/quartz-core` —— 启动器大脑：元数据/规则引擎/下载编排/实例/JVM 装配/
  认证/整合包/渲染路由/输入协议/uipack/HAL，**37 项单测全绿，clippy 零告警**
- `crates/quartz-cli` —— 离线可跑的 CLI（`qz`）
- `crates/quartz-ffi` —— C-ABI 稳定边界样例（生产绑定走 UniFFI / NAPI-RS）
- `apps/shell-tauri` —— Tauri 2 壳 + Web UI（桌面可 dev；浏览器预览走桩数据）
- `docs/` —— 架构 / 路线图 / ui-pack 热更协议

## 功能层 1:1 对标 ZL2 / FCL

UI 视觉保持自有多设计语言体系（见 docs/UI-THEMES.md），**功能与操作逻辑对标
ZalithLauncher2 与 Fold Craft Launcher**，删多补缺：

**已对齐（本轮交付）**
- 导航同构：主页 / 下载 / 实例(管理) / 账户 / 设置（原「内容」一级页并入实例页分区，
  FCL ManageUI 7 Tab / ZL2 版本详情同款语义）
- 主页操作逻辑：当前版本卡（只列已安装）+ 一键启动（ZL2 操作菜单 / FCL 右栏版本卡同款）
- 下载页五分区：游戏 / 模组 / 资源包 / 光影 / 整合包；内容生态走 Modrinth API
  （搜索 / 加载器与游戏版本筛选 / 排序 / 项目版本列表 / 一键安装含 REQUIRED
  依赖闭包自动解析，文件已存在跳过 —— FCL ModDependenciesResolver / ZL2
  QuickDownloadPlanner 同款语义）
- 整合包双通道导入：应用内文件选择（raw IPC 字节流）+ **系统文件关联**
  （文件管理器点开 .mrpack/.zip 自动导入，MainActivity ACTION_VIEW 接管，
  ZL2 ImportModpackActivity / FCL ImportActivity 同款自动化）
- 实例管理增强：重命名 / 复制（目录递归复制）/ 删除 / 选用 + 按实例覆盖
  内存与渲染层（FCL VersionSettingPage 同款）
- Mod 启用/禁用：`.disabled` 改名语义（FCL/ZL2 同款，不删文件）
- 启动前自动化链：preflight → Java 运行时按需自动下载 → **资产缺失自动补全**
  （ZL2 VERIFY_AND_REPAIR / FCL checkGameCompletion 同款）→ 自动续启动
- Java 运行时管理分区：Java 8/17/21/25 状态一览 + 手动预下载
  （ZL2 JavaManage / FCL JavaManageDialog 同款）
- 微软设备码自动复制（ZL2 同款）；authlib-injector 外置登录保留
- 日志上传 mclo.gs 生成分享链接（ZL2 LogsUpload / FCL logshare 同款）
- BMCLAPI 镜像自适应下载（quartz-core mirror 层原有能力）

**已删除（对标无此功能）**：设置页 Azure client_id 暴露卡（ZL2/FCL 内置凭据；
本项目改为账户页首次使用微软登录时按需一次性配置）

**后续差距（记录）**：CurseForge 双源、皮肤/衣柜、Terracotta 联机、控件编辑器、
整合包导出、多游戏目录 —— 需后端新能力，按 roadmap 逐项补齐。

## 快速开始

```bash
# 1) 跑测试
cargo test

# 2) 生成一份 1.20.4 启动计划（渲染路由 + JVM/游戏参数，全部离线）
cargo run -p quartz-cli -- plan 1.20.4 --player Steve --memory-mb 2048

# 3) 渲染路由演示（把调研结论里的矩阵跑给你看）
cargo run -p quartz-cli -- renderers --mc 1.20.4 --os android --vulkan
cargo run -p quartz-cli -- renderers --mc 1.20.4 --os windows      # → dozen (WoA)
cargo run -p quartz-cli -- renderers --mc 1.16.5 --os android      # → gl4es

# 4) 实例管理（./.quartz-root 下）
cargo run -p quartz-cli -- instances add --id dev --name "开发整合" --mc 1.20.4
cargo run -p quartz-cli -- instances list

# 5) 离线 UUID（与 Java nameUUIDFromBytes("OfflinePlayer:<name>") 一致）
cargo run -p quartz-cli -- uuid Steve

# 6) ui-pack 热更协议演示：manifest 校验 + 灰度判定 + staging 任务计划（内置示例，离线）
cargo run -p quartz-cli -- uipack-check --install-id install-0001

# 7) 桌面壳（需要各平台系统依赖，见 apps/shell-tauri/README.md）
cd apps/shell-tauri/ui && npm install && npm run dev   # 纯 UI 预览
cargo install tauri-cli --version "^2"
cd ../src-tauri && cargo tauri dev                     # 桌面调试窗口
```

## 仓库结构

```
quartz-launcher/
├── crates/quartz-core/        # 核心（模块地图见 src/lib.rs 文档）
│   └── fixtures/              # 合成样例数据（结构真实、哈希占位）
├── crates/quartz-cli/         # qz 命令行
├── crates/quartz-ffi/         # C-ABI 边界
├── apps/shell-tauri/          # Tauri 2 壳 + Web UI + ui-pack 契约
├── docs/                      # ARCHITECTURE / ROADMAP / UI-PACK
├── LICENSE                    # GPL-3.0-or-later
└── .github/workflows/ci.yml   # fmt + clippy + test + UI typecheck
```

## 下一步（按 ROADMAP）

1. **POC-A**：Tauri 2 Android 壳 + core 在线下载链路跑通
2. **POC-B**：鸿蒙 ArkWeb 薄壳加载同一份 ui-pack（方案 A 最后一块不确定拼图）
3. **WoA**：OpenJDK windows-aarch64 自构建验证

## 许可

GPL-3.0-or-later（当前选择：若复用 Pojav 系核心代码则传染 GPL；
若仅复用其构建产物可再评估，见 docs/ARCHITECTURE.md 第 5 节）。
