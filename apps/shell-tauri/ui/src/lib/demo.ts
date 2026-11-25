/* 演示模式标记：仅当以 VITE_DEMO=1 构建时为 true（Vite 静态替换，
   APK 生产构建中恒为 false，相关 UI 分支整体被 tree-shake）。 */
export const IS_DEMO = import.meta.env.VITE_DEMO === "1";
