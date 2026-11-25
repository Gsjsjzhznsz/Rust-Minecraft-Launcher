import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";
import md3Theme from "starlight-theme-md3";

export default defineConfig({
  // GitHub Pages 项目站点：https://gsjsjzhznsz.github.io/Rust-Minecraft-Launcher/
  // UI 在线演示部署在 <base>/demo/（由 .github/workflows/pages.yml 每日自动重建）
  site: "https://gsjsjzhznsz.github.io",
  base: "/Rust-Minecraft-Launcher",
  output: "static",
  integrations: [
    starlight({
      title: "Quartz Launcher",
      description:
        "业界第一个 Rust 核心 + 五端同构的 Minecraft: Java Edition 启动器。Rust 大脑 · 系统 WebView 壳 · ui-pack 实时热更。",
      locales: {
        root: { label: "简体中文", lang: "zh-CN" },
      },
      head: [
        {
          // starlight-theme-md3 引用了旧版 Starlight 的全局 StarlightThemeProvider
          // （0.43 已移除）。这里提前注入空实现兜底，避免 ReferenceError；
          // 主题切换器自身的同步逻辑由主题的 custom element 完成，不受影响。
          tag: "script",
          attrs: {},
          content:
            "window.StarlightThemeProvider = window.StarlightThemeProvider ?? { updatePickers() {}, fetchPreferredTheme() {} };",
        },
      ],
      logo: { src: "./src/assets/icon.svg" },
      social: [
        {
          icon: "github",
          label: "GitHub",
          href: "https://github.com/Gsjsjzhznsz/Rust-Minecraft-Launcher",
        },
      ],
      // Material Design 3 主题：种子色取自 Quartz 品牌绿 #3ddc97
      plugins: [
        md3Theme({
          seed: "#3ddc97",
          variant: "tonalSpot",
          density: "compact",
          shape: "medium",
          contrast: "standard",
          tonalSurface: true,
          motion: true,
        }),
      ],
      sidebar: [
        {
          label: "指南",
          items: [
            { label: "安装", link: "/guides/install/" },
            { label: "快速上手", link: "/guides/quickstart/" },
            { label: "渲染层选择", link: "/guides/renderers/" },
            { label: "实例与整合包", link: "/guides/instances/" },
            { label: "账号与登录", link: "/guides/account/" },
          ],
        },
        {
          label: "参考",
          items: [
            { label: "架构设计", link: "/reference/architecture/" },
            { label: "UI 实时更新（ui-pack）", link: "/reference/uipack/" },
            { label: "网页演示版", link: "/reference/demo/" },
            { label: "路线图", link: "/reference/roadmap/" },
            { label: "常见问题", link: "/reference/faq/" },
          ],
        },
      ],
    }),
  ],
});
