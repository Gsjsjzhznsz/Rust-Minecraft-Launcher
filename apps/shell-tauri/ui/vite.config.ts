import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
  },
  // Pages 演示构建用：VITE_BASE=/Rust-Minecraft-Launcher/demo/ VITE_DEMO=1 npm run build:demo
  base: process.env.VITE_BASE || "/",
  build: {
    target: "es2021",
    outDir: process.env.VITE_OUT_DIR || "dist",
  },
});
