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
- **配色蓝本**：21 种语言的 palette / 材质常量 / 圆角 / 阴影一律取自 Style Atlas
  各款风格的 `palette` + `tokens` 实测值（`scripts/extract_atlas_styles.js` 从站点
  bundle 提取，产出 `/tmp/atlas/styles.json`）；不臆造颜色。

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
| 材质 | `--panel-blur`（静态 backdrop-filter；玻璃族专用，非玻璃语言必须显式置 none/省略） |
| 控制台 | `--console-bg / --console-text / --console-err / --console-sys / --console-dim` |
| 其他 | `--selection-bg / --sidebar-w / --card-pad` |

主题块必须**完整覆盖**上表 —— 未定义的变量会从 `html` 当前主题泄漏（预览子树尤甚）。

## 3. 21 种设计语言（五组，配色取自 Atlas 实测值）

主打 = **`glass` 玻璃拟态**（Atlas 01 号卡，默认 Token 契约在 base.css `:root`）。

| 组 | id | 名称 | 出处与特征 | 明暗 |
| --- | --- | --- | --- | --- |
| 玻璃与光 | `glass` | 玻璃拟态 | 薰衣草底 + 珊瑚/长春花紫/薄荷光斑、blur 18px、r24、紫罗兰大柔影 | 浅 |
| 玻璃与光 | `liquid` | 液态光学玻璃 | 高光描边、blur 20px、r30、glow #86BBFF | 浅 |
| 玻璃与光 | `aurora` | 极光界面 | #0E1026 深底 + #5658FF/#E45CFF 光带、半透明面板 | 深 |
| 产品语言 | `bento` | 便当网格 | 白卡 r20、强调面板整体翻 #191919、accent #FF5F3D | 浅 |
| 产品语言 | `m3` | Material You | M3 紫种子 #6750A4、pill 控件、emphasized 缓动 | 浅 |
| 产品语言 | `fluent` | Fluent 云母 | 云母分层（sidebar #DDE4F0）、accent #2E64A5、r12 | 浅 |
| 产品语言 | `command` | 指挥仪表盘 | #0C1724/#152538、细线 #2C4257、accent #38D1BE、tabular-nums | 深 |
| 极简理性 | `swiss` | 瑞士国际主义 | 网格底纹、hairline、无投影、accent #FF2B21 | 浅 |
| 极简理性 | `editorial` | 编辑排版 | paper #F2EEE5、衬线体、1px 规则线、accent #D55733 | 浅 |
| 极简理性 | `luxury` | 奢华极简 | #181713 + 象牙/金、衬线大字距、无圆角 | 深 |
| 极简理性 | `wabi` | 日式侘寂 | 米纸 #EEE9DF、墨色、朱印 #8B3A2F、非对称短划线 | 浅 |
| 极简理性 | `ink` | 水墨留白 | 宣纸 #ECE8DE、墨 #222C28、朱砂竖笔 #B95E4E | 浅 |
| 质感趣味 | `clay` | 黏土拟态 | 内亮外深双色体块、r28、coral #FF9A92、回弹缓动 | 浅 |
| 质感趣味 | `neu` | 新拟物柔光 | 同色系双影（凸/凹）、accent #5576FF、r24 | 浅 |
| 质感趣味 | `brutal` | 新粗野主义 | 2px 粗边框、硬阴影随按压增减、#FFDD00/#101010、黑底导航 | 浅 |
| 质感趣味 | `y2k` | Y2K 复古未来 | 糖果渐变背景、铬渐变按钮、双层硬影 #FF8BD7/#6CE7FF | 浅 |
| 极客霓虹 | `terminal` | 开发者终端 | 全等宽、CRT 扫描线、`$`/`//`/`>` 提示符、#B7FF76/#36462E | 深 |
| 极客霓虹 | `cyber` | 赛博终端 | 荧光 #D5FF38 + glitch #FF2FAC、切角按钮、霓虹下划线 | 深 |
| 极客霓虹 | `vaporwave` | 蒸汽波 | 紫粉渐变字、网格地平线、粉色辉光、blur 面板 | 深 |
| 极客霓虹 | `oled` | OLED 深夜 | 纯黑画布、亮度分层 #131313/#1d1d1d 描边、accent #88F4C2 | 深 |
| 极客霓虹 | `pixel` | 像素复古 | 深蓝格纹 #1B1F3A、块状浮雕、有限色板、steps 缓动 | 深 |

## 4. 动效四档（对齐 motion-lab）

| 档 | 变量 | 用途 | 典型时长 |
| --- | --- | --- | --- |
| fast | `--motion-fast` | 按钮、收藏、开关 | 120–160ms |
| nav | `--motion-nav` | 导航、选择、选项卡 | 180–240ms |
| surface | `--motion-surface` | 容器展开、弹窗、页面入场 | 240–320ms |
| status | `--motion-status` | 提示、通知、进度条宽度 | 160–220ms |

红线：只动 `transform` / `opacity`；不动画 blur；装饰性循环动画（呼吸光晕、流光扫过、
渐变流动）已全部移除；`prefers-reduced-motion` 与设置页「动效总开关」（`data-motion="off"`）
相互独立、任一生效即全关。

## 5. 新增一种设计语言

1. 优先用 `scripts/extract_atlas_styles.js` 从 Atlas 提取该风格的 palette/tokens 实测值；
2. 在 `themes.css` 追加 `[data-theme="my-id"]` 块：完整 Token 覆盖 + 少量结构性规则
   （参考 brutalism 的按压阴影、terminal 的提示符前缀——结构性规则只写「Token 表达不了的身份细节」）；
3. 在 `theme.svelte.ts` 的 `THEMES` 注册表追加元数据（组别 / 名称 / 主张 / 标签 / 明暗 /
   `metaColor` / 色点双色）；
4. 完成。图鉴分组卡片、快捷色点、持久化、动效档位自动接入，无需改任何组件。

## 6. 入口

- 设置页「外观 · 设计语言」：图鉴卡片（悬停只高亮、点击才应用）+ 动效实验室；
- 侧栏 / 顶栏「设计语言」色点：任意页面即时快切。
