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

## 当前状态（M0 基础已就位）

- `crates/quartz-core` —— 启动器大脑：元数据/规则引擎/下载编排/实例/JVM 装配/
  认证/整合包/渲染路由/输入协议/uipack/HAL，**37 项单测全绿，clippy 零告警**
- `crates/quartz-cli` —— 离线可跑的 CLI（`qz`）
- `crates/quartz-ffi` —— C-ABI 稳定边界样例（生产绑定走 UniFFI / NAPI-RS）
- `apps/shell-tauri` —— Tauri 2 壳 + Web UI（桌面可 dev；浏览器预览走桩数据）
- `docs/` —— 架构 / 路线图 / ui-pack 热更协议

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
