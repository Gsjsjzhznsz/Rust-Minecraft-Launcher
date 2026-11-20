# Quartz 壳层（Tauri 2 / 方案 A）

UI 为系统 WebView 渲染的 Web 资源，后端直调 `quartz-core`（Tauri command 即核心 API 的薄封装）。
本目录是独立 package（不参与根 workspace 的默认 CI），因为 Tauri 构建依赖各平台系统库。

## 目录

```
shell-tauri/
├── src-tauri/           # Rust 壳：command 注册、窗口配置、capabilities
│   ├── src/lib.rs       # app_meta / versions_offline / renderer_advice / plan_offline
│   └── tauri.conf.json  # 窗口 + 前端构建对接
└── ui/                  # Web UI（vanilla TS + Vite，零框架依赖）
    └── src/main.ts      # invoke() 调用核心；纯浏览器预览时走 stub.json
```

## 本地运行（桌面）

前置：Rust stable、Node 18+、各平台系统依赖
（Linux: `libwebkit2gtk-4.1-dev libgtk-3-dev`；macOS: Xcode CLT；Windows: WebView2）。

```bash
cargo install tauri-cli --version "^2"
cd apps/shell-tauri/ui && npm install
cd ../src-tauri && cargo tauri dev        # 桌面调试窗口
cargo tauri build                         # 桌面打包（bundle.active 当前为 false）
```

只预览 UI（无 Tauri 环境）：

```bash
cd apps/shell-tauri/ui && npm install && npm run dev   # 打开 http://localhost:5173
# 纯浏览器模式下 main.ts 自动降级为 stub.json 桩数据
```

## 移动端（M1 里程碑启用）

```bash
cargo tauri android init    # 生成 gen/android（需 Android Studio + NDK）
cargo tauri ios init        # 生成 gen/ios（需 macOS + Xcode）
```

## 命令契约（UI 只允许调用这些）

| command | 说明 | 热更可改 |
| --- | --- | --- |
| `app_meta` | 名称/版本/OS/架构 | 否 |
| `versions_offline` | 内嵌清单样例；在线版走 `meta::fetch_version_manifest` | 是 |
| `renderer_advice(mc, caps?)` | renderbus 选择结果与原因 | 是 |
| `plan_offline(player, memoryMb)` | 启动计划 JSON（演示） | 是 |

规则：新增能力 = 先在 quartz-core 实现 → 壳层 command 暴露 → UI 调用；
UI 逻辑永远允许通过 ui-pack 热更（docs/UI-PACK.md），command 契约保持向后兼容。
