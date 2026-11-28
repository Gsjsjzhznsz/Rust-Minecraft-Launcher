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

/* ============ Modrinth 演示数据集 ============ */

interface DemoHit {
  project_id: string;
  slug: string;
  title: string;
  description: string;
  categories: string[];
  display_categories: string[];
  icon_url: string | null;
  downloads: number;
  author: string;
  date_modified: string;
}

const modrinthCatalog: Record<string, DemoHit[]> = {
  mod: [
    { project_id: "AABBCCDD", slug: "sodium", title: "Sodium", description: "现代化渲染优化，大幅提升帧率", categories: ["optimization", "performance"], display_categories: ["optimization"], icon_url: null, downloads: 4_800_000, author: "jaskarth", date_modified: "2025-11-20" },
    { project_id: "DDEEFF00", slug: "lithium", title: "Lithium", description: "游戏逻辑与通用优化", categories: ["optimization"], display_categories: ["optimization"], icon_url: null, downloads: 2_100_000, author: "jellysquid3", date_modified: "2025-10-12" },
    { project_id: "11223344", slug: "jei", title: "JEI", description: "物品管理器：合成表 / 用途查询", categories: ["utility"], display_categories: ["utility"], icon_url: null, downloads: 3_200_000, author: "mezz", date_modified: "2025-11-01" },
    { project_id: "55667788", slug: "fabric-api", title: "Fabric API", description: "Fabric 模组前置库（多数模组依赖）", categories: ["library"], display_categories: ["library"], icon_url: null, downloads: 9_900_000, author: "FabricMC", date_modified: "2025-11-25" },
  ],
  resourcepack: [
    { project_id: "RP000001", slug: "faithful-32x", title: "Faithful 32x", description: "经典高清材质包", categories: ["vanilla-like"], display_categories: ["vanilla-like"], icon_url: null, downloads: 1_400_000, author: "FaithfulTeam", date_modified: "2025-09-30" },
  ],
  shaderpack: [
    { project_id: "SP000001", slug: "complementary", title: "Complementary Shaders", description: "高质量光照与天空（需 Zink/ANGLE）", categories: ["semi-realistic"], display_categories: ["semi-realistic"], icon_url: null, downloads: 3_800_000, author: "EminGT", date_modified: "2025-11-15" },
  ],
  modpack: [
    { project_id: "MP000001", slug: "prominence", title: "Prominence II RPG", description: "RPG 探索整合包（Fabric）", categories: ["adventure", "rpg"], display_categories: ["adventure"], icon_url: null, downloads: 890_000, author: "prominence", date_modified: "2025-11-18" },
    { project_id: "MP000002", slug: "simply-optimized", title: "Simply Optimized", description: "纯优化向轻量整合（性能拉满零冗余）", categories: ["optimization"], display_categories: ["optimization"], icon_url: null, downloads: 1_100_000, author: "RaptorClaws", date_modified: "2025-10-28" },
  ],
};

function modrinthVersionsFor(projectId: string): Record<string, unknown>[] {
  const hit = Object.values(modrinthCatalog).flat().find((h) => h.project_id === projectId);
  const num = hit?.downloads ?? 0;
  return [
    {
      id: `${projectId}-v2`,
      name: hit?.title ?? projectId,
      version_number: "2.1.0",
      changelog: "性能优化与兼容性修复",
      game_versions: ["1.20.4", "1.20.1"],
      loaders: ["fabric"],
      date_published: "2025-11-20",
      featured: true,
      downloads: num,
      files: [{ url: "", filename: `${hit?.slug ?? "mod"}-2.1.0.jar`, primary: true, size: 1_024_000, hashes: {} }],
      dependencies: [{ version_id: "55667788-v2", project_id: "55667788", file_name: null, dependency_type: projectId === "AABBCCDD" ? "required" : "optional" }],
    },
    {
      id: `${projectId}-v1`,
      name: hit?.title ?? projectId,
      version_number: "2.0.2",
      changelog: "稳定版",
      game_versions: ["1.20.4"],
      loaders: ["fabric"],
      date_published: "2025-10-01",
      featured: false,
      downloads: Math.floor(num / 3),
      files: [{ url: "", filename: `${hit?.slug ?? "mod"}-2.0.2.jar`, primary: true, size: 986_000, hashes: {} }],
      dependencies: [],
    },
  ];
}

/* 演示内容库（安装/导入后追加，联动内容管理页） */
const demoContent: Record<string, { name: string; kind: string; sizeBytes: number; isDir: boolean; modifiedAt: number; disabled: boolean }[]> = {
  mods: [{ name: "sodium-0.6.0.jar", kind: "mods", sizeBytes: 1_024_000, isDir: false, modifiedAt: Math.floor(now / 1000) - 86400, disabled: false }],
  resourcepacks: [{ name: "faithful-32x.zip", kind: "resourcepacks", sizeBytes: 21_000_000, isDir: false, modifiedAt: Math.floor(now / 1000) - 172800, disabled: false }],
  shaderpacks: [{ name: "complementary-v5.zip", kind: "shaderpacks", sizeBytes: 4_600_000, isDir: false, modifiedAt: Math.floor(now / 1000) - 7200, disabled: false }],
  saves: [{ name: "新世界", kind: "saves", sizeBytes: 88_000_000, isDir: true, modifiedAt: Math.floor(now / 1000) - 3600, disabled: false }],
  screenshots: [{ name: "2025-11-24_12.00.png", kind: "screenshots", sizeBytes: 2_400_000, isDir: false, modifiedAt: Math.floor(now / 1000) - 600, disabled: false }],
};

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

  /* --- 实例扩展操作（对标 ZL2/FCL） --- */
  rename_instance_cmd: (a) => {
    const inst = instances.find((x) => x.id === a?.id);
    if (inst) inst.name = String(a?.name ?? inst.name);
    return null;
  },
  duplicate_instance_cmd: (a) => {
    const src = instances.find((x) => x.id === a?.id);
    if (!src) throw new Error("mock: 实例不存在");
    const newId = `${src.id}-copy-${Math.floor(Math.random() * 1e6)}`;
    instances.push({ ...src, id: newId, name: `${src.name} 副本`, selected: false, createdAt: new Date().toISOString().slice(0, 10) });
    return newId;
  },
  set_instance_memory: (a) => {
    const inst = instances.find((x) => x.id === a?.id);
    if (inst) inst.memoryMb = Number(a?.memoryMb ?? inst.memoryMb);
    return null;
  },
  set_instance_renderer: (a) => {
    const inst = instances.find((x) => x.id === a?.id);
    if (inst) inst.renderer = (a?.renderer as string | null) ?? "zink";
    return null;
  },

  /* --- Modrinth 内容生态 --- */
  modrinth_search: (a) => {
    const kind = String(a?.projectType ?? "mod");
    const query = String(a?.query ?? "").toLowerCase();
    let hits = modrinthCatalog[kind] ?? [];
    if (query) hits = hits.filter((h) => h.title.toLowerCase().includes(query) || h.description.toLowerCase().includes(query));
    const loaders = (a?.loaders as string[] | undefined) ?? [];
    if (kind === "mod" && loaders.length && !loaders.includes("fabric")) hits = [];
    return { hits, total_hits: hits.length };
  },
  modrinth_versions: (a) => modrinthVersionsFor(String(a?.projectId ?? "")),
  modrinth_install: (a) => {
    const versionId = String(a?.versionId ?? "");
    const kind = String(a?.kind ?? "mod");
    const projectId = versionId.replace(/-v[12]$/, "");
    const hit = Object.values(modrinthCatalog).flat().find((h) => h.project_id === projectId);
    const dirKind = kind === "modpack" ? null : kind === "resourcepack" ? "resourcepacks" : kind === "shaderpack" ? "shaderpacks" : "mods";
    if (kind === "modpack") {
      void (async () => {
        await simProgress(
          "modrinth://progress",
          [
            { stage: "resolve", weight: 10, file: versionId },
            { stage: "modpack-download", weight: 40, file: `${hit?.slug ?? "modpack"}.mrpack` },
          ],
          2800,
        );
        await simProgress(
          "mrpack://progress",
          [
            { stage: "libraries", weight: 40, file: `${hit?.title ?? "整合包"} 基版本+加载器` },
            { stage: "modpack", weight: 60, file: "overrides 文件（共 214 个）" },
          ],
          4200,
        );
        const id = `mrpack-${Math.floor(Math.random() * 1e6)}`;
        instances.push({ id, name: hit?.title ?? "演示整合包", mcVersion: "1.20.4", loaderDesc: "Fabric 0.15.11", javaMajor: 17, memoryMb, renderer: "zink", createdAt: new Date().toISOString().slice(0, 10), selected: false });
      })();
    } else {
      const depCount = projectId === "AABBCCDD" ? 1 : 0;
      const stages: SimStage[] = [
        { stage: "resolve", weight: 15, file: hit?.title ?? versionId },
      ];
      if (depCount) stages.push({ stage: "download", weight: 25, file: "fabric-api-0.100.0.jar（依赖）" });
      stages.push({ stage: "download", weight: 60, file: `${hit?.slug ?? "mod"}-2.1.0.jar` });
      void simProgress("modrinth://progress", stages, 2600, () => {
        if (dirKind) {
          const list = demoContent[dirKind];
          list?.unshift({ name: `${hit?.slug ?? "mod"}-2.1.0.jar`, kind: dirKind, sizeBytes: 1_024_000, isDir: false, modifiedAt: Math.floor(Date.now() / 1000), disabled: false });
        }
      });
    }
    return null;
  },

  /* --- 整合包导入 --- */
  import_modpack_bytes: () => {
    void simProgress(
      "mrpack://progress",
      [
        { stage: "libraries", weight: 40, file: "演示整合包 基版本+加载器" },
        { stage: "modpack", weight: 60, file: "overrides 文件（共 214 个）" },
      ],
      4200,
      () => {
        const id = `mrpack-${Math.floor(Math.random() * 1e6)}`;
        instances.push({ id, name: "导入的演示整合包", mcVersion: "1.20.1", loaderDesc: "Fabric 0.15.11", javaMajor: 17, memoryMb, renderer: "zink", createdAt: new Date().toISOString().slice(0, 10), selected: false });
      },
    );
    return null;
  },
  import_mrpack: () => {
    void simProgress(
      "mrpack://progress",
      [
        { stage: "libraries", weight: 40, file: "导入整合包 基版本+加载器" },
        { stage: "modpack", weight: 60, file: "overrides 文件" },
      ],
      3600,
      () => {
        const id = `mrpack-${Math.floor(Math.random() * 1e6)}`;
        instances.push({ id, name: "外部导入整合包", mcVersion: "1.20.1", loaderDesc: "Fabric 0.15.11", javaMajor: 17, memoryMb, renderer: "zink", createdAt: new Date().toISOString().slice(0, 10), selected: false });
      },
    );
    return null;
  },
  take_pending_import: () => null,
  consume_pending_import: () => true,

  /* --- 内容 --- */
  list_content_cmd: (a) => {
    const kind = String(a?.kind ?? "mods");
    const list = demoContent[kind];
    return list ? list.map((x) => ({ ...x })) : [];
  },
  delete_content_cmd: (a) => {
    const kind = String(a?.kind ?? "mods");
    const name = String(a?.name ?? "");
    const list = demoContent[kind];
    if (list) {
      const idx = list.findIndex((x) => x.name === name);
      if (idx >= 0) list.splice(idx, 1);
    }
    return true;
  },
  toggle_content_cmd: (a) => {
    const kind = String(a?.kind ?? "mods");
    const name = String(a?.name ?? "");
    const list = demoContent[kind];
    if (!list) throw new Error("mock: 内容不存在");
    const item = list.find((x) => x.name === name);
    if (!item) throw new Error("mock: 内容不存在");
    item.disabled = !item.disabled;
    item.name = item.disabled ? name.replace(/\.disabled$/, "") + ".disabled" : name.replace(/\.disabled$/, "");
    return item.name;
  },

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
  get_runtime_status: (a) => {
    const major = Number(a?.javaMajor ?? 17);
    const installed = major === 8 || major === 17;
    return { installed, version: installed ? String(major) : null, path: `/jres/${major}`, specAvailable: major !== 25 };
  },
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
