import { mount } from "svelte";
import "./app.css";
import App from "./App.svelte";

// 开发态浏览器预览：无 Tauri 时注入 mock（生产构建整段被 tree-shake 掉）
import { installMock } from "./lib/mock";
if (import.meta.env.DEV) installMock();

const app = mount(App, { target: document.getElementById("app")! });

export default app;
