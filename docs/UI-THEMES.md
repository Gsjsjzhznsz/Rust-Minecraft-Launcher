# UI 设计语言系统（多主题实时切换）

> 参考方法论：[Style Atlas](https://weepwood.github.io/style-atlas/) ——
> 「把视觉风格提炼为 Token、组件和约束」。风格是解决问题的工具，不是页面表面的装饰。

## 1. 架构

```
ui/src/
├── styles/
│   ├── base.css      # 语义 Token 契约 + 全部组件类（只消费语义变量，零具体配色/字体/时长）
│   └── themes.css    # N 种设计语言：每主题 = Token 覆盖块 + 少量结构性识别规则
└── lib/
    └── theme.svelte.ts  # 主题注册表 / 实时切换 / localStorage 持久化 / 动效总开关
```

- **切换机制**：`html[data-theme="…"]` 属性切换，Token 经 CSS 自定义属性级联到全树，
  零重排零刷新即时生效；预览小窗用 scoped `data-theme` 在子树内复用同一套规则。
- **持久化**：`quartz.theme` / `quartz.motion` 写 localStorage，`main.ts` 在 mount 前
  同步恢复（避免首帧闪默认主题）；首次使用尊重系统 `prefers-reduced-motion`。
- **状态栏联动**：切换时同步 `<meta name="theme-color">`（Android 状态栏随主题变色）。

## 2. Token 契约（每种语言必须完整定义）

| 分组 | 变量 |
| --- | --- |
| 表面 | `--bg-app / --bg-panel / --bg-panel-2 / --bg-inset / --bg-nav / --bg-hover / --bg-active` |
| 边框 | `--border-w / --border / --border-strong / --border-dim` |
| 文本 | `--text / --text-dim / --text-faint / --text-on-accent` |
| 主色 | `--accent / --accent-hover / --accent-dim / --accent-border / --accent-2 / --accent-2-dim` |
| 状态 | `--ok(--dim) / --warn(--dim) / --err(--dim) / --info(--dim) / --purple(--dim)` |
| 形状 | `--r-lg / --r-md / --r-sm / --r-pill` |
| 阴影 | `--shadow-card / --shadow-pop / --shadow-accent` |
| 字体 | `--font-body / --font-display / --font-mono / --fs-body / --weight-display / --ls-display / --tt-display / --tt-label / --ls-label` |
| 姿态 | `--btn-hover-t / --btn-active-t / --nav-indicator` |
| 动效 | `--motion-fast / --motion-nav / --motion-surface / --motion-status / --ease-out / --ease-standard / --ease-emphasized` |
| 控制台 | `--console-bg / --console-text / --console-err / --console-sys / --console-dim` |
| 其他 | `--selection-bg / --sidebar-w / --card-pad` |

主题块必须**完整覆盖**上表 —— 未定义的变量会从 `html` 当前主题泄漏（预览子树尤甚）。

## 3. 六种设计语言

| id | 名称 | 出处与特征 | 明暗 |
| --- | --- | --- | --- |
| `swiss` | 瑞士国际主义 | 网格底纹、hairline、无投影、Helvetica 系、大写字距标签、红色主色 | 浅 |
| `m3` | Material You | 语义色面、pill 控件、顶部圆角输入框、M3 emphasized 缓动、绿色种子 | 浅 |
| `fluent` | Fluent 云母 | 石墨分层材质（静态晕染）、轻边框+顶部高光、小圆角、Segoe UI 系 | 深 |
| `terminal` | 开发者终端 | 全等宽、CRT 扫描线、`$`/`//`/`>` 提示符结构、反色按钮、即时动效 | 深 |
| `brutal` | 新粗野主义 | 2px 粗边框、实心偏移阴影随按压增减、荧光衬底标题、黑底激活导航 | 浅 |
| `pixel` | 像素复古 | 8px 方格底纹、块状浮雕（按压反转）、有限色板、MC 气质、steps 缓动 | 深 |

## 4. 动效四档（对齐 motion-lab）

| 档 | 变量 | 用途 | 典型时长 |
| --- | --- | --- | --- |
| fast | `--motion-fast` | 按钮、收藏、开关 | 90–150ms |
| nav | `--motion-nav` | 导航、选择、选项卡 | 120–220ms |
| surface | `--motion-surface` | 容器展开、弹窗、页面入场 | 160–300ms |
| status | `--motion-status` | 提示、通知、进度条宽度 | 100–200ms |

红线：只动 `transform` / `opacity`；不动画 blur；装饰性循环动画（呼吸光晕、流光扫过、
渐变流动）已全部移除；`prefers-reduced-motion` 与设置页「动效总开关」（`data-motion="off"`）
相互独立、任一生效即全关。

## 5. 新增一种设计语言

1. 在 `themes.css` 追加 `[data-theme="my-id"]` 块：完整 Token 覆盖 + 少量结构性规则
   （参考 brutalism 的按压阴影、terminal 的提示符前缀——结构性规则只写「Token 表达不了的身份细节」）；
2. 在 `theme.svelte.ts` 的 `THEMES` 注册表追加元数据（名称 / 主张 / 标签 / 明暗 / `metaColor` / 色点双色）；
3. 完成。图鉴卡片、快捷色点、持久化、动效档位自动接入，无需改任何组件。

## 6. 入口

- 设置页「外观 · 设计语言」：图鉴卡片（悬停只高亮、点击才应用）+ 动效实验室；
- 侧栏 / 顶栏「设计语言」色点：任意页面即时快切。
