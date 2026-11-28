import { mount } from "svelte";
import "./styles/base.css";
import "./styles/themes.css";
import { initTheme } from "./lib/theme.svelte";
import App from "./App.svelte";

// 在 mount 前同步恢复上次选择的设计语言与动效开关（避免首帧闪默认主题）
initTheme();

// 浏览器预览（dev / Pages 演示构建）：无 Tauri 时注入模拟后端。
// APK 生产构建两个开关皆为 false，本段整体 tree-shake，不会进包。
import { installMock } from "./lib/mock";
if (import.meta.env.DEV || import.meta.env.VITE_DEMO === "1") installMock();

const app = mount(App, { target: document.getElementById("app")! });

export default app;
