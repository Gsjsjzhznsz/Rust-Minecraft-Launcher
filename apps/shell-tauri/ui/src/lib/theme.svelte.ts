/* 设计语言注册表与实时切换 —— Svelte 5 runes 版
   参照 Style Atlas 方法论：风格是 Token + 组件约束的集合体，
   切换 = 换 html[data-theme] 属性，选择即时应用、localStorage 持久化。 */

export type ThemeId = "swiss" | "m3" | "fluent" | "terminal" | "brutal" | "pixel";

export interface ThemeMeta {
  id: ThemeId;
  name: string;
  en: string;
  tagline: string;
  tags: string[];
  dark: boolean;
  /** Android 状态栏 theme-color */
  metaColor: string;
  /** 快捷切换点的双面色 */
  dot: [string, string];
}

export const THEMES: ThemeMeta[] = [
  {
    id: "swiss",
    name: "瑞士国际主义",
    en: "Swiss Style",
    tagline: "网格、秩序与 hairline——用清晰建立信任",
    tags: ["理性", "高密度", "无装饰"],
    dark: false,
    metaColor: "#f4f4f2",
    dot: ["#f4f4f2", "#d5281b"],
  },
  {
    id: "m3",
    name: "Material You",
    en: "Material 3",
    tagline: "语义色面与大圆角——友好且可扩展的产品语言",
    tags: ["现代", "组件化", "动效饱满"],
    dark: false,
    metaColor: "#f8faef",
    dot: ["#f8faef", "#3b691f"],
  },
  {
    id: "fluent",
    name: "Fluent 云母",
    en: "Fluent Mica",
    tagline: "桌面分层材质与自然导航——安静、克制、冷静",
    tags: ["桌面", "分层", "深色"],
    dark: true,
    metaColor: "#20242a",
    dot: ["#20242a", "#6cc4f8"],
  },
  {
    id: "terminal",
    name: "开发者终端",
    en: "Terminal UI",
    tagline: "等宽与日志结构——直接、可信、零装饰",
    tags: ["极客", "等宽", "高对比"],
    dark: true,
    metaColor: "#0b0f0b",
    dot: ["#0b0f0b", "#43d675"],
  },
  {
    id: "brutal",
    name: "新粗野主义",
    en: "Neo Brutalism",
    tagline: "粗边框与硬阴影——直接制造强烈的品牌记忆",
    tags: ["叛逆", "强排版", "硬阴影"],
    dark: false,
    metaColor: "#f2ede0",
    dot: ["#f2ede0", "#ffd613"],
  },
  {
    id: "pixel",
    name: "像素复古",
    en: "Pixel Retro",
    tagline: "有限色板与块状浮雕——把启动器做成一台游戏机",
    tags: ["8-bit", "怀旧", "MC 气质"],
    dark: true,
    metaColor: "#15171a",
    dot: ["#15171a", "#5bbd4c"],
  },
];

const DEFAULT_THEME: ThemeId = "swiss";
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
