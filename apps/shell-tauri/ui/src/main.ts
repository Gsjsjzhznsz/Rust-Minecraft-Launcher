import { mount } from "svelte";
import "./app.css";
import App from "./App.svelte";

// 浏览器预览（dev / Pages 演示构建）：无 Tauri 时注入模拟后端。
// APK 生产构建两个开关皆为 false，本段整体 tree-shake，不会进包。
import { installMock } from "./lib/mock";
if (import.meta.env.DEV || import.meta.env.VITE_DEMO === "1") installMock();

const app = mount(App, { target: document.getElementById("app")! });

export default app;
