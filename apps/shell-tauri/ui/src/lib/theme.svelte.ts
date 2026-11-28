/* 设计语言注册表与实时切换 —— Svelte 5 runes 版
   参照 Style Atlas 方法论：风格是 Token + 组件约束的集合体，
   切换 = 换 html[data-theme] 属性，选择即时应用、localStorage 持久化。

   主题体系（21 种，全部取材 https://weepwood.github.io/style-atlas/ ）：
   - 配色 / 材质常量 / 圆角 / 动效节奏一律以 Atlas 每款风格的
     palette + tokens 实测值为蓝本（提取脚本 scripts/extract_atlas_styles.js）。
   - 主打 = 玻璃拟态（Atlas 01 号卡），首次使用默认应用。
   - 五组划分对应 Atlas 的选型维度（氛围 / 产品 / 极简 / 趣味 / 深色）。 */

export type ThemeId =
  | "glass"
  | "liquid"
  | "aurora"
  | "bento"
  | "m3"
  | "fluent"
  | "command"
  | "swiss"
  | "editorial"
  | "luxury"
  | "wabi"
  | "ink"
  | "clay"
  | "neu"
  | "brutal"
  | "y2k"
  | "terminal"
  | "cyber"
  | "vaporwave"
  | "oled"
  | "pixel";

export interface ThemeMeta {
  id: ThemeId;
  name: string;
  en: string;
  /** 分组标题（对应 Atlas 选型维度） */
  group: string;
  tagline: string;
  tags: string[];
  dark: boolean;
  /** Android 状态栏 theme-color */
  metaColor: string;
  /** 快捷切换点的双面色 */
  dot: [string, string];
}

export const THEMES: ThemeMeta[] = [
  /* ============ 玻璃与光（主打） ============ */
  {
    id: "glass",
    name: "玻璃拟态",
    en: "Glassmorphism",
    group: "玻璃与光",
    tagline: "半透明材质、背景模糊与柔和光斑构造空间层次——Atlas 主打款",
    tags: ["透明", "层次", "主打"],
    dark: false,
    metaColor: "#e5e1ff",
    dot: ["#e5e1ff", "#6b79ff"],
  },
  {
    id: "liquid",
    name: "液态光学玻璃",
    en: "Optical Liquid Glass",
    group: "玻璃与光",
    tagline: "半透明叠层、边缘高光与光学辉光构建深度",
    tags: ["折射", "高光"],
    dark: false,
    metaColor: "#cfefff",
    dot: ["#cfefff", "#86bbff"],
  },
  {
    id: "aurora",
    name: "极光界面",
    en: "Aurora UI",
    group: "玻璃与光",
    tagline: "模糊多色光带置于深色空间，流动而沉浸",
    tags: ["氛围", "深色"],
    dark: true,
    metaColor: "#0e1026",
    dot: ["#0e1026", "#8b8cff"],
  },

  /* ============ 产品语言 ============ */
  {
    id: "bento",
    name: "便当网格",
    en: "Bento Grid",
    group: "产品语言",
    tagline: "不同尺寸的卡片组织复杂信息，扫描效率与节奏兼得",
    tags: ["模块化", "高密度"],
    dark: false,
    metaColor: "#e9e9e5",
    dot: ["#e9e9e5", "#ff5f3d"],
  },
  {
    id: "m3",
    name: "Material You",
    en: "Material 3",
    group: "产品语言",
    tagline: "语义色面、大圆角与动态配色——可扩展的产品界面",
    tags: ["现代", "组件化"],
    dark: false,
    metaColor: "#f7f2fa",
    dot: ["#f7f2fa", "#6750a4"],
  },
  {
    id: "fluent",
    name: "Fluent 云母",
    en: "Fluent Mica",
    group: "产品语言",
    tagline: "桌面分层材质与自然导航——安静、克制、冷静",
    tags: ["桌面", "分层"],
    dark: false,
    metaColor: "#f7f9fc",
    dot: ["#dde4f0", "#2e64a5"],
  },
  {
    id: "command",
    name: "指挥仪表盘",
    en: "Command Center",
    group: "产品语言",
    tagline: "高密度指标、细分隔线与可扫描的数据视图",
    tags: ["数据", "仪表盘"],
    dark: true,
    metaColor: "#0c1724",
    dot: ["#0c1724", "#38d1be"],
  },

  /* ============ 极简理性 ============ */
  {
    id: "swiss",
    name: "瑞士国际主义",
    en: "Swiss Style",
    group: "极简理性",
    tagline: "网格、秩序与 hairline——用清晰建立信任",
    tags: ["理性", "无装饰"],
    dark: false,
    metaColor: "#f7f7f5",
    dot: ["#f7f7f5", "#ff2b21"],
  },
  {
    id: "editorial",
    name: "编辑排版",
    en: "Editorial",
    group: "极简理性",
    tagline: "版式、留白与图文节奏，像一份可阅读的数字刊物",
    tags: ["杂志", "衬线"],
    dark: false,
    metaColor: "#f2eee5",
    dot: ["#f2eee5", "#d55733"],
  },
  {
    id: "luxury",
    name: "奢华极简",
    en: "Luxury Minimalism",
    group: "极简理性",
    tagline: "高质量字体、极少配色与精确留白的稀缺感",
    tags: ["高级", "克制"],
    dark: true,
    metaColor: "#181713",
    dot: ["#181713", "#e6dbc3"],
  },
  {
    id: "wabi",
    name: "日式侘寂",
    en: "Wabi-sabi",
    group: "极简理性",
    tagline: "米纸色、墨色与非对称留白，克制与时间感",
    tags: ["留白", "侘寂"],
    dark: false,
    metaColor: "#eee9df",
    dot: ["#eee9df", "#8b3a2f"],
  },
  {
    id: "ink",
    name: "水墨留白",
    en: "Ink Wash",
    group: "极简理性",
    tagline: "笔触墨阶、宣纸留白与含蓄的非对称编排",
    tags: ["东方", "水墨"],
    dark: false,
    metaColor: "#ece8de",
    dot: ["#ece8de", "#b95e4e"],
  },

  /* ============ 质感趣味 ============ */
  {
    id: "clay",
    name: "黏土拟态",
    en: "Claymorphism",
    group: "质感趣味",
    tagline: "圆润体块、内外阴影与低攻击性配色的玩具感",
    tags: ["柔软", "3D"],
    dark: false,
    metaColor: "#d8ecff",
    dot: ["#d8ecff", "#ff9a92"],
  },
  {
    id: "neu",
    name: "新拟物柔光",
    en: "Neumorphism",
    group: "质感趣味",
    tagline: "同色系高光与阴影塑造轻微浮雕，控件像从背景生长",
    tags: ["柔光", "浮雕"],
    dark: false,
    metaColor: "#e7ecf2",
    dot: ["#e7ecf2", "#5576ff"],
  },
  {
    id: "brutal",
    name: "新粗野主义",
    en: "Neo Brutalism",
    group: "质感趣味",
    tagline: "粗边框、硬阴影与大胆排版，直接制造品牌记忆",
    tags: ["叛逆", "硬阴影"],
    dark: false,
    metaColor: "#fffdf3",
    dot: ["#ffdd00", "#101010"],
  },
  {
    id: "y2k",
    name: "Y2K 复古未来",
    en: "Y2K Revival",
    group: "质感趣味",
    tagline: "金属光泽、糖果渐变与早期互联网符号",
    tags: ["千禧", "糖果"],
    dark: false,
    metaColor: "#d9f6ff",
    dot: ["#f6d7ff", "#8d78ff"],
  },

  /* ============ 极客霓虹 ============ */
  {
    id: "terminal",
    name: "开发者终端",
    en: "Terminal UI",
    group: "极客霓虹",
    tagline: "等宽、提示符与日志结构——直接、可信、零装饰",
    tags: ["极客", "等宽"],
    dark: true,
    metaColor: "#0b0e0a",
    dot: ["#0b0e0a", "#b7ff76"],
  },
  {
    id: "cyber",
    name: "赛博终端",
    en: "Cyberpunk",
    group: "极客霓虹",
    tagline: "深黑背景、荧光色与切角结构，高能量未来叙事",
    tags: ["霓虹", "未来"],
    dark: true,
    metaColor: "#070909",
    dot: ["#070909", "#d5ff38"],
  },
  {
    id: "vaporwave",
    name: "蒸汽波",
    en: "Vaporwave",
    group: "极客霓虹",
    tagline: "紫粉渐变、网格地平线与复古数字符号",
    tags: ["梦核", "怀旧"],
    dark: true,
    metaColor: "#160d2b",
    dot: ["#160d2b", "#f85ccf"],
  },
  {
    id: "oled",
    name: "OLED 深夜",
    en: "OLED Noir",
    group: "极客霓虹",
    tagline: "纯黑画布、精确亮度层级与高对比悬浮面板",
    tags: ["纯黑", "省电"],
    dark: true,
    metaColor: "#050505",
    dot: ["#050505", "#88f4c2"],
  },
  {
    id: "pixel",
    name: "像素复古",
    en: "Pixel Retro",
    group: "极客霓虹",
    tagline: "有限色板与块状浮雕——把启动器做成一台游戏机",
    tags: ["8-bit", "MC 气质"],
    dark: true,
    metaColor: "#1b1f3a",
    dot: ["#1b1f3a", "#7ce38b"],
  },
];

/** 主打设计语言：玻璃拟态（Atlas 01 号卡） */
const DEFAULT_THEME: ThemeId = "glass";
const THEME_KEY = "quartz.theme";
const MOTION_KEY = "quartz.motion";

export const themeState = $state<{ id: ThemeId; motion: boolean }>({
  id: DEFAULT_THEME,
  motion: true,
});

function isThemeId(v: string | null): v is ThemeId {
  return v !== null && THEMES.some((t) => t.id === v);
}

function readStored(): { id: ThemeId; motion: boolean } {
  let id: ThemeId = DEFAULT_THEME;
  let motion = true;
  try {
    const sid = window.localStorage.getItem(THEME_KEY);
    if (isThemeId(sid)) id = sid;
    const sm = window.localStorage.getItem(MOTION_KEY);
    if (sm === "0") motion = false;
    // 首次使用尊重系统级减少动态偏好
    if (sm === null && window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
      motion = false;
    }
  } catch {
    /* 隐私模式等场景下静默降级 */
  }
  return { id, motion };
}

function applyDom() {
  const root = document.documentElement;
  root.dataset.theme = themeState.id;
  root.dataset.motion = themeState.motion ? "on" : "off";
  const meta = document.querySelector('meta[name="theme-color"]');
  if (meta) {
    const t = THEMES.find((x) => x.id === themeState.id);
    if (t) meta.setAttribute("content", t.metaColor);
  }
}

/** 在 mount 前同步调用：恢复持久化选择并落到 DOM */
export function initTheme() {
  const s = readStored();
  themeState.id = s.id;
  themeState.motion = s.motion;
  applyDom();
}

/** 实时切换设计语言（点击即应用，无需刷新） */
export function setTheme(id: ThemeId) {
  if (!isThemeId(id)) return;
  themeState.id = id;
  try {
    window.localStorage.setItem(THEME_KEY, id);
  } catch { /* ignore */ }
  applyDom();
}

/** 动效总开关（减少即时反馈；prefers-reduced-motion 始终独立生效） */
export function toggleMotion() {
  themeState.motion = !themeState.motion;
  try {
    window.localStorage.setItem(MOTION_KEY, themeState.motion ? "1" : "0");
  } catch { /* ignore */ }
  applyDom();
}
