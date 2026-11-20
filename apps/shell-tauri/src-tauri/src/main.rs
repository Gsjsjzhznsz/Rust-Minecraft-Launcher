// Quartz 壳层 —— 桌面入口（macOS ARM / Windows ARM / Linux）。
// 移动端（Android/iOS）由 tauri mobile 模板生成对应入口后复用本 lib。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    quartz_shell_lib::run();
}
