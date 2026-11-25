/* 演示态模拟后端：无 Tauri 环境（浏览器 dev / GitHub Pages demo 构建）时注入
   __TAURI_INTERNALS__，并内置事件总线 + 全流程模拟器（下载进度 / 启动日志 /
   微软设备码登录等），让整个 UI 在纯网页里可完整"演示"。
   APK 生产构建中 DEV === false 且 VITE_DEMO 未设，调用点被 dead-code 消除，
   本模块不会进入 APK。 */

type Cmd = (args: Record<string, unknown> | undefined) => unknown;

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const now = Date.now();

/* ============ 可变演示状态（让操作"记住"结果） ============ */

let accounts: { kind: string; username: string; uuid: string; selected: boolean }[] = [
  { kind: "offline", username: "Steve", uuid: "5c1c1e9a4b0f9d2e8a7b6c5d4e3f1111", selected: true },
  { kind: "offline", username: "Alex", uuid: "a41d2f3b8c9e7d6a5b4c3d2e1f0a2222", selected: false },
];

const installed: string[] = ["1.20.4", "1.16.5"];

interface DemoInstance {
  id: string;
  name: string;
  mcVersion: string;
  loaderDesc: string;
  javaMajor: number;
  memoryMb: number;
  renderer: string;
  createdAt: string;
  selected: boolean;
}

let instances: DemoInstance[] = [
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
];

const settings = {
  extraJvmArgs: ["-XX:+UseG1GC"] as string[],
  azureClientId: null as string | null,
  selectedInstance: "survival-pack" as string | null,
};
let memoryMb = 2048;

interface DemoRenderer {
  id: string;
  name: string;
  note: string;
  eglLib: string;
  glLib: string;
  sizeHint: string;
  specAvailable: boolean;
  installed: boolean;
  active: boolean;
}

let renderers: DemoRenderer[] = [
  { id: "zink", name: "Zink (Vulkan)", note: "GL4.6→Vulkan，1.17+/Sodium/光影推荐档", eglLib: "", glLib: "", sizeHint: "约 45MB", specAvailable: true, installed: true, active: true },
  { id: "angle-vk", name: "ANGLE (Vulkan)", note: "GL ES→Vulkan，兼容档", eglLib: "", glLib: "", sizeHint: "约 30MB", specAvailable: true, installed: false, active: false },
  { id: "softgl", name: "软渲染", note: "无 GPU 兜底，性能低", eglLib: "", glLib: "", sizeHint: "约 12MB", specAvailable: false, installed: false, active: false },
];

let authlibInstalled = false;
let gameRunning = false;

/* ============ 事件总线（模拟 Rust 侧 emit） ============ */

const callbacks = new Map<number, (ev: { event: string; id: number; payload: unknown }) => void>();
const listeners = new Map<number, { event: string; cbId: number }>();
let cbSeq = 0;
let listenerSeq = 0;
let eventSeq = 0;

function emitEvent(event: string, payload: unknown) {
  for (const l of listeners.values()) {
    if (l.event !== event) continue;
    const cb = callbacks.get(l.cbId);
    if (cb) cb({ event, id: ++eventSeq, payload });
  }
}

/* ============ 进度模拟器 ============ */

interface SimStage {
  stage: string;
  weight: number;
  file?: string;
}

async function simProgress(channel: string, stages: SimStage[], totalMs: number, onDone?: () => void) {
  const totalWeight = stages.reduce((s, x) => s + x.weight, 0);
  let acc = 0;
  const stepMs = Math.max(70, Math.floor(totalMs / (stages.length * 8)));
  for (const st of stages) {
    for (let i = 1; i <= 8; i++) {
      await sleep(stepMs);
      acc += st.weight / 8;
      const percent = Math.min(99, Math.round((acc / totalWeight) * 100));
      emitEvent(channel, { stage: st.stage, current: percent, total: 100, percent, file: st.file ?? "", done: false });
    }
  }
  onDone?.();
  emitEvent(channel, { stage: "done", current: 100, total: 100, percent: 100, file: "", done: true });
}

/* ============ 启动模拟 ============ */

async function simulateLaunch(versionId: string, playerName: string) {
  gameRunning = true;
  const lines: [string, string][] = [
    ["quartz", `[quartz] LaunchPlan 装配完成（Java 17 / Zink (Vulkan) / ${memoryMb}MB）`],
    ["main", "[12:00:01] [main/INFO]: Environment: Environment[name=client, session=...]"],
    ["main", `[12:00:01] [main/INFO]: Setting user: ${playerName}`],
    ["Render thread", "[12:00:02] [Render thread/INFO]: Backend library: LWJGL version 3.3.3-snapshot"],
    ["Render thread", "[12:00:03] [Render thread/INFO]: Reloading ResourceManager: vanilla"],
    ["Render thread", "[12:00:04] [Render thread/INFO]: Opened resource pack: vanilla"],
    ["Render thread", "[12:00:05] [Render thread/INFO]: Sound engine started"],
    ["Render thread", "[12:00:06] [Render thread/INFO]: Created: 1024x512x4 minecraft:textures/atlas/blocks.png-atlas"],
  ];
  for (const [source, line] of lines) {
    emitEvent("launch://log", { source, line });
    await sleep(650);
  }
  await sleep(3000);
  gameRunning = false;
  emitEvent("launch://exit", { code: 0, version: versionId, logTail: null });
}

/* ============ 版本清单（静态演示数据） ============ */

function makeVersions(): { id: string; type: string; url: string }[] {
  const out: { id: string; type: string; url: string }[] = [];
  for (let i = 0; i < 4; i++) out.push({ id: `1.21.${4 - i}`, type: "release", url: "" });
  out.push({ id: "24w14a", type: "snapshot", url: "" });
  out.push({ id: "24w13b", type: "snapshot", url: "" });
  for (const v of ["1.20.4", "1.20.1", "1.19.4", "1.18.2", "1.16.5", "1.12.2", "1.8.9"]) {
    out.push({ id: v, type: "release", url: "" });
  }
  out.push({ id: "b1.7.3", type: "old_beta", url: "" });
  out.push({ id: "a1.2.6", type: "old_alpha", url: "" });
  return out;
}

/* ============ 命令表 ============ */

const commands: Record<string, Cmd> = {
  /* --- 账户 --- */
  get_account: () => accounts.find((a) => a.selected) ?? null,
  list_accounts: () => accounts,
  offline_login: (a) => {
    const name = String(a?.name ?? "Player");
    accounts = [{ kind: "offline", username: name, uuid: "11112222333344445555666677778888", selected: true }];
    return accounts[0];
  },
  select_account: (a) => {
    const name = a?.username;
    for (const acc of accounts) acc.selected = acc.username === name;
    return null;
  },
  remove_account: (a) => {
    accounts = accounts.filter((x) => x.username !== a?.username);
    if (accounts.length && !accounts.some((x) => x.selected)) accounts[0].selected = true;
    return null;
  },
  ms_login: () => {
    void (async () => {
      await sleep(500);
      emitEvent("ms://prompt", {
        userCode: "QUARTZ-DEMO",
        verificationUri: "https://www.microsoft.com/link",
        message: "演示模式：不会真实登录 Microsoft 账号，稍后自动完成",
      });
      await sleep(2600);
      const acc = { kind: "microsoft", username: "DemoPlayer", uuid: "d3m0d3m0d3m0d3m0d3m0d3m0d3m0d3m0", selected: true };
      accounts = [{ ...acc }, ...accounts.map((x) => ({ ...x, selected: false }))];
      emitEvent("ms://result", { ok: true, account: acc });
    })();
    return null;
  },
  authlib_login: (a) => {
    const acc = { kind: "authlib", username: String(a?.username ?? "Player"), uuid: "a7b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5", selected: true };
    accounts = [{ ...acc }, ...accounts.map((x) => ({ ...x, selected: false }))];
    return acc;
  },
  download_authlib: () => {
    void (async () => {
      await sleep(1300);
      authlibInstalled = true;
      emitEvent("authlib://progress", { done: true, version: "3.2.38-demo" });
    })();
    return null;
  },
  authlib_status: () => ({ installed: authlibInstalled, version: authlibInstalled ? "3.2.38-demo" : null }),

  /* --- 版本与下载 --- */
  list_versions: () => makeVersions(),
  list_installed_versions: () => [...installed],
  download_game: (a) => {
    const versionId = String(a?.versionId ?? "1.21.4");
    void simProgress(
      "download://progress",
      [
        { stage: "manifest", weight: 6, file: "version_manifest_v2.json" },
        { stage: "version-json", weight: 6, file: `${versionId}.json` },
        { stage: "client", weight: 34, file: `${versionId}.jar` },
        { stage: "libraries", weight: 22, file: "libraries（共 61 个）" },
        { stage: "asset-index", weight: 4, file: "indexes/5.json" },
        { stage: "assets", weight: 28, file: "objects（共 3932 个）" },
      ],
      6500,
      () => {
        if (!installed.includes(versionId)) installed.push(versionId);
      },
    );
    return null;
  },
  repair_assets: (a) => {
    const versionId = String(a?.versionId ?? "");
    void simProgress(
      "download://progress",
      [
        { stage: "asset-index", weight: 20, file: "indexes/5.json" },
        { stage: "assets", weight: 80, file: "objects（校验缺漏 12 个）" },
      ],
      3200,
    );
    void versionId;
    return null;
  },
  assets_status: () => ({ indexPresent: true, indexId: "5", total: 3932, missing: 0, missingBytes: 0 }),

  /* --- 实例 --- */
  list_instances: () => instances,
  select_instance: (a) => {
    const id = a?.id ?? null;
    settings.selectedInstance = id as string | null;
    for (const inst of instances) inst.selected = inst.id === id;
    return null;
  },
  create_instance_cmd: (a) => {
    const id = String(a?.id ?? `inst-${Math.floor(Math.random() * 1e6)}`);
    instances.push({
      id,
      name: String(a?.name ?? "新实例"),
      mcVersion: String(a?.mcVersion ?? "1.20.4"),
      loaderDesc: a?.loaderDesc ? String(a.loaderDesc) : "原版",
      javaMajor: Number(a?.javaMajor ?? 17),
      memoryMb: Number(a?.memoryMb ?? memoryMb),
      renderer: "zink",
      createdAt: new Date().toISOString().slice(0, 10),
      selected: false,
    });
    return null;
  },
  delete_instance_cmd: (a) => {
    instances = instances.filter((x) => x.id !== a?.id);
    if (settings.selectedInstance === a?.id) settings.selectedInstance = instances[0]?.id ?? null;
    return null;
  },
  install_loader_cmd: (a) => {
    const mc = String(a?.mc ?? "");
    const loader = String(a?.loader ?? "fabric");
    const loaderVersion = String(a?.loaderVersion ?? "");
    const artifact = `${mc}-${loader}-${loaderVersion}`;
    void simProgress(
      "loader://progress",
      [
        { stage: "loader-meta", weight: 30, file: `${loader}-loader-meta.json` },
        { stage: "libraries", weight: 50, file: `${loader}-libraries（共 24 个）` },
        { stage: "profile", weight: 20, file: artifact },
      ],
      4200,
      () => {
        if (!installed.includes(artifact)) installed.push(artifact);
        for (const inst of instances) if (inst.mcVersion === mc) inst.loaderDesc = `${loader} ${loaderVersion}`;
      },
    );
    return null;
  },
  list_loader_versions: () => [
    { version: "0.15.11", stable: true },
    { version: "0.15.10", stable: true },
    { version: "0.16.0-beta.1", stable: false },
  ],

  /* --- 内容 --- */
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

  /* --- 启动 --- */
  launch_preflight: () => ({
    versionInstalled: true,
    versionJsonOk: true,
    javaMajor: 17,
    accountOk: accounts.some((x) => x.selected),
    runtimeInstalled: true,
    runtimeSpecAvailable: true,
    runtimeVersion: "17",
    runtimeDir: "/jres/17",
  }),
  launch_game: (a) => {
    const versionId = String(a?.versionId ?? "");
    const player = accounts.find((x) => x.selected)?.username ?? "Steve";
    void simulateLaunch(versionId, player);
    return null;
  },
  stop_game: () => {
    if (!gameRunning) return false;
    gameRunning = false;
    void (async () => {
      await sleep(800);
      emitEvent("launch://exit", { code: 0, version: null, logTail: null });
    })();
    return true;
  },

  /* --- 运行时 / 渲染层 / LWJGL --- */
  get_runtime_status: () => ({ installed: true, version: "17", path: "/jres/17", specAvailable: true }),
  download_runtime: (a) => {
    const major = Number(a?.javaMajor ?? 17);
    void simProgress(
      "runtime://progress",
      [
        { stage: "runtime-download", weight: 70, file: `openjdk-${major}-aarch64.tar.gz` },
        { stage: "unpack", weight: 30, file: `/jres/${major}` },
      ],
      5200,
    );
    return null;
  },
  list_renderers: () => renderers,
  download_renderer: (a) => {
    const id = String(a?.id ?? "");
    void simProgress(
      "renderer://progress",
      [
        { stage: "download", weight: 70, file: `${id}-pack.zip` },
        { stage: "unpack", weight: 30, file: `${id}` },
      ],
      3800,
      () => {
        for (const r of renderers) r.installed = r.installed || r.id === id;
      },
    );
    return null;
  },
  set_active_renderer: (a) => {
    const id = String(a?.id ?? "");
    for (const r of renderers) r.active = r.id === id && r.installed;
    return null;
  },
  get_lwjgl_status: () => ({ installed: true, path: "/lwjgl" }),
  download_lwjgl: () => {
    void simProgress(
      "lwjgl://progress",
      [
        { stage: "download", weight: 70, file: "lwjgl3.3.3-pack.zip" },
        { stage: "unpack", weight: 30, file: "/lwjgl" },
      ],
      3000,
    );
    return null;
  },

  /* --- 环境 / 设置 --- */
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
  request_storage_permission: () => true,
  get_memory_mb: () => memoryMb,
  set_memory_mb: (a) => {
    memoryMb = Number(a?.mb ?? memoryMb);
    return null;
  },
  get_settings: () => ({
    extraJvmArgs: settings.extraJvmArgs,
    azureClientIdSet: settings.azureClientId !== null,
    azureClientId: settings.azureClientId,
    selectedInstance: settings.selectedInstance,
  }),
  set_jvm_args_cmd: (a) => {
    settings.extraJvmArgs = Array.isArray(a?.args) ? (a.args as string[]) : [];
    return null;
  },
  set_azure_client_id: (a) => {
    settings.azureClientId = (a?.id as string | null) ?? null;
    return null;
  },
  app_meta: () => ({ name: "Quartz Launcher", version: "0.2.8", os: "android", arch: "arm64" }),
  get_diagnostics: () => ({ lastCrash: null, lastLaunchTail: null, freeBytes: 21_000_000_000 }),
  get_window_label: () => "main",
  list_logs: () => [
    { name: "latest.log", size: 128_000, modifiedMs: now - 3600_000 },
    { name: "debug.log", size: 512_000, modifiedMs: now - 7200_000 },
  ],
  read_log_tail: () =>
    "[12:00:01] [main/INFO]: Setting user: Steve\n[12:00:02] [Render thread/INFO]: Backend library: LWJGL",
};

/* ============ 安装入口 ============ */

export function installMock() {
  const w = window as unknown as Record<string, unknown>;
  if (w.__TAURI_INTERNALS__) return;

  w.__TAURI_INTERNALS__ = {
    invoke: async (cmd: string, args?: Record<string, unknown>) => {
      await new Promise((r) => setTimeout(r, 90));
      // 事件插件通道：listen 把 handler（经 transformCallback 得到的 id）按事件名登记
      if (cmd === "plugin:event|listen") {
        listeners.set(++listenerSeq, { event: String(args?.event ?? ""), cbId: Number(args?.handler ?? -1) });
        return listenerSeq;
      }
      if (cmd === "plugin:event|unlisten") return null;
      const fn = commands[cmd];
      if (!fn) throw new Error(`mock: 未收录命令 ${cmd}`);
      return fn(args);
    },
    transformCallback: (cb?: (ev: { event: string; id: number; payload: unknown }) => void) => {
      const id = ++cbSeq;
      if (cb) callbacks.set(id, cb);
      return id;
    },
    convertCallback: (cb?: unknown) => {
      void cb;
      return ++cbSeq;
    },
    metadata: {
      currentWindow: { label: "main" },
      currentWebview: { windowLabel: "main", label: "main" },
    },
    plugins: {},
  };
  console.info("[quartz-mock] 演示后端已注入（模拟数据，仅供网页预览）");
}
