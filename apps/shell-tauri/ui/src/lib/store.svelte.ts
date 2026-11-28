/* Quartz 全局状态与动作 —— Svelte 5 runes 版（自旧 main.ts 1:1 移植全部逻辑） */
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  KIND_ORDER,
  PAGE_SIZE,
  STAGE_LABELS,
  type Account,
  type AccountEntry,
  type AppMeta,
  type AssetsStatus,
  type ContentEntry,
  type DownloadProgress,
  type EnvInfo,
  type InstanceEntry,
  type LaunchExit,
  type LaunchLog,
  type LoaderEntry,
  type LogEntry,
  type MigrateMsg,
  type MsPrompt,
  type MsResult,
  type Page,
  type Preflight,
  type ProjectVersion,
  type RendererInfo,
  type SearchHit,
  type SearchResp,
  type SettingsInfo,
  type VersionInfo,
} from "./types";
import { IS_DEMO } from "./demo";
import { bytes, fmtBytes } from "./format";

/* ============ Toast ============ */
export type Toast = { id: number; msg: string; kind: "ok" | "err" | "" };
export const toasts = $state<{ list: Toast[] }>({ list: [] });
let toastSeq = 0;
export function toast(msg: string, kind: "ok" | "err" | "" = "") {
  const id = ++toastSeq;
  toasts.list.push({ id, msg, kind });
  window.setTimeout(() => {
    const i = toasts.list.findIndex((t) => t.id === id);
    if (i >= 0) toasts.list.splice(i, 1);
  }, 3200);
}

/* ============ 会话 / 导航 ============ */
export const session = $state({
  account: null as Account | null,
  entered: false,
  page: "home" as Page,
});

/* ============ 诊断横幅 ============ */
export const diag = $state({ visible: false, text: "" });

/* ============ 版本 ============ */
export const versionsState = $state({
  list: null as VersionInfo[] | null,
  note: "版本清单加载中…",
  installed: [] as string[],
  kindFilter: "all",
  search: "",
  shown: PAGE_SIZE,
  selected: window.localStorage.getItem("quartz.selectedVersion") ?? "",
  busy: false,
});

export function isSelectedInstalled(): boolean {
  return versionsState.installed.includes(versionsState.selected);
}

async function loadVersions(showLoading = true) {
  if (showLoading) versionsState.note = "版本清单加载中…";
  try {
    const list = await invoke<VersionInfo[]>("list_versions");
    if (list.length === 0) throw new Error("版本清单为空");
    versionsState.list = list;
    // 恢复上次选择；无法恢复时取第一个版本（对齐旧版 restoreSelected 语义）
    const prev = versionsState.selected;
    if (!prev || !(list.some((v) => v.id === prev) || versionsState.installed.includes(prev))) {
      versionsState.selected = list[0]?.id ?? "";
    }
    // ZL2/FCL 语义：主页「当前版本」应是已安装实例 —— 默认选中优先落到已安装版本
    if (
      versionsState.installed.length > 0 &&
      !versionsState.installed.includes(versionsState.selected)
    ) {
      versionsState.selected = versionsState.installed[0];
    }
    window.localStorage.setItem("quartz.selectedVersion", versionsState.selected);
  } catch (e) {
    versionsState.list = null;
    versionsState.note = `版本清单拉取失败：${String(e)}（点击下方按钮重试）`;
  }
}

async function refreshInstalled() {
  try {
    versionsState.installed = await invoke<string[]>("list_installed_versions");
  } catch {
    versionsState.installed = [];
  }
}

export function retryLoadVersions() {
  void loadVersions(false);
}

/* ============ 下载进度（home 与 download 页共享同一份状态） ============ */
export type ProgressStatus = { kind: "ok" | "err"; text: string } | null;
export const downloadState = $state({
  visible: false,
  percent: 0,
  current: 0,
  total: 0,
  stage: "",
  file: "",
  status: null as ProgressStatus,
});

/* ============ Java 运行时 ============ */
export const rtState = $state({
  visible: false,
  percent: 0,
  current: 0,
  total: 0,
  stage: "",
  file: "",
});
let runtimeDownloading = false;
let pendingLaunchVersion: string | null = null;

/* ============ 渲染层 / LWJGL（共用进度卡） ============ */
export const rendererState = $state({
  visible: false,
  percent: 0,
  current: 0,
  total: 0,
  stage: "",
  file: "",
  kind: "renderer" as "renderer" | "lwjgl",
});
let rendererDownloading = false;
let lwjglDownloading = false;

/* ============ Modloader ============ */
export const loaderState = $state({
  kind: "fabric",
  mcInput: "",
  versions: [] as LoaderEntry[],
  visible: false,
  percent: 0,
  file: "",
  stage: "",
  status: null as ProgressStatus,
  error: "",
});

/* ============ 启动控制台 ============ */
const CONSOLE_MAX_LINES = 400;
export const launchState = $state({
  lines: [{ source: "dim", line: "[quartz] 启动日志将显示在这里" }] as { source: string; line: string }[],
  badgeText: "空闲",
  badgeCls: "dim" as "dim" | "ok" | "warn" | "err",
  stopVisible: false,
  exitText: null as string | null,
});
export function appendLog(source: string, line: string) {
  const box = launchState.lines;
  if (box.length === 1 && box[0].source === "dim") box.length = 0;
  box.push({ source, line });
  while (box.length > CONSOLE_MAX_LINES) box.shift();
}
export function clearConsole() {
  launchState.lines = [{ source: "dim", line: "[quartz] 启动日志将显示在这里" }];
}
function setLaunchBadge(text: string, cls: "dim" | "ok" | "warn" | "err") {
  launchState.badgeText = text;
  launchState.badgeCls = cls;
}

/* ============ 环境 / 权限 / 徽标 ============ */
export const envState = $state({
  gameDir: "读取中…",
  storageMode: "-",
  storageFree: "-",
  layoutOk: true,
  layoutStatus: "-",
  storageBadge: { text: "检测中…", cls: "dim" as string },
  fullAccess: true,
  permState: "-",
  meta: null as AppMeta | null,
  runtimeBadge: { text: "运行时检测中…", cls: "dim" as string },
  runtimeText: "-",
  lwjglState: "-",
  lwjglOk: false,
  renderers: [] as RendererInfo[],
  rendererBadge: { text: "-", cls: "dim" as string },
});

export const permBanners = $state({
  login: false,
  main: false,
  settings: false,
});

function togglePermUI(granted: boolean) {
  permBanners.login = !granted;
  permBanners.main = !granted;
  permBanners.settings = !granted;
  envState.permState = granted ? "已授权（完整文件访问）" : "未授权";
}

export async function requestPermission() {
  try {
    await invoke("request_storage_permission");
    toast("Android 11+ 已打开「所有文件访问」设置页，请开启后返回应用；低版本请在弹窗中允许", "");
  } catch (e) {
    toast(`权限请求失败：${String(e)}`, "err");
  }
  window.setTimeout(refreshPermission, 1200);
  window.setTimeout(refreshPermission, 3000);
}

export async function refreshPermission() {
  let granted = true;
  try {
    granted = (await invoke<EnvInfo>("get_env_info")).fullAccess;
  } catch {
    return; // 桥接不可用时静默
  }
  const changed = granted !== envState.fullAccess;
  envState.fullAccess = granted;
  togglePermUI(granted);
  if (changed && granted) {
    toast("已获得完整文件权限，正在切换标准目录…", "ok");
    try {
      const diag = await invoke<EnvInfo>("reinit_storage");
      envState.gameDir = diag.gameDir;
      appendLog("quartz", `[quartz] 存储已切换到：${diag.gameDir}`);
      await refreshInstalled();
    } catch (e) {
      toast(`存储切换失败：${String(e)}`, "err");
    }
  }
}

/* ============ 环境与元信息 ============ */
export const storageWarn = $state({ visible: false, text: "" });

async function loadEnvAndMeta() {
  try {
    const env = await invoke<EnvInfo>("get_env_info");
    envState.gameDir = env.gameDir;
    const modeText =
      env.storageMode === "public"
        ? "公共标准目录（文件管理器可见，卸载不丢）"
        : env.storageMode === "external"
          ? "外部私有目录（可在文件管理器查看）"
          : env.storageMode === "internal"
            ? "内部私有目录（应用专属）"
            : "桌面环境";
    envState.storageMode = modeText;
    if (typeof env.freeBytes === "number") {
      envState.storageFree = fmtBytes(env.freeBytes) + " 可用";
    }
    envState.fullAccess = env.fullAccess;
    togglePermUI(env.fullAccess);
    envState.storageBadge = env.layoutOk
      ? { text: "可写", cls: "ok" }
      : { text: "不可写", cls: "err" };
    envState.layoutOk = env.layoutOk;
    envState.layoutStatus = env.layoutOk ? "标准 .minecraft 目录已就绪" : `初始化失败：${env.layoutError ?? "未知"}`;
    if (!env.layoutOk) {
      storageWarn.visible = true;
      storageWarn.text = `存储初始化失败：${env.layoutError ?? "未知错误"}。登录与下载功能可能不可用，请反馈完整报错。`;
    } else {
      storageWarn.visible = false;
    }
  } catch (e) {
    envState.gameDir = `获取失败：${String(e)}`;
  }
  try {
    envState.meta = await invoke<AppMeta>("app_meta");
  } catch {
    /* 关于信息获取失败不影响使用 */
  }
}

/* ============ 诊断 ============ */
async function showDiagnostics() {
  try {
    const d = await invoke<{ lastCrash: string | null; lastLaunchTail: string | null; freeBytes: number | null }>(
      "get_diagnostics",
    );
    const parts: string[] = [];
    if (d.lastCrash) parts.push("检测到上次异常退出（闪退证据已落盘）：\n" + d.lastCrash);
    if (typeof d.freeBytes === "number" && d.freeBytes < 500 * 1024 * 1024) {
      parts.push(
        "⚠ 设备存储空间不足（剩余 " +
          fmtBytes(d.freeBytes) +
          "）：下载会失败，应用也可能闪退，建议清理空间后重试",
      );
    }
    if (d.lastLaunchTail) parts.push("最近一次游戏启动日志尾部：\n" + d.lastLaunchTail);
    if (parts.length) {
      diag.text = parts.join("\n\n————\n\n");
      diag.visible = true;
    }
    if (typeof d.freeBytes === "number") {
      envState.storageFree = fmtBytes(d.freeBytes) + " 可用";
    }
  } catch {
    /* 诊断失败不影响启动 */
  }
}

/* ============ Java 运行时状态 ============ */
async function refreshRuntimeBadge() {
  try {
    const st = await invoke<{ installed: boolean; version: string | null; path: string; specAvailable: boolean }>(
      "get_runtime_status",
      { javaMajor: 17 },
    );
    if (st.installed) {
      envState.runtimeBadge = { text: `Java 运行时就绪（${st.version ?? "17"}）`, cls: "ok" };
      envState.runtimeText = `就绪（${st.version ?? "17"}）`;
    } else if (st.specAvailable) {
      envState.runtimeBadge = { text: "运行时未下载（启动时自动获取）", cls: "warn" };
      envState.runtimeText = "未下载（启动时自动获取）";
    } else {
      envState.runtimeBadge = { text: "暂无下载源", cls: "dim" };
      envState.runtimeText = "暂无下载源";
    }
  } catch {
    envState.runtimeBadge = { text: "运行时检测失败", cls: "dim" };
    envState.runtimeText = "检测失败";
  }
}

function showRtProgress() {
  runtimeDownloading = true;
  rtState.visible = true;
  rtState.percent = 0;
  rtState.stage = "准备中";
  rtState.file = "";
}

function onRuntimeProgress(p: DownloadProgress) {
  if (!runtimeDownloading) return;
  if (p.stage === "error") {
    runtimeDownloading = false;
    rtState.visible = false;
    appendLog("quartz", `[quartz] 运行时下载失败：${p.error ?? "未知错误"}`);
    toast("运行时下载失败，可重试", "err");
    pendingLaunchVersion = null;
    return;
  }
  if (p.stage === "done") {
    runtimeDownloading = false;
    rtState.visible = false;
    appendLog("quartz", `[quartz] Java 运行时就绪：${p.file}`);
    toast("Java 运行时就绪", "ok");
    refreshRuntimeBadge();
    const v = pendingLaunchVersion;
    pendingLaunchVersion = null;
    if (v) void doLaunch(v);
    return;
  }
  rtState.stage = p.stage === "runtime-download" ? "下载" : "解包";
  rtState.file = p.file ?? "";
  if (p.total > 0) {
    rtState.percent = Math.min(100, p.percent);
    rtState.current = p.current;
    rtState.total = p.total;
  } else {
    rtState.percent = 100;
    rtState.current = p.current;
    rtState.total = 0;
  }
}

/* ============ 渲染层 ============ */
async function refreshRenderers() {
  let infos: RendererInfo[] = [];
  try {
    infos = await invoke<RendererInfo[]>("list_renderers");
  } catch {
    return;
  }
  envState.renderers = infos;
  const active = infos.find((r) => r.active);
  envState.rendererBadge = active
    ? { text: active.name, cls: active.installed ? "ok" : "warn" }
    : { text: "-", cls: "dim" };
}

export async function downloadRenderer(id: string) {
  if (rendererDownloading) {
    toast("已有渲染层下载任务", "");
    return;
  }
  rendererDownloading = true;
  rendererState.kind = "renderer";
  rendererState.visible = true;
  rendererState.percent = 0;
  rendererState.stage = "下载";
  try {
    await invoke("download_renderer", { id });
  } catch (e) {
    rendererDownloading = false;
    rendererState.visible = false;
    toast(String(e), "err");
  }
}

export async function switchRenderer(id: string, name: string) {
  try {
    await invoke("set_active_renderer", { id });
    toast(`渲染层已切换为 ${name}`, "ok");
    await refreshRenderers();
  } catch (e) {
    toast(String(e), "err");
  }
}

function onRendererProgress(p: DownloadProgress) {
  if (!rendererDownloading) return;
  if (p.stage === "error") {
    rendererDownloading = false;
    rendererState.visible = false;
    appendLog("quartz", `[quartz] 渲染层下载失败：${p.error ?? "未知错误"}`);
    toast("渲染层下载失败，可重试", "err");
    return;
  }
  if (p.stage === "done") {
    rendererDownloading = false;
    rendererState.visible = false;
    appendLog("quartz", `[quartz] 渲染层就绪：${p.file}`);
    toast("渲染层就绪", "ok");
    refreshRenderers();
    return;
  }
  rendererState.file = p.file ?? "";
  if (p.total > 0) {
    rendererState.percent = Math.min(100, p.percent);
    rendererState.current = p.current;
    rendererState.total = p.total;
  } else {
    rendererState.percent = 100;
    rendererState.current = p.current;
    rendererState.total = 0;
  }
}

/* ============ LWJGL ============ */
async function refreshLwjglState() {
  try {
    const st = await invoke<{ installed: boolean; path: string }>("get_lwjgl_status");
    envState.lwjglOk = st.installed;
    envState.lwjglState = st.installed ? "已就绪" : "未下载（启动时自动获取）";
  } catch {
    envState.lwjglState = "检测失败";
    envState.lwjglOk = false;
  }
}

export async function downloadLwjgl() {
  if (lwjglDownloading) {
    toast("已有 LWJGL 下载任务", "");
    return;
  }
  lwjglDownloading = true;
  rendererState.kind = "lwjgl";
  rendererState.visible = true;
  rendererState.percent = 0;
  rendererState.stage = "LWJGL";
  try {
    await invoke("download_lwjgl");
  } catch (e) {
    lwjglDownloading = false;
    rendererState.visible = false;
    toast(String(e), "err");
  }
}

function onLwjglProgress(p: DownloadProgress) {
  if (p.stage === "error") {
    lwjglDownloading = false;
    rendererState.visible = false;
    toast(`LWJGL 下载失败：${p.error ?? ""}`, "err");
    return;
  }
  if (p.stage === "done") {
    lwjglDownloading = false;
    rendererState.visible = false;
    appendLog("quartz", `[quartz] LWJGL 安卓包就绪：${p.file}`);
    toast("LWJGL 安卓包就绪", "ok");
    refreshLwjglState();
    return;
  }
  if (!lwjglDownloading) return;
  rendererState.kind = "lwjgl";
  rendererState.visible = true;
  rendererState.stage = "LWJGL";
  rendererState.file = p.file ?? "";
  if (p.total > 0) {
    rendererState.percent = Math.min(100, p.percent);
    rendererState.current = p.current;
    rendererState.total = p.total;
  }
}

/* ============ 游戏下载 ============ */
function onProgress(p: DownloadProgress) {
  if (p.stage === "error") {
    downloadState.status = {
      kind: "err",
      text: `下载失败（可重试，已下载文件会自动跳过）：${p.error ?? "未知错误"}`,
    };
  } else if (p.stage === "done") {
    downloadState.percent = 100;
    downloadState.stage = "完成";
    downloadState.file = "";
    downloadState.status = { kind: "ok", text: "下载完成！游戏文件已写入标准 .minecraft 目录。" };
  } else {
    downloadState.status = null;
    downloadState.percent = Math.max(0, Math.min(100, p.percent || 0));
    downloadState.current = p.current;
    downloadState.total = p.total;
    downloadState.stage = STAGE_LABELS[p.stage] ?? p.stage;
    downloadState.file = p.file ?? "";
  }

  if (p.stage === "done") {
    versionsState.busy = false;
    refreshInstalled();
    toast("下载完成", "ok");
    refreshAssetsStatus();
    // ZL2/FCL 同款：启动前自动补全完成后，若在等待启动则继续启动
    if (autoRepairThenLaunch) {
      const v = autoRepairThenLaunch;
      autoRepairThenLaunch = null;
      void doLaunch(v);
    }
  } else if (p.stage === "error") {
    versionsState.busy = false;
    toast("下载失败", "err");
    if (autoRepairThenLaunch) {
      autoRepairThenLaunch = null;
      setLaunchBadge("启动失败", "err");
    }
  }
}

/** 启动前自动补全链（ZL2 VERIFY_AND_REPAIR / FCL checkGameCompletion 同款）：
 *  资产缺失 → 自动修复 → 修复完成自动继续启动。 */
let autoRepairThenLaunch: string | null = null;

export async function startDownload(versionId: string) {
  if (!versionId) {
    toast("请先选择版本", "err");
    return;
  }
  if (versionsState.busy) {
    toast("已有下载任务进行中", "err");
    return;
  }
  versionsState.busy = true;
  versionsState.selected = versionId;
  window.localStorage.setItem("quartz.selectedVersion", versionId);
  downloadState.status = null;
  downloadState.percent = 0;
  downloadState.stage = "准备中";
  downloadState.file = "";
  downloadState.current = 0;
  downloadState.total = 0;
  downloadState.visible = true;
  try {
    await invoke("download_game", { versionId });
  } catch (e) {
    versionsState.busy = false;
    downloadState.status = { kind: "err", text: `下载启动失败：${String(e)}` };
    toast("下载启动失败", "err");
  }
}

/* ============ 账户 ============ */
export const accountsState = $state({ list: [] as AccountEntry[], error: "" });

export async function loadAccounts() {
  try {
    accountsState.list = await invoke<AccountEntry[]>("list_accounts");
    const selected = accountsState.list.find((a) => a.selected);
    if (selected) session.account = selected;
  } catch (e) {
    toast(`账号列表加载失败：${String(e)}`, "err");
  }
}

export async function switchAccount(uuid: string, username: string) {
  try {
    await invoke("select_account", { uuid });
    toast(`已切换到 ${username}`, "ok");
    await loadAccounts();
  } catch (e) {
    toast(`切换失败：${String(e)}`, "err");
  }
}

export async function removeAccount(uuid: string, username: string) {
  try {
    await invoke("remove_account", { uuid });
    toast(`已删除 ${username}`, "ok");
    await loadAccounts();
  } catch (e) {
    toast(`删除失败：${String(e)}`, "err");
  }
}

export async function addAccount(name: string): Promise<boolean> {
  accountsState.error = "";
  if (!name) {
    accountsState.error = "请输入用户名";
    return false;
  }
  try {
    const acc = await invoke<Account>("offline_login", { name });
    toast(`已添加账号 ${acc.username}`, "ok");
    await loadAccounts();
    return true;
  } catch (e) {
    accountsState.error = String(e);
    return false;
  }
}

/* ============ 登录 ============ */
export const loginState = $state({ error: "", pending: false });

export async function doLogin(name: string): Promise<boolean> {
  loginState.error = "";
  if (!name) {
    loginState.error = "请输入用户名";
    return false;
  }
  loginState.pending = true;
  try {
    session.account = await invoke<Account>("offline_login", { name });
    enterApp();
    return true;
  } catch (e) {
    loginState.error = String(e);
    return false;
  } finally {
    loginState.pending = false;
  }
}

/* ============ 设置 / 内存 / 资产 / 日志 ============ */
export const settingsState = $state({
  jvmArgs: "",
  azureId: "",
  azureState: "-",
  memory: 1024,
  memoryLabel: "1024 MB",
  assetsBadge: { text: "-", cls: "dim" as string },
  assetsIndex: "-",
  assetsObjects: "-",
  logs: [] as LogEntry[],
  logText: null as string | null,
  authlibState: "-",
  msPrompt: null as string | null,
  msResult: null as { ok: boolean; text: string } | null,
});

async function loadSettings() {
  try {
    const s = await invoke<SettingsInfo>("get_settings");
    settingsState.jvmArgs = (s.extraJvmArgs ?? []).join("\n");
    settingsState.azureId = s.azureClientId ?? "";
    settingsState.azureState = s.azureClientIdSet ? "已配置" : "未配置（微软登录不可用）";
    instancesState.selectedId = s.selectedInstance ?? null;
  } catch (e) {
    toast(String(e), "err");
  }
}

async function refreshMemorySetting() {
  try {
    const mb = await invoke<number>("get_memory_mb");
    settingsState.memory = mb;
    settingsState.memoryLabel = `${mb} MB`;
  } catch {
    /* 桌面/异常时静默 */
  }
}

export async function saveMemorySetting(mb: number) {
  try {
    await invoke("set_memory_mb", { mb });
    settingsState.memoryLabel = `${mb} MB`;
    toast(`内存分配已保存为 ${mb} MB（下次启动生效）`, "ok");
  } catch (e) {
    toast(String(e), "err");
  }
}

export async function refreshAssetsStatus() {
  const vid = versionsState.selected;
  if (!vid) {
    settingsState.assetsBadge = { text: "未选版本", cls: "dim" };
    settingsState.assetsIndex = "-";
    settingsState.assetsObjects = "-";
    return;
  }
  try {
    const s = await invoke<AssetsStatus>("assets_status", { versionId: vid });
    if (!s.indexPresent) {
      settingsState.assetsBadge = { text: "无索引", cls: "warn" };
      settingsState.assetsIndex = "未安装（重新下载该版本可修复）";
      settingsState.assetsObjects = "-";
      return;
    }
    settingsState.assetsIndex = `${s.indexId}（${s.total} 个对象）`;
    if (s.missing === 0) {
      settingsState.assetsBadge = { text: "完整", cls: "ok" };
      settingsState.assetsObjects = "全部就绪";
    } else {
      settingsState.assetsBadge = { text: `缺 ${s.missing}`, cls: "err" };
      settingsState.assetsObjects = `缺失 ${s.missing} 个（约 ${bytes(s.missingBytes)}）`;
    }
  } catch (e) {
    settingsState.assetsBadge = { text: "检测失败", cls: "dim" };
    settingsState.assetsIndex = String(e);
  }
}

export async function repairAssets() {
  const vid = versionsState.selected;
  if (!vid) {
    toast("请先在主页选择版本", "err");
    return;
  }
  downloadState.visible = true;
  try {
    await invoke("repair_assets", { versionId: vid });
    toast("资产修复已开始，进度见主页/下载页", "");
  } catch (e) {
    toast(String(e), "err");
  }
}

async function refreshLogList() {
  try {
    settingsState.logs = await invoke<LogEntry[]>("list_logs");
  } catch {
    settingsState.logs = [];
  }
}

export async function viewLogTail(name: string) {
  if (!name) {
    toast("暂无日志", "err");
    return;
  }
  try {
    settingsState.logText = (await invoke<string>("read_log_tail", { name })) || "（空）";
  } catch (e) {
    toast(String(e), "err");
  }
}

export async function copyLogTail(name: string) {
  if (!name) {
    toast("暂无日志", "err");
    return;
  }
  try {
    const text = await invoke<string>("read_log_tail", { name });
    await navigator.clipboard.writeText(text);
    toast("已复制日志尾部", "ok");
  } catch (_) {
    toast("复制失败，请长按选择文本", "err");
  }
}

export async function saveJvmArgs(text: string) {
  const args = text
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter((l) => l.length > 0);
  try {
    await invoke("set_jvm_args_cmd", { args });
    toast("JVM 参数已保存", "ok");
  } catch (e) {
    toast(String(e), "err");
  }
}

export async function saveAzureClientId(v: string) {
  try {
    await invoke("set_azure_client_id", { id: v || null });
    settingsState.azureState = v ? "已配置" : "未配置（微软登录不可用）";
    toast("已保存 client_id", "ok");
  } catch (e) {
    toast(String(e), "err");
  }
}

/* ============ 启动游戏 ============ */
async function doLaunch(versionId: string) {
  try {
    setLaunchBadge("启动中…", "warn");
    if (instancesState.selectedId) {
      appendLog("quartz", `[quartz] 实例隔离模式：${instancesState.selectedId}`);
    }
    appendLog("quartz", `[quartz] 启动 Minecraft ${versionId}…`);
    await invoke("launch_game", { versionId, instanceId: instancesState.selectedId || null });
    setLaunchBadge("运行中", "ok");
    launchState.stopVisible = true;
  } catch (e) {
    const msg = String(e);
    if (msg.startsWith("RUNTIME_MISSING:")) {
      const major = Number(msg.split(":")[1] || 17);
      appendLog("quartz", `[quartz] 需要 Java ${major} 运行时，开始自动下载…`);
      showRtProgress();
      try {
        await invoke("download_runtime", { javaMajor: major });
      } catch (e2) {
        runtimeDownloading = false;
        rtState.visible = false;
        toast(String(e2), "err");
        setLaunchBadge("启动失败", "err");
      }
      return;
    }
    setLaunchBadge("启动失败", "err");
    launchState.stopVisible = false;
    appendLog("quartz", `[quartz] 启动失败：${msg}`);
    toast(msg, "err");
  }
}

export async function launch() {
  const versionId = versionsState.selected;
  if (!versionId) {
    toast("请先选择版本", "err");
    return;
  }
  if (!versionsState.installed.includes(versionId)) {
    toast("该版本尚未下载，请先在「下载游戏」页获取", "err");
    return;
  }
  try {
    const pf = await invoke<Preflight>("launch_preflight", { versionId });
    if (!pf.accountOk) {
      toast("请先在「账户」页登录", "err");
      session.page = "account";
      return;
    }
    if (!pf.versionInstalled) {
      toast("版本文件缺失，请重新下载", "err");
      return;
    }
    if (!pf.runtimeInstalled) {
      if (!pf.runtimeSpecAvailable) {
        toast(`该版本需要 Java ${pf.javaMajor}，运行时下载源暂未收录`, "err");
        return;
      }
      appendLog("quartz", `[quartz] 需要 Java ${pf.javaMajor} 运行时，开始自动下载…`);
      pendingLaunchVersion = versionId;
      showRtProgress();
      await invoke("download_runtime", { javaMajor: pf.javaMajor });
      return; // 完成事件回调里继续 doLaunch
    }
    // 自动补全链：启动前检查资产，缺失则自动修复后继续（可跳过手动修复）
    try {
      const st = await invoke<AssetsStatus>("assets_status", { versionId });
      if (st.indexPresent && st.missing > 0) {
        appendLog(
          "quartz",
          `[quartz] 检测到 ${st.missing} 个缺失资源文件（约 ${bytes(st.missingBytes)}），启动前自动补全…`,
        );
        setLaunchBadge("自动补全资源…", "warn");
        autoRepairThenLaunch = versionId;
        downloadState.visible = true;
        await invoke("repair_assets", { versionId });
        return;
      }
    } catch {
      /* 资产检查失败不阻塞启动（启动后端还会做完整校验） */
    }
    await doLaunch(versionId);
  } catch (e) {
    toast(String(e), "err");
  }
}

export async function stopGame() {
  try {
    const stopped = await invoke<boolean>("stop_game");
    if (stopped) {
      toast("已发送终止信号", "ok");
      appendLog("quartz", "[quartz] 已发送终止信号 (SIGKILL)");
    } else {
      toast("当前没有运行中的游戏", "");
    }
  } catch (e) {
    toast(String(e), "err");
  }
}

/* ============ 实例 ============ */
export const instancesState = $state({
  list: [] as InstanceEntry[],
  selectedId: null as string | null,
  formError: "",
});

export async function refreshInstances() {
  try {
    instancesState.list = await invoke<InstanceEntry[]>("list_instances");
  } catch (e) {
    instancesState.list = [];
  }
}

export async function createInstance(
  name: string,
  mc: string,
  loader: string,
  loaderVer: string,
): Promise<boolean> {
  instancesState.formError = "";
  if (!name || !mc) {
    instancesState.formError = "实例名称与 MC 版本均为必填";
    return false;
  }
  try {
    let lv = loaderVer;
    if (loader && !lv) {
      const list = await invoke<LoaderEntry[]>("list_loader_versions", { loader, mc });
      const stable = list.find((v) => v.stable) ?? list[0];
      if (!stable) throw new Error(`没有可用的 ${loader} 版本`);
      lv = stable.version;
    }
    await invoke("create_instance_cmd", {
      name,
      mcVersion: mc,
      loader: loader || null,
      loaderVersion: lv || null,
    });
    toast("实例已创建", "ok");
    await refreshInstances();
    await loadSettings();
    return true;
  } catch (e) {
    instancesState.formError = String(e);
    return false;
  }
}

export async function instanceAction(act: string, id: string) {
  try {
    if (act === "select") {
      await invoke("select_instance", { id });
      toast("已切换实例", "ok");
    } else if (act === "unselect") {
      await invoke("select_instance", { id: null });
      toast("已回到全局 .minecraft", "ok");
    } else if (act === "delete") {
      if (!window.confirm(`删除实例 ${id}（含游戏目录）？`)) return;
      await invoke("delete_instance_cmd", { id });
      toast("实例已删除", "ok");
    }
    await refreshInstances();
    await loadSettings();
  } catch (e) {
    toast(String(e), "err");
  }
}

/* ============ 内容管理 ============ */
export const contentState = $state({
  kind: "mods",
  list: null as ContentEntry[] | null,
  scope: "全局 .minecraft",
  error: "",
});

export async function refreshContent() {
  contentState.scope = instancesState.selectedId
    ? `实例 ${instancesState.selectedId}`
    : "全局 .minecraft";
  try {
    contentState.list = await invoke<ContentEntry[]>("list_content_cmd", {
      kind: contentState.kind,
      instanceId: instancesState.selectedId,
    });
    contentState.error = "";
  } catch (e) {
    contentState.list = [];
    contentState.error = String(e);
  }
}

export async function deleteContent(name: string) {
  if (!window.confirm(`删除 ${name}？`)) return;
  try {
    await invoke("delete_content_cmd", {
      kind: contentState.kind,
      name,
      instanceId: instancesState.selectedId,
    });
    toast("已删除", "ok");
    await refreshContent();
  } catch (e) {
    toast(String(e), "err");
  }
}

/* ============ Modloader ============ */
export async function loadLoaderVersions() {
  const mc = loaderState.mcInput.trim();
  loaderState.error = "";
  if (!mc) {
    loaderState.error = "请先输入 MC 版本";
    return;
  }
  try {
    loaderState.versions = await invoke<LoaderEntry[]>("list_loader_versions", {
      loader: loaderState.kind,
      mc,
    });
  } catch (e) {
    loaderState.versions = [];
    loaderState.error = String(e);
  }
}

export async function installLoader(version: string) {
  const mc = loaderState.mcInput.trim();
  if (!mc) {
    toast("请先输入 MC 版本", "err");
    return;
  }
  loaderState.visible = true;
  loaderState.stage = loaderState.kind;
  loaderState.percent = 0;
  loaderState.status = null;
  try {
    await invoke("install_loader_cmd", { mc, loader: loaderState.kind, loaderVersion: version });
    toast("安装任务已开始，进度实时更新", "ok");
  } catch (e) {
    loaderState.visible = false;
    toast(String(e), "err");
  }
}

function onLoaderProgress(p: DownloadProgress) {
  loaderState.visible = true;
  loaderState.stage = p.stage;
  loaderState.percent = p.percent;
  loaderState.file = p.file || "";
  if (p.error) {
    loaderState.status = { kind: "err", text: p.error };
  } else if (p.done) {
    loaderState.status = { kind: "ok", text: "安装完成，可在主页版本选择中找到新版本" };
    versionsState.installed.push(String(p.file || ""));
    void refreshInstalled();
  }
}

/* ============ 微软 / authlib ============ */
export async function startMsLogin() {
  try {
    await invoke("ms_login");
  } catch (e) {
    toast(String(e), "err");
  }
}

function onMsPrompt(dc: MsPrompt) {
  settingsState.msPrompt = `请在浏览器打开 ${dc.verificationUri} 并输入代码 ${dc.userCode}，等待授权完成…`;
  settingsState.msResult = null;
  // ZL2 同款自动化：设备码自动复制到剪贴板，用户打开验证页后直接粘贴
  void navigator.clipboard?.writeText(dc.userCode).then(
    () => toast("验证代码已复制到剪贴板", "ok"),
    () => {},
  );
}

function onMsResult(r: MsResult) {
  if (r.ok && r.account) {
    settingsState.msResult = { ok: true, text: `微软登录成功：${r.account.username}` };
    void loadAccounts();
  } else {
    settingsState.msResult = { ok: false, text: `微软登录失败：${r.error ?? "未知错误"}` };
  }
}

export async function refreshAuthlib() {
  try {
    const s = await invoke<{ installed: boolean; version: string | null }>("authlib_status");
    settingsState.authlibState = s.installed ? `已安装 ${s.version ?? ""}` : "未安装";
  } catch {
    settingsState.authlibState = "-";
  }
}

export async function downloadAuthlibAgent() {
  try {
    await invoke("download_authlib");
    toast("agent 下载任务已开始", "ok");
  } catch (e) {
    toast(String(e), "err");
  }
}

export async function authlibLogin(
  server: string,
  user: string,
  pass: string,
): Promise<boolean> {
  const err = el_authlibError;
  err.text = "";
  if (!server || !user || !pass) {
    err.text = "服务器地址、账号、密码均为必填";
    return false;
  }
  try {
    const acc = await invoke<Account>("authlib_login", { server, username: user, password: pass });
    toast(`外置登录成功：${acc.username}`, "ok");
    await loadAccounts();
    return true;
  } catch (e) {
    err.text = String(e);
    return false;
  }
}
/* authlib 表单错误状态（页面可绑定） */
export const authlibForm = $state({ error: "" });
const el_authlibError = authlibForm;

/* ============ 退出详情复制 ============ */
export async function copyExitDetail() {
  try {
    await navigator.clipboard.writeText(launchState.exitText ?? "");
    toast("已复制退出详情", "ok");
  } catch (_) {
    toast("复制失败，请长按选择文本", "err");
  }
}

/* ============ 页面切换（懒加载） ============ */
export function showPage(page: Page) {
  session.page = page;
  if (page === "instances") void refreshInstances();
  if (page === "settings") void loadSettings();
  if (page === "account") void refreshAuthlib();
}

/* ============ 进入主界面 ============ */
export function enterApp() {
  session.entered = true;
  loadEnvAndMeta();
  loadAccounts();
  loadVersions();
  refreshInstalled();
  refreshRuntimeBadge();
  refreshPermission();
  refreshRenderers();
  refreshLwjglState();
  refreshMemorySetting();
  refreshAssetsStatus();
  refreshLogList();
  loadSettings();
  refreshInstances();
  refreshAuthlib();
  refreshJavaRuntimes();
  void takePendingImport();
}

/** 前台回来时再检查一次外部投递的整合包（onNewIntent 无事件通道，轮询兑底） */
if (typeof document !== "undefined") {
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "visible" && session.entered) {
      void takePendingImport();
    }
  });
}

/* ============ 事件通道（一次性注册） ============ */
let eventsBound = false;
export async function initEvents() {
  if (eventsBound) return;
  eventsBound = true;
  await listen<DownloadProgress>("download://progress", (ev) => onProgress(ev.payload));
  await listen<DownloadProgress>("runtime://progress", (ev) => onRuntimeProgress(ev.payload));
  await listen<DownloadProgress>("renderer://progress", (ev) => onRendererProgress(ev.payload));
  await listen<DownloadProgress>("lwjgl://progress", (ev) => onLwjglProgress(ev.payload));
  await listen<LaunchLog>("launch://log", (ev) => appendLog(ev.payload.source, ev.payload.line));
  await listen<LaunchExit>("launch://exit", (ev) => {
    launchState.stopVisible = false;
    const code = ev.payload.code;
    if (code === 0) {
      setLaunchBadge("已退出（0）", "dim");
      toast("游戏已正常退出", "ok");
      launchState.exitText = null;
    } else {
      setLaunchBadge(`已退出（${code}）`, "err");
      toast(`游戏已退出（代码 ${code}），详情见控制台`, "err");
      launchState.exitText = ev.payload.logTail || "（无日志输出 —— 进程未产出任何内容）";
    }
    appendLog("quartz", `[quartz] 进程退出，代码 ${code}`);
    if (ev.payload.logTail) {
      for (const line of ev.payload.logTail.split("\n").slice(-40)) {
        appendLog("stderr", line);
      }
    }
  });
  await listen<MigrateMsg>("quartz://migrate", (ev) => {
    const p = ev.payload;
    if (p.error) {
      toast(`存储迁移失败：${p.error}`, "err");
      appendLog("quartz", `[quartz] 存储迁移失败：${p.error}`);
      return;
    }
    if (p.done) {
      toast("旧游戏数据已迁移到标准目录", "ok");
      appendLog("quartz", "[quartz] 存储迁移完成");
      refreshInstalled();
      loadEnvAndMeta();
    }
  });
  await listen<DownloadProgress>("loader://progress", (ev) => onLoaderProgress(ev.payload));
  await listen<DownloadProgress>("modrinth://progress", (ev) => onModrinthProgress(ev.payload));
  await listen<MsPrompt>("ms://prompt", (ev) => onMsPrompt(ev.payload));
  await listen<MsResult>("ms://result", (ev) => onMsResult(ev.payload));
  await listen<{ done: boolean; version?: string | null; error?: string | null }>(
    "authlib://progress",
    (ev) => {
      if (ev.payload.error) {
        toast(`agent 下载失败：${ev.payload.error}`, "err");
      } else {
        toast(`agent 已安装：${ev.payload.version ?? ""}`, "ok");
      }
      void refreshAuthlib();
    },
  );
  await listen<DownloadProgress>("mrpack://progress", (ev) => {
    const p = ev.payload;
    const pr = importState.progress;
    if (p.error) {
      toast(`整合包导入失败：${p.error}`, "err");
      appendLog("quartz", `[quartz] 整合包导入失败：${p.error}`);
      pr.status = { kind: "err", text: p.error ?? "未知错误" };
    } else if (p.done) {
      toast("整合包导入完成", "ok");
      appendLog("quartz", "[quartz] 整合包导入完成");
      pr.percent = 100;
      pr.stage = "完成";
      pr.status = { kind: "ok", text: "导入完成！新实例已就绪，可在「实例」页查看。" };
      pr.file = p.file || "";
      void refreshInstances();
    } else {
      pr.visible = true;
      pr.percent = Math.max(0, Math.min(100, p.percent || 0));
      pr.stage = STAGE_LABELS[p.stage] ?? p.stage;
      pr.file = p.file || p.stage;
      pr.status = null;
      appendLog("quartz", `[quartz] 整合包导入 ${Math.round(p.percent)}%：${p.file || p.stage}`);
    }
  });
}

/* ============ Modrinth 内容生态（ZL2/FCL 下载页同款分区） ============ */
export type ModrinthKind = "mod" | "resourcepack" | "shaderpack" | "modpack";

export const modrinthState = $state({
  query: "",
  loaders: [] as string[],
  gameVersions: "",
  sort: "relevance",
  hits: null as SearchHit[] | null,
  totalHits: 0,
  loading: false,
  error: "",
  shown: 20,
  /* 当前生效的分区（切 Tab 时触发重搜） */
  lastKind: null as ModrinthKind | null,
  /* 项目详情（版本列表） */
  detail: null as { hit: SearchHit; versions: ProjectVersion[] } | null,
  detailLoading: false,
  /* 安装进度 */
  progress: { visible: false, stage: "", percent: 0, file: "", status: null as ProgressStatus },
});

export function modrinthFiltersFor(kind: ModrinthKind): { loaders: string[]; gameVersions: string[] } {
  const gv = modrinthState.gameVersions.trim();
  return {
    loaders: kind === "mod" ? modrinthState.loaders : [],
    gameVersions: gv ? [gv] : [],
  };
}

export async function modrinthSearch(kind: ModrinthKind, reset = true) {
  if (reset) {
    modrinthState.shown = 20;
    modrinthState.error = "";
  }
  modrinthState.lastKind = kind;
  modrinthState.loading = true;
  try {
    const f = modrinthFiltersFor(kind);
    const resp = await invoke<SearchResp>("modrinth_search", {
      query: modrinthState.query,
      projectType: kind,
      loaders: f.loaders,
      gameVersions: f.gameVersions,
      index: modrinthState.sort,
      offset: 0,
      limit: 50,
    });
    modrinthState.hits = resp.hits;
    modrinthState.totalHits = resp.total_hits;
  } catch (e) {
    modrinthState.hits = [];
    modrinthState.error = String(e);
  } finally {
    modrinthState.loading = false;
  }
}

export async function modrinthOpen(hit: SearchHit, kind: ModrinthKind) {
  modrinthState.detail = { hit, versions: [] };
  modrinthState.detailLoading = true;
  try {
    const f = modrinthFiltersFor(kind);
    const list = await invoke<ProjectVersion[]>("modrinth_versions", {
      projectId: hit.project_id,
      loaders: f.loaders,
      gameVersions: f.gameVersions,
    });
    modrinthState.detail.versions = list;
  } catch (e) {
    toast(`版本列表加载失败：${String(e)}`, "err");
  } finally {
    modrinthState.detailLoading = false;
  }
}

export function modrinthCloseDetail() {
  modrinthState.detail = null;
}

function onModrinthProgress(p: DownloadProgress) {
  const pr = modrinthState.progress;
  if (p.error) {
    pr.status = { kind: "err", text: p.error };
  } else if (p.done) {
    pr.percent = 100;
    pr.stage = "完成";
    pr.file = p.file ?? "";
    pr.status = { kind: "ok", text: "安装完成！内容已写入当前作用域目录。" };
  } else {
    pr.status = null;
    pr.percent = Math.max(0, Math.min(100, p.percent || 0));
    pr.stage = STAGE_LABELS[p.stage] ?? p.stage;
    pr.file = p.file ?? "";
  }
  if (p.done || p.error) {
    if (instancesState.selectedId || contentState.list !== null) {
      void refreshContent();
    }
  }
}

export async function modrinthInstall(versionId: string, kind: ModrinthKind) {
  if (modrinthState.progress.visible) {
    toast("已有内容安装任务", "");
    return;
  }
  modrinthState.progress = {
    visible: true,
    stage: "准备中",
    percent: 0,
    file: "",
    status: null,
  };
  try {
    await invoke("modrinth_install", {
      versionId,
      kind,
      instanceId: instancesState.selectedId,
    });
  } catch (e) {
    modrinthState.progress.visible = false;
    toast(String(e), "err");
  }
}

/* ============ 本地整合包导入（.mrpack / .zip） ============ */
export const importState = $state({
  busy: false,
  fileName: "",
  /* 导入管线进度（mrpack://progress 驱动；Modrinth 整合包下载完成后转入本管线） */
  progress: { visible: false, stage: "", percent: 0, file: "", status: null as ProgressStatus },
});

export async function importModpackFile(file: File): Promise<boolean> {
  if (importState.busy) {
    toast("已有导入任务进行中", "err");
    return false;
  }
  importState.busy = true;
  importState.fileName = file.name;
  try {
    const buf = await file.arrayBuffer();
    // Tauri v2 raw IPC：字节流直传后端（WebView 原生文件选择器 + 系统文件关联双通道）
    await invoke("import_modpack_bytes", new Uint8Array(buf));
    toast(`整合包「${file.name}」导入已开始`, "ok");
    return true;
  } catch (e) {
    toast(`整合包导入失败：${String(e)}`, "err");
    return false;
  } finally {
    importState.busy = false;
  }
}

/** 外部「打开方式」投递的整合包自动导入（ZL2/FCL 文件关联同款自动化）。 */
export async function takePendingImport() {
  try {
    const path = await invoke<string | null>("take_pending_import");
    if (!path) return;
    toast("检测到外部整合包，自动导入…", "ok");
    appendLog("quartz", `[quartz] 文件关联导入：${path}`);
    await invoke("import_mrpack", { path });
    await invoke("consume_pending_import", { path });
  } catch {
    /* 桌面端/演示模式无此能力 */
  }
}

/* ============ Java 运行时管理（ZL2 JavaManage / FCL JavaManageDialog 同款） ============ */
export const JAVA_MAJORS = [8, 17, 21, 25] as const;

export const javaState = $state({
  list: null as { major: number; installed: boolean; version: string | null; specAvailable: boolean }[] | null,
});

export async function refreshJavaRuntimes() {
  const out: { major: number; installed: boolean; version: string | null; specAvailable: boolean }[] = [];
  for (const major of JAVA_MAJORS) {
    try {
      const st = await invoke<{ installed: boolean; version: string | null; specAvailable: boolean }>(
        "get_runtime_status",
        { javaMajor: major },
      );
      out.push({ major, ...st });
    } catch {
      out.push({ major, installed: false, version: null, specAvailable: false });
    }
  }
  javaState.list = out;
}

export async function downloadJavaRuntime(major: number) {
  if (runtimeDownloading) {
    toast("已有运行时下载任务", "");
    return;
  }
  appendLog("quartz", `[quartz] 手动下载 Java ${major} 运行时…`);
  showRtProgress();
  try {
    await invoke("download_runtime", { javaMajor: major });
  } catch (e) {
    runtimeDownloading = false;
    rtState.visible = false;
    toast(String(e), "err");
  }
}

/* ============ 日志上传分享（ZL2 mclo.gs / FCL logshare 同款） ============ */
export const logUpload = $state({ busy: false, url: null as string | null, error: "" });

export async function uploadLogTail(name: string) {
  if (!name) {
    toast("暂无日志", "err");
    return;
  }
  if (IS_DEMO) {
    logUpload.url = "https://api.mclo.gs/1/demo";
    toast("演示模式：真实 APK 中会把日志上传 mclo.gs 生成分享链接", "");
    return;
  }
  logUpload.busy = true;
  logUpload.error = "";
  logUpload.url = null;
  try {
    const text = await invoke<string>("read_log_tail", { name });
    const body = new URLSearchParams({ content: text });
    const resp = await fetch("https://api.mclo.gs/1/log", {
      method: "POST",
      headers: { "Content-Type": "application/x-www-form-urlencoded" },
      body: body.toString(),
    });
    const data = (await resp.json()) as { success: boolean; url?: string; error?: string };
    if (data.success && data.url) {
      logUpload.url = data.url;
      await navigator.clipboard?.writeText(data.url).catch(() => {});
      toast("已上传 mclo.gs，链接已复制", "ok");
    } else {
      throw new Error(data.error ?? "上传失败");
    }
  } catch (e) {
    logUpload.error = String(e);
    toast(`日志上传失败：${String(e)}`, "err");
  } finally {
    logUpload.busy = false;
  }
}

/* ============ 实例扩展操作（重命名 / 复制） ============ */
export async function renameInstance(id: string, name: string) {
  if (!name.trim()) {
    toast("名称不能为空", "err");
    return;
  }
  try {
    await invoke("rename_instance_cmd", { id, name: name.trim() });
    toast("实例已重命名", "ok");
    await refreshInstances();
  } catch (e) {
    toast(String(e), "err");
  }
}

export async function duplicateInstance(id: string) {
  try {
    const newId = await invoke<string>("duplicate_instance_cmd", { id });
    toast(`实例已复制为 ${newId}`, "ok");
    await refreshInstances();
  } catch (e) {
    toast(String(e), "err");
  }
}

export async function toggleContent(name: string) {
  try {
    const newName = await invoke<string>("toggle_content_cmd", {
      kind: contentState.kind,
      name,
      instanceId: instancesState.selectedId,
    });
    toast(newName.endsWith(".disabled") ? "已禁用（文件保留）" : "已启用", "ok");
    await refreshContent();
  } catch (e) {
    toast(String(e), "err");
  }
}

/* ============ 实例内存/渲染器按实例覆盖（FCL VersionSetting 同款） ============ */
export async function setInstanceMemory(id: string, mb: number) {
  try {
    await invoke("set_instance_memory", { id, memoryMb: mb });
    toast(`实例内存已保存为 ${mb} MB`, "ok");
    await refreshInstances();
  } catch (e) {
    toast(String(e), "err");
  }
}

export async function setInstanceRenderer(id: string, renderer: string | null) {
  try {
    await invoke("set_instance_renderer", { id, renderer });
    toast(renderer ? "实例渲染层已覆盖" : "已恢复跟随全局", "ok");
    await refreshInstances();
  } catch (e) {
    toast(String(e), "err");
  }
}

/* ============ Boot ============ */
export async function boot() {
  await initEvents();
  void showDiagnostics();
  await refreshPermission();
  try {
    const acc = await invoke<Account | null>("get_account");
    if (acc && acc.username) {
      session.account = acc;
      enterApp();
      return;
    }
  } catch {
    /* 未登录 */
  }
}
