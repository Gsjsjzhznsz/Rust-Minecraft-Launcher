---
title: UI 实时更新（ui-pack）
description: 改界面不发版 —— 版本化、签名校验、灰度、秒级回滚的 Web 资源包协议
---

Quartz 的 UI（Svelte 5 + Web 资源）不编译进壳层二进制，而是作为一份
**版本化、可签名校验、可灰度、可秒级回滚**的 Web 资源包分发。壳层只负责按
manifest 拉取、校验、原子切换 —— **改界面不发版**。

## 包结构

```text
ui-pack/
├── manifest.json        # 清单（唯一入口，写死在壳内的可信 URL 起步）
├── pack/                # 实际 UI 资源（构建产物，不含源码）
│   ├── index.html
│   ├── assets/*.js
│   ├── assets/*.css
│   └── i18n/*.json
└── pack.sha256          # pack 目录 tar 的整体哈希（可选，二次校验）
```

## manifest 关键字段

| 字段 | 作用 |
| --- | --- |
| `schema` | 破坏性变更递增；壳层拒绝高于自身支持的 schema |
| `minShellVersion` | UI 可声明依赖的壳层 command 最低版本，防止调不存在的 command |
| `rollout` | 灰度：壳层按 `hash(install_id + salt) % 100 < percent` 判定命中 |
| `previous` | 回滚指针：切换失败或用户报障时一键回退上一版本 |
| `signature` | ed25519 签名，公钥内嵌壳层二进制，校验失败即拒绝 |

## 更新流程

```text
启动 → 读本地 current/manifest.json（或首次用内嵌 pack）
     → 并行：加载 current UI（立即可用）+ 后台拉取线上 manifest
     → 校验签名 / schema / minShellVersion / 灰度命中
     → 增量下载 files（sha256 校验，复用 core::download 断点逻辑）
     → 原子切换：staging/ 完整后 rename → current/
     → 下一次启动生效（或 UI 内「立即应用」软重启 WebView）
失败路径：任一步失败 → 保留 current，事件上报，静默重试（下次启动）
回滚路径：current 启动崩溃 > N 次 → 自动切 previous 指针
```

## 合规边界

iOS 上架包内嵌完整可用 UI，热更只改界面与内容，不触碰 command 契约
（Apple 3.3.2 红线内）。本地可用 `qz uipack-check` 离线演练整套
manifest 校验 + 灰度判定 + staging 任务计划。
