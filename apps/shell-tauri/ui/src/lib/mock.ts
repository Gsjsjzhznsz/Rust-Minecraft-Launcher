/* 开发态专用：无 Tauri 环境时（浏览器 vite dev）注入 __TAURI_INTERNALS__ mock，
   让整个 UI 可在浏览器中完整预览。生产构建中 import.meta.env.DEV === false，
   调用点被 dead-code 消除，本模块不会进入 APK。 */

type Cmd = (args: Record<string, unknown> | undefined) => unknown;

const now = Date.now();

const accounts = [
  { kind: "offline", username: "Steve", uuid: "5c1c1e9a4b0f9d2e8a7b6c5d4e3f1111", selected: true },
  { kind: "offline", username: "Alex", uuid: "a41d2f3b8c9e7d6a5b4c3d2e1f0a2222", selected: false },
];

function makeVersions(): { id: string; type: string; url: string }[] {
  const out: { id: string; type: string; url: string }[] = [];
  for (let i = 0; i < 6; i++) out.push({ id: `1.21.${4 - i}`, type: "release", url: "" });
  out.push({ id: "24w14a", type: "snapshot", url: "" });
  out.push({ id: "24w13b", type: "snapshot", url: "" });
  for (const v of ["1.20.4", "1.20.1", "1.19.4", "1.18.2", "1.16.5", "1.12.2", "1.8.9"]) {
    out.push({ id: v, type: "release", url: "" });
  }
  out.push({ id: "b1.7.3", type: "old_beta", url: "" });
  out.push({ id: "a1.2.6", type: "old_alpha", url: "" });
  return out;
}

const commands: Record<string, Cmd> = {
  get_account: () => accounts[0],
  list_accounts: () => accounts,
  offline_login: (a) => ({
    kind: "offline",
    username: String(a?.name ?? ""),
    uuid: "11112222333344445555666677778888",
  }),
  select_account: () => null,
  remove_account: () => null,
  list_versions: () => makeVersions(),
  list_installed_versions: () => ["1.20.4", "1.16.5"],
  list_instances: () => [
    {
      id: "survival-pack",
      name: "生存服整合",
      mcVersion: "1.20.4",
      loaderDesc: "Fabric 0.15.11",
      javaMajor: 17,
      memoryMb: 2048,
      renderer: "zink",
      createdAt: "2025-11-01",
      selected: true,
    },
    {
      id: "vanilla-112",
      name: "原版怀旧 1.12.2",
      mcVersion: "1.12.2",
      loaderDesc: "原版",
      javaMajor: 8,
      memoryMb: 1024,
      renderer: "zink",
      createdAt: "2025-11-10",
      selected: false,
    },
  ],
  list_content_cmd: (a) => {
    const kind = String(a?.kind ?? "mods");
    const name =
      {
        mods: "sodium.jar",
        resourcepacks: "faithful32.zip",
        shaderpacks: "complementary.zip",
        saves: "新世界",
        screenshots: "2025-11-24_12.00.png",
      }[kind] ?? "item.jar";
    return [{ name, kind, sizeBytes: 3_141_592, isDir: false, modifiedAt: Math.floor(now / 1000) - 86400 }];
  },
  delete_content_cmd: () => null,
  list_loader_versions: () => [
    { version: "0.15.11", stable: true },
    { version: "0.15.10", stable: true },
    { version: "0.16.0-beta.1", stable: false },
  ],
  create_instance_cmd: () => null,
  select_instance: () => null,
  delete_instance_cmd: () => null,
  get_env_info: () => ({
    gameDir: "/storage/emulated/0/Android/data/com.quartz.launcher/files/.minecraft",
    storageMode: "public",
    layoutOk: true,
    layoutError: null,
    fullAccess: true,
    freeBytes: 21_000_000_000,
  }),
  reinit_storage: () => ({
    gameDir: "/storage/emulated/0/.minecraft",
    storageMode: "public",
    layoutOk: true,
    layoutError: null,
    fullAccess: true,
    freeBytes: 21_000_000_000,
  }),
  app_meta: () => ({ name: "Quartz Launcher", version: "0.2.8", os: "android", arch: "arm64" }),
  get_diagnostics: () => ({ lastCrash: null, lastLaunchTail: null, freeBytes: 21_000_000_000 }),
  get_runtime_status: () => ({ installed: true, version: "17", path: "/jres/17", specAvailable: true }),
  list_renderers: () => [
    { id: "zink", name: "Zink (Vulkan)", note: "GL4.6→Vulkan，1.17+/Sodium/光影推荐档", eglLib: "", glLib: "", sizeHint: "约 45MB", specAvailable: true, installed: true, active: true },
    { id: "angle-vk", name: "ANGLE (Vulkan)", note: "GL ES→Vulkan，兼容档", eglLib: "", glLib: "", sizeHint: "约 30MB", specAvailable: true, installed: false, active: false },
    { id: "softgl", name: "软渲染", note: "无 GPU 兜底，性能低", eglLib: "", glLib: "", sizeHint: "约 12MB", specAvailable: false, installed: false, active: false },
  ],
  set_active_renderer: () => null,
  get_lwjgl_status: () => ({ installed: true, path: "/lwjgl" }),
  get_settings: () => ({
    extraJvmArgs: ["-XX:+UseG1GC"],
    azureClientIdSet: false,
    azureClientId: null,
    selectedInstance: "survival-pack",
  }),
  get_memory_mb: () => 2048,
  assets_status: () => ({ indexPresent: true, indexId: "5", total: 3932, missing: 0, missingBytes: 0 }),
  list_logs: () => [
    { name: "latest.log", size: 128_000, modifiedMs: now - 3600_000 },
    { name: "debug.log", size: 512_000, modifiedMs: now - 7200_000 },
  ],
  read_log_tail: () =>
    "[12:00:01] [main/INFO]: Setting user: Steve\n[12:00:02] [Render thread/INFO]: Backend library: LWJGL",
  authlib_status: () => ({ installed: false, version: null }),
  get_window_label: () => "main",
};

export function installMock() {
  const w = window as unknown as Record<string, unknown>;
  if (w.__TAURI_INTERNALS__) return;

  let cbSeq = 0;
  let listenerSeq = 0;

  w.__TAURI_INTERNALS__ = {
    invoke: async (cmd: string, args?: Record<string, unknown>) => {
      await new Promise((r) => setTimeout(r, 90));
      // event 插件通道（listen/unlisten）
      if (cmd === "plugin:event|listen") return ++listenerSeq;
      if (cmd === "plugin:event|unlisten") return null;
      const fn = commands[cmd];
      if (!fn) throw new Error(`mock: 未收录命令 ${cmd}`);
      return fn(args);
    },
    transformCallback: (cb: unknown) => {
      void cb;
      return ++cbSeq;
    },
    convertCallback: (cb: unknown) => {
      void cb;
      return ++cbSeq;
    },
    metadata: {
      currentWindow: { label: "main" },
      currentWebview: { windowLabel: "main", label: "main" },
    },
    plugins: {},
  };
  console.info("[quartz-mock] Tauri internals 已注入（仅开发态预览用）");
}
