# ui-pack 协议 v0（方案 A：UI 实时更新）

> 目标：改界面不发版。UI 是一份版本化、可签名校验、可灰度、可秒级回滚的
> Web 资源包；壳层只负责按 manifest 拉取、校验、原子切换。

## 1. 包结构

```
ui-pack/
├── manifest.json        # 清单（唯一入口，写死在壳内的可信 URL 起步）
├── pack/                # 实际 UI 资源（构建产物，不含源码）
│   ├── index.html
│   ├── assets/*.js
│   ├── assets/*.css
│   └── i18n/*.json
└── pack.sha256          # pack 目录 tar 的整体哈希（可选，二次校验）
```

## 2. manifest.json

```json
{
  "schema": 1,
  "channel": "stable",
  "version": "2026.11.21-r2",
  "createdAt": "2026-11-21T08:00:00Z",
  "minShellVersion": "0.1.0",
  "rollout": { "percent": 100, "salt": "r2" },
  "previous": { "version": "2026.11.14-r5", "manifest": "https://cdn.example/ui/2026.11.14-r5/manifest.json" },
  "files": [
    {
      "path": "pack/index.html",
      "sha256": "b6f1…",
      "size": 4210,
      "url": "https://cdn.example/ui/2026.11.21-r2/pack/index.html"
    }
  ],
  "signature": "ed25519:…(对 files+version+channel 规范化 JSON 的签名)"
}
```

字段约束：
- `schema`：破坏性变更递增；壳层拒绝高于自身支持的 schema。
- `minShellVersion`：UI 可声明依赖的壳层 command 最低版本，防止调不存在的 command。
- `rollout`：灰度；壳层按 `hash(install_id + salt) % 100 < percent` 判定是否命中。
- `previous`：回滚指针；切换失败或用户报障时，壳层可一键回退到上一版本。
- `signature`：ed25519 公钥内嵌于壳层二进制（随发版更新），manifest 校验失败即拒绝。

## 3. 更新流程

```
启动 → 读本地 current/manifest.json（或首次用内嵌 pack）
     → 并行：加载 current UI（立即可用）+ 后台拉取线上 manifest
     → 校验签名 / schema / minShellVersion / 灰度命中
     → 增量下载 files（sha256 校验，复用 core::download 的断点逻辑）
     → 原子切换：staging/ 目录完整后 rename → current/
     → 下一次启动生效（或 UI 内「立即应用」软重启 WebView）
失败路径：任一步失败 → 保留 current，事件上报，静默重试（下次启动）
回滚路径：current 启动崩溃 > N 次 → 自动切 previous 指针
```

## 4. 壳层职责边界（合规红线）

- 上架包（iOS App Store / 鸿蒙应用市场）必须内嵌**完整可用**的 ui-pack；
  热更只允许改界面与内容，不得改变应用实质（Apple 3.3.2）。
- 侧载渠道（Android APK / iOS TrollStore/AltStore / 鸿蒙 unsigned HAP）可放宽，
  但同一套 manifest 协议保持一致，便于审计。
- command 契约（壳层暴露给 UI 的 API）只增不改；删除需 schema 递增 + 过渡期。
- 签名私钥与发布系统隔离；CI 只产出带公钥验签的包。

## 5. 与 core::download 的复用

协议核心侧已落地为 `quartz_core::uipack`（有单测覆盖）：

- `Manifest` 模型与 `parse()`：schema 递增保护（壳层拒绝高于 `SUPPORTED_SCHEMA` 的版本）、
  `minShellVersion` semver 比较、文件路径安全白名单（禁绝对/穿越/反斜杠）、sha256 格式校验；
- `rollout_hit()`：`FNV-1a(install_id + salt) % 100 < percent` 的确定性灰度判定；
- `download_tasks()`：把 files 展开为 `core::download` 任务（`HashSpec::Sha256` 校验 +
  断点跳过 + 并发窗口）；
- `rollback_target()`：previous 回滚指针，供壳层实现崩溃 N 次自动切回。

壳层剩余职责：ed25519 验签（公钥内嵌二进制）、staging 原子 rename 切换、崩溃计数与回滚执行。
