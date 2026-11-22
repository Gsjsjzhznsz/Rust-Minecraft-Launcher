import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

/* ---------- 类型 ---------- */
type Account = { kind: string; username: string; uuid: string };
type AccountEntry = Account & { selected: boolean };
type VersionInfo = { id: string; type: string; url: string };
type DownloadProgress = {
  stage: string;
  current: number;
  total: number;
  percent: number;
  file: string;
  done: boolean;
  error?: string;
};
type EnvInfo = {
  gameDir: string;
  storageMode: string;
  layoutOk: boolean;
  layoutError: string | null;
  fullAccess: boolean;
};
type AppMeta = { name: string; version: string; os: string; arch: string };
type Preflight = {
  versionInstalled: boolean;
  versionJsonOk: boolean;
  javaMajor: number;
  accountOk: boolean;
  runtimeInstalled: boolean;
  runtimeSpecAvailable: boolean;
  runtimeVersion: string | null;
  runtimeDir: string;
};
type LaunchLog = { source: string; line: string };
type LaunchExit = { code: number; version: string };
type MigrateMsg = { percent: number; done: boolean; error: string | null };

/* ---------- DOM 工具 ---------- */
const el = <T extends HTMLElement>(id: string): T => document.getElementById(id) as T;
const show = (id: string) => el<HTMLElement>(id).classList.remove("hidden");
const hide = (id: string) => el<HTMLElement>(id).classList.add("hidden");
const setText = (id: string, text: string) => {
  el<HTMLElement>(id).textContent = text;
};

const KIND_LABELS: Record<string, string> = {
  release: "正式版",
  snapshot: "快照",
  old_beta: "Beta",
  old_alpha: "Alpha",
};
const STAGE_LABELS: Record<string, string> = {
  manifest: "版本清单",
  "version-json": "版本 JSON",
  client: "客户端 jar",
  libraries: "依赖库",
  "asset-index": "资源索引",
  assets: "资源文件",
  done: "完成",
  error: "出错",
};
const KIND_ORDER = ["release", "snapshot", "old_beta", "old_alpha"] as const;
const PAGE_SIZE = 50;

function bytes(n: number): string {
  if (!n) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  let v = n;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i += 1;
  }
  return `${v.toFixed(i === 0 ? 0 : 1)} ${units[i]}`;
}

function alertIn(id: string, cls: string, msg: string) {
  const node = el<HTMLElement>(id);
  node.className = `alert ${cls}`;
  node.textContent = msg;
}
function alertOut(id: string) {
  const node = el<HTMLElement>(id);
  node.textContent = "";
  node.classList.add("hidden");
}

/* ---------- Toast ---------- */
function toast(msg: string, type: "ok" | "err" | "" = "") {
  const wrap = el<HTMLDivElement>("toast-wrap");
  const node = document.createElement("div");
  node.className = `toast ${type}`.trim();
  node.textContent = msg;
  wrap.appendChild(node);
  window.setTimeout(() => node.remove(), 3200);
}

/* ---------- 全局状态 ---------- */
let versions: VersionInfo[] | null = null;
let installed = new Set<string>();
let kindFilter = "all";
let searchText = "";
let shownCount = PAGE_SIZE;
let busy = false;
let currentAccount: Account | null = null;
let fullAccess = true;
let runtimeDownloading = false;
let pendingLaunchVersion: string | null = null;

const storage = {
  get selected(): string {
    return window.localStorage.getItem("quartz.selectedVersion") ?? "";
  },
  set selected(v: string) {
    window.localStorage.setItem("quartz.selectedVersion", v);
  },
};

/* ---------- 头像 ---------- */
function avatarStyle(uuid: string): string {
  let h = 0;
  for (const c of uuid) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  const hue = h % 360;
  return `background: linear-gradient(135deg, hsl(${hue} 70% 62%), hsl(${(hue + 50) % 360} 70% 45%))`;
}
function avatarLetter(name: string): string {
  return (name[0] ?? "?").toUpperCase();
}

function renderAccountChips() {
  const name = currentAccount?.username ?? "未登录";
  const letter = currentAccount ? avatarLetter(currentAccount.username) : "?";
  const style = currentAccount ? avatarStyle(currentAccount.uuid) : "";
  for (const [avatarId, nameId] of [
    ["sidebar-avatar", "sidebar-account-name"],
    ["topbar-avatar", "topbar-account-name"],
  ] as const) {
    const avatar = el<HTMLSpanElement>(avatarId);
    avatar.textContent = letter;
    avatar.setAttribute("style", style);
    setText(nameId, name);
  }
}

/* ---------- 版本清单 ---------- */
async function loadVersions(showLoading = true) {
  const note = el<HTMLDivElement>("version-list-note");
  if (showLoading) note.textContent = "版本清单加载中…";
  try {
    const list = await invoke<VersionInfo[]>("list_versions");
    if (list.length === 0) throw new Error("版本清单为空");
    versions = list;
    renderVersionList();
    fillHomeSelect();
  } catch (e) {
    versions = null;
    note.textContent = `版本清单拉取失败：${String(e)}（点击下方按钮重试）`;
    fillHomeSelect();
  }
}

async function refreshInstalled() {
  try {
    const list = await invoke<string[]>("list_installed_versions");
    installed = new Set(list);
  } catch {
    installed = new Set();
  }
  updateInstallBadge();
  if (versions) renderVersionList();
}

/* 主页版本下拉 */
function fillHomeSelect() {
  const sel = el<HTMLSelectElement>("version-select");
  const prev = storage.selected;
  sel.innerHTML = "";
  if (!versions || versions.length === 0) {
    const opt = document.createElement("option");
    opt.value = "";
    opt.textContent = installed.size > 0 ? "官方清单未获取，可先用已安装版本" : "版本清单未获取，请前往「下载游戏」页";
    sel.appendChild(opt);
    for (const v of installed) {
      const o = document.createElement("option");
      o.value = v;
      o.textContent = `${v}（已安装）`;
      sel.appendChild(o);
    }
    sel.disabled = false;
    updateInstallBadge();
    return;
  }
  for (const kind of KIND_ORDER) {
    const list = versions.filter((v) => v.type === kind);
    if (list.length === 0) continue;
    const group = document.createElement("optgroup");
    group.label = KIND_LABELS[kind];
    for (const v of list) {
      const opt = document.createElement("option");
      opt.value = v.id;
      opt.textContent = v.id;
      group.appendChild(opt);
    }
    sel.appendChild(group);
  }
  sel.disabled = false;
  restoreSelected(sel, prev);
  updateInstallBadge();
}

function restoreSelected(sel: HTMLSelectElement, prev: string) {
  if (prev && versions?.some((v) => v.id === prev)) {
    sel.value = prev;
  } else if (prev && installed.has(prev)) {
    sel.value = prev;
  } else {
    sel.value = versions?.[0]?.id ?? "";
  }
  storage.selected = sel.value;
}

function updateInstallBadge() {
  const v = el<HTMLSelectElement>("version-select").value;
  const badge = el<HTMLSpanElement>("install-badge");
  if (!v) {
    badge.className = "badge badge-dim";
    badge.textContent = "未选择版本";
    return;
  }
  if (installed.has(v)) {
    badge.className = "badge badge-ok";
    badge.textContent = "已安装";
  } else {
    badge.className = "badge badge-warn";
    badge.textContent = "未安装，可在下载页获取";
  }
}

/* 下载页列表 */
function typeBadge(kind: string): HTMLElement {
  const b = document.createElement("span");
  const map: Record<string, string> = {
    release: "badge-ok",
    snapshot: "badge-warn",
    old_beta: "badge-purple",
    old_alpha: "badge-dim",
  };
  b.className = `badge ${map[kind] ?? "badge-dim"}`;
  b.textContent = KIND_LABELS[kind] ?? kind;
  return b;
}

function renderVersionList() {
  const wrap = el<HTMLDivElement>("version-list");
  wrap.innerHTML = "";
  const note = document.createElement("div");
  note.className = "list-note";
  note.id = "version-list-note";
  wrap.appendChild(note);

  if (!versions) {
    note.textContent = el<HTMLDivElement>("version-list-note").textContent || "版本清单未加载";
    return;
  }

  const text = searchText.trim().toLowerCase();
  const filtered = versions.filter(
    (v) => (kindFilter === "all" || v.type === kindFilter) && (!text || v.id.toLowerCase().includes(text)),
  );
  if (filtered.length === 0) {
    note.textContent = "没有匹配的版本";
    hide("load-more-btn");
    return;
  }

  shownCount = Math.min(shownCount, filtered.length);
  for (const v of filtered.slice(0, shownCount)) {
    wrap.appendChild(versionRow(v));
  }

  const more = el<HTMLButtonElement>("load-more-btn");
  if (shownCount < filtered.length) {
    more.textContent = `加载更多（${shownCount} / ${filtered.length}）`;
    show("load-more-btn");
  } else {
    hide("load-more-btn");
  }
}

function versionRow(v: VersionInfo): HTMLElement {
  const row = document.createElement("div");
  row.className = "v-row";

  const name = document.createElement("span");
  name.className = "v-name";
  name.textContent = v.id;
  row.appendChild(name);

  const badges = document.createElement("span");
  badges.className = "v-badges";
  badges.appendChild(typeBadge(v.type));
  if (installed.has(v.id)) {
    const ok = document.createElement("span");
    ok.className = "badge badge-ok";
    ok.textContent = "已安装";
    badges.appendChild(ok);
  }
  row.appendChild(badges);

  const actions = document.createElement("span");
  actions.className = "v-actions";
  const btn = document.createElement("button");
  btn.className = "btn btn-primary btn-sm";
  btn.textContent = installed.has(v.id) ? "重新下载" : "下载";
  btn.disabled = busy;
  btn.addEventListener("click", () => startDownload(v.id));
  actions.appendChild(btn);
  row.appendChild(actions);
  return row;
}

/* ---------- 下载 ---------- */
let listening = false;
async function ensureProgressListener() {
  if (listening) return;
  listening = true;
  await listen<DownloadProgress>("download://progress", (event) => onProgress(event.payload));
  await listen<DownloadProgress>("runtime://progress", (event) => onRuntimeProgress(event.payload));
  await listen<DownloadProgress>("renderer://progress", (event) => onRendererProgress(event.payload));
  await listen<DownloadProgress>("lwjgl://progress", (event) => onLwjglProgress(event.payload));
  await listen<LaunchLog>("launch://log", (event) => appendLog(event.payload.source, event.payload.line));
  await listen<LaunchExit>("launch://exit", (event) => {
    hide("stop-btn");
    if (event.payload.code === 0) {
      setLaunchBadge("已退出（0）", "dim");
      toast("游戏已正常退出", "ok");
    } else {
      setLaunchBadge(`已退出（${event.payload.code}）`, "err");
      toast(`游戏已退出（代码 ${event.payload.code}），详情见控制台`, "err");
    }
    appendLog("quartz", `[quartz] 进程退出，代码 ${event.payload.code}`);
  });
  await listen<MigrateMsg>("quartz://migrate", (event) => {
    const p = event.payload;
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
}

function progressEls(prefix: string) {
  return {
    fill: el<HTMLDivElement>(`${prefix}-progress-fill`),
    text: el<HTMLSpanElement>(`${prefix}-progress-text`),
    stage: el<HTMLSpanElement>(`${prefix}-progress-stage`),
    file: el<HTMLDivElement>(`${prefix}-progress-file`),
    status: el<HTMLDivElement>(`${prefix}-progress-status`),
  };
}

function onProgress(p: DownloadProgress) {
  for (const prefix of ["home", "dl"]) {
    const ui = progressEls(prefix);
    if (p.stage === "error") {
      alertIn(`${prefix}-progress-status`, "alert-error", `下载失败（可重试，已下载文件会自动跳过）：${p.error ?? "未知错误"}`);
    } else if (p.stage === "done") {
      ui.fill.style.width = "100%";
      ui.text.textContent = "100%";
      ui.stage.textContent = "完成";
      ui.file.textContent = "";
      alertIn(`${prefix}-progress-status`, "alert-ok", "下载完成！游戏文件已写入标准 .minecraft 目录。");
    } else {
      alertOut(`${prefix}-progress-status`);
      const percent = Math.max(0, Math.min(100, p.percent || 0));
      ui.fill.style.width = `${percent.toFixed(1)}%`;
      ui.text.textContent = `${percent.toFixed(1)}%（${bytes(p.current)} / ${bytes(p.total)}）`;
      ui.stage.textContent = STAGE_LABELS[p.stage] ?? p.stage;
      ui.file.textContent = p.file ?? "";
    }
  }

  if (p.stage === "done") {
    busy = false;
    refreshInstalled();
    toast("下载完成", "ok");
  } else if (p.stage === "error") {
    busy = false;
    toast("下载失败", "err");
  }
  setBusyUI(busy);
}

function setBusyUI(state: boolean) {
  for (const btn of document.querySelectorAll<HTMLButtonElement>(".v-actions .btn")) {
    btn.disabled = state;
  }
}

async function startDownload(versionId: string) {
  if (!versionId) {
    toast("请先选择版本", "err");
    return;
  }
  if (busy) {
    toast("已有下载任务进行中", "err");
    return;
  }
  busy = true;
  setBusyUI(true);
  storage.selected = versionId;
  el<HTMLSelectElement>("version-select").value = versionId;
  updateInstallBadge();

  for (const prefix of ["home", "dl"]) {
    const ui = progressEls(prefix);
    alertOut(`${prefix}-progress-status`);
    ui.fill.style.width = "0%";
    ui.text.textContent = "0%";
    ui.stage.textContent = "准备中";
    ui.file.textContent = "";
  }
  show("home-progress");
  show("dl-progress");
  await ensureProgressListener();
  try {
    await invoke("download_game", { versionId });
  } catch (e) {
    busy = false;
    setBusyUI(false);
    for (const prefix of ["home", "dl"]) {
      alertIn(`${prefix}-progress-status`, "alert-error", `下载启动失败：${String(e)}`);
    }
    toast("下载启动失败", "err");
  }
}

/* ---------- 账户 ---------- */
async function loadAccounts() {
  try {
    const entries = await invoke<AccountEntry[]>("list_accounts");
    renderAccounts(entries);
    const selected = entries.find((a) => a.selected);
    if (selected) currentAccount = selected;
    renderAccountChips();
  } catch (e) {
    toast(`账号列表加载失败：${String(e)}`, "err");
  }
}

function renderAccounts(entries: AccountEntry[]) {
  const wrap = el<HTMLDivElement>("account-list");
  wrap.innerHTML = "";
  if (entries.length === 0) {
    const note = document.createElement("div");
    note.className = "list-note card";
    note.textContent = "还没有账号，先添加一个离线账号";
    wrap.appendChild(note);
    return;
  }
  for (const a of entries) {
    wrap.appendChild(accountCard(a));
  }
}

function accountCard(a: AccountEntry): HTMLElement {
  const card = document.createElement("div");
  card.className = `account-card${a.selected ? " selected" : ""}`;

  const avatar = document.createElement("span");
  avatar.className = "avatar";
  avatar.textContent = avatarLetter(a.username);
  avatar.setAttribute("style", avatarStyle(a.uuid));
  card.appendChild(avatar);

  const info = document.createElement("div");
  info.className = "account-info";
  const nameRow = document.createElement("div");
  nameRow.className = "account-name";
  nameRow.textContent = a.username;
  if (a.selected) {
    const cur = document.createElement("span");
    cur.className = "badge badge-ok";
    cur.textContent = "当前";
    nameRow.appendChild(cur);
  }
  const uuidRow = document.createElement("div");
  uuidRow.className = "account-uuid";
  uuidRow.textContent = a.uuid;
  info.appendChild(nameRow);
  info.appendChild(uuidRow);
  card.appendChild(info);

  const actions = document.createElement("div");
  actions.className = "account-actions";
  if (!a.selected) {
    const switchBtn = document.createElement("button");
    switchBtn.className = "btn btn-sm";
    switchBtn.textContent = "切换";
    switchBtn.addEventListener("click", async () => {
      try {
        await invoke("select_account", { uuid: a.uuid });
        currentAccount = a;
        renderAccountChips();
        await loadAccounts();
        toast(`已切换到 ${a.username}`, "ok");
      } catch (e) {
        toast(`切换失败：${String(e)}`, "err");
      }
    });
    actions.appendChild(switchBtn);
  }
  const delBtn = document.createElement("button");
  delBtn.className = "btn btn-sm btn-danger";
  delBtn.textContent = "删除";
  delBtn.addEventListener("click", async () => {
    // 两段式确认，避免误删
    if (delBtn.dataset.confirm !== "1") {
      delBtn.dataset.confirm = "1";
      delBtn.textContent = "确认删除？";
      window.setTimeout(() => {
        delBtn.dataset.confirm = "";
        delBtn.textContent = "删除";
      }, 3000);
      return;
    }
    try {
      await invoke("remove_account", { uuid: a.uuid });
      toast(`已删除 ${a.username}`, "ok");
      await loadAccounts();
    } catch (e) {
      toast(`删除失败：${String(e)}`, "err");
    }
  });
  actions.appendChild(delBtn);
  card.appendChild(actions);
  return card;
}

async function addAccount() {
  const input = el<HTMLInputElement>("new-account-name");
  const name = input.value.trim();
  alertOut("account-error");
  if (!name) {
    alertIn("account-error", "alert-error", "请输入用户名");
    return;
  }
  const btn = el<HTMLButtonElement>("add-account-btn");
  btn.disabled = true;
  try {
    const acc = await invoke<Account>("offline_login", { name });
    input.value = "";
    toast(`已添加账号 ${acc.username}`, "ok");
    await loadAccounts();
  } catch (e) {
    alertIn("account-error", "alert-error", String(e));
  } finally {
    btn.disabled = false;
  }
}

/* ---------- 登录屏 ---------- */
async function doLogin() {
  const name = el<HTMLInputElement>("username").value.trim();
  alertOut("login-error");
  if (!name) {
    alertIn("login-error", "alert-error", "请输入用户名");
    return;
  }
  const btn = el<HTMLButtonElement>("login-btn");
  btn.disabled = true;
  try {
    currentAccount = await invoke<Account>("offline_login", { name });
    enterApp();
  } catch (e) {
    alertIn("login-error", "alert-error", String(e));
  } finally {
    btn.disabled = false;
  }
}

/* ---------- 存储与环境 ---------- */
async function loadEnvAndMeta() {
  try {
    const env = await invoke<EnvInfo>("get_env_info");
    setText("game-dir", env.gameDir);
    setText("settings-game-dir", env.gameDir);
    const modeText =
      env.storageMode === "public"
        ? "公共标准目录（文件管理器可见，卸载不丢）"
        : env.storageMode === "external"
          ? "外部私有目录（可在文件管理器查看）"
          : env.storageMode === "internal"
            ? "内部私有目录（应用专属）"
            : "桌面环境";
    setText("storage-mode", modeText);
    fullAccess = env.fullAccess;
    togglePermUI(env.fullAccess);
    for (const badgeId of ["storage-badge", "settings-storage-badge"]) {
      const badge = el<HTMLSpanElement>(badgeId);
      if (env.layoutOk) {
        badge.className = "badge badge-ok";
        badge.textContent = "可写";
      } else {
        badge.className = "badge badge-err";
        badge.textContent = "不可写";
      }
    }
    const layout = el<HTMLElement>("layout-status");
    layout.textContent = env.layoutOk ? "标准 .minecraft 目录已就绪" : `初始化失败：${env.layoutError ?? "未知"}`;
    if (!env.layoutOk) {
      show("storage-warn");
      el<HTMLElement>("storage-warn").textContent = `存储初始化失败：${env.layoutError ?? "未知错误"}。登录与下载功能可能不可用，请反馈完整报错。`;
    } else {
      hide("storage-warn");
    }
  } catch (e) {
    setText("game-dir", `获取失败：${String(e)}`);
  }
  try {
    const meta = await invoke<AppMeta>("app_meta");
    setText("about-name", meta.name);
    setText("about-version", `v${meta.version}`);
    setText("about-platform", `${meta.os} · ${meta.arch}`);
    setText("app-version", `v${meta.version}`);
  } catch {
    /* 关于信息获取失败不影响使用 */
  }
}

/* ---------- 启动控制台 ---------- */
const CONSOLE_MAX_LINES = 400;
function consoleLineClass(source: string): string {
  if (source === "stderr") return "console-line err";
  if (source === "quartz") return "console-line sys";
  return "console-line";
}
function appendLog(source: string, line: string) {
  const box = el<HTMLDivElement>("console-box");
  // 清掉占位行
  const first = box.firstElementChild;
  if (first && first.classList.contains("dim") && box.children.length === 1) first.remove();
  const node = document.createElement("div");
  node.className = consoleLineClass(source);
  node.textContent = line;
  box.appendChild(node);
  while (box.children.length > CONSOLE_MAX_LINES) box.removeChild(box.firstElementChild!);
  box.scrollTop = box.scrollHeight;
}
function setLaunchBadge(text: string, cls: "dim" | "ok" | "warn" | "err") {
  const badge = el<HTMLSpanElement>("launch-badge");
  badge.className = `badge badge-${cls}`;
  badge.textContent = text;
}
function clearConsole() {
  const box = el<HTMLDivElement>("console-box");
  box.innerHTML = '<div class="console-line dim">[quartz] 启动日志将显示在这里</div>';
}

/* ---------- 完整文件权限 ---------- */
function togglePermUI(granted: boolean) {
  for (const id of ["perm-banner", "login-perm", "settings-perm"]) {
    if (granted) hide(id);
    else show(id);
  }
  const state = el<HTMLSpanElement>("perm-state");
  state.textContent = granted ? "已授权（完整文件访问）" : "未授权";
  state.style.color = granted ? "var(--ok, #4ade80)" : "var(--warn, #fbbf24)";
}
async function refreshPermission() {
  let granted = true;
  try {
    granted = (await invoke<EnvInfo>("get_env_info")).fullAccess;
  } catch {
    return; // 桥接不可用时静默
  }
  const changed = granted !== fullAccess;
  fullAccess = granted;
  togglePermUI(granted);
  if (changed && granted) {
    // 授权成功：切换到公共标准目录 + 迁移旧数据
    toast("已获得完整文件权限，正在切换标准目录…", "ok");
    try {
      const diag = await invoke<EnvInfo>("reinit_storage");
      setText("game-dir", diag.gameDir);
      setText("settings-game-dir", diag.gameDir);
      appendLog("quartz", `[quartz] 存储已切换到：${diag.gameDir}`);
      await refreshInstalled();
    } catch (e) {
      toast(`存储切换失败：${String(e)}`, "err");
    }
  }
}
async function requestPermission() {
  try {
    await invoke("request_storage_permission");
    toast("Android 11+ 已打开「所有文件访问」设置页，请开启后返回应用；低版本请在弹窗中允许", "");
  } catch (e) {
    toast(`权限请求失败：${String(e)}`, "err");
  }
  // 授权后回焦时自动重检
  window.setTimeout(refreshPermission, 1200);
  window.setTimeout(refreshPermission, 3000);
}

/* ---------- Java 运行时状态 ---------- */
async function refreshRuntimeBadge() {
  const badge = el<HTMLSpanElement>("runtime-badge");
  const settingsRuntime = el<HTMLSpanElement>("settings-runtime");
  try {
    const st = await invoke<{ installed: boolean; version: string | null; path: string; specAvailable: boolean }>("get_runtime_status", { javaMajor: 17 });
    if (st.installed) {
      badge.className = "badge badge-ok";
      badge.textContent = `Java 运行时就绪（${st.version ?? "17"}）`;
      settingsRuntime.textContent = `就绪（${st.version ?? "17"}）`;
    } else if (st.specAvailable) {
      badge.className = "badge badge-warn";
      badge.textContent = "运行时未下载（启动时自动获取）";
      settingsRuntime.textContent = "未下载（启动时自动获取）";
    } else {
      badge.className = "badge badge-dim";
      badge.textContent = "暂无下载源";
      settingsRuntime.textContent = "暂无下载源";
    }
  } catch {
    badge.className = "badge badge-dim";
    badge.textContent = "运行时检测失败";
    settingsRuntime.textContent = "检测失败";
  }
}

function showRtProgress() {
  runtimeDownloading = true;
  show("rt-progress");
  const fill = el<HTMLDivElement>("rt-progress-fill");
  fill.style.width = "0%";
  setText("rt-progress-text", "0%");
  setText("rt-progress-stage", "准备中");
  setText("rt-progress-file", "");
}
function onRuntimeProgress(p: DownloadProgress) {
  if (!runtimeDownloading) return;
  if (p.stage === "error") {
    runtimeDownloading = false;
    hide("rt-progress");
    appendLog("quartz", `[quartz] 运行时下载失败：${p.error ?? "未知错误"}`);
    toast("运行时下载失败，可重试", "err");
    pendingLaunchVersion = null;
    return;
  }
  if (p.stage === "done") {
    runtimeDownloading = false;
    hide("rt-progress");
    appendLog("quartz", `[quartz] Java 运行时就绪：${p.file}`);
    toast("Java 运行时就绪", "ok");
    refreshRuntimeBadge();
    const v = pendingLaunchVersion;
    pendingLaunchVersion = null;
    if (v) doLaunch(v);
    return;
  }
  const fill = el<HTMLDivElement>("rt-progress-fill");
  setText("rt-progress-stage", p.stage === "runtime-download" ? "下载" : "解包");
  setText("rt-progress-file", p.file ?? "");
  if (p.total > 0) {
    fill.style.width = `${Math.min(100, p.percent).toFixed(1)}%`;
    setText("rt-progress-text", `${p.percent.toFixed(1)}%（${bytes(p.current)} / ${bytes(p.total)}）`);
  } else {
    fill.style.width = "100%";
    setText("rt-progress-text", String(p.current));
  }
}

/* ---------- 渲染层 + LWJGL 安卓包（全部在线下载，不随 APK 内置） ---------- */
interface RendererInfo {
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
let rendererDownloading = false;
let lwjglDownloading = false;

async function refreshRenderers() {
  const list = el<HTMLDivElement>("renderer-list");
  let infos: RendererInfo[] = [];
  try {
    infos = await invoke<RendererInfo[]>("list_renderers");
  } catch {
    return;
  }
  const active = infos.find((r) => r.active);
  const badge = el<HTMLSpanElement>("renderer-badge");
  badge.textContent = active ? active.name : "-";
  badge.className = active?.installed ? "badge badge-ok" : "badge badge-warn";
  list.innerHTML = "";
  for (const r of infos) {
    const row = document.createElement("div");
    row.className = "kv";
    const name = document.createElement("span");
    name.className = "kv-key";
    name.textContent = r.name;
    const val = document.createElement("span");
    val.className = "kv-val";
    val.textContent = `${r.note} · ${r.sizeHint}`;
    row.appendChild(name);
    row.appendChild(val);
    if (r.active) {
      const b = document.createElement("span");
      b.className = "badge badge-ok";
      b.textContent = "使用中";
      row.appendChild(b);
    } else if (r.installed) {
      const b = document.createElement("span");
      b.className = "badge badge-dim";
      b.textContent = "已安装";
      row.appendChild(b);
    } else if (!r.specAvailable) {
      const b = document.createElement("span");
      b.className = "badge badge-dim";
      b.textContent = "暂无源";
      row.appendChild(b);
    }
    if (r.specAvailable && !r.installed) {
      const btn = document.createElement("button");
      btn.className = "btn btn-sm btn-primary";
      btn.textContent = "下载";
      btn.addEventListener("click", async () => {
        if (rendererDownloading) {
          toast("已有渲染层下载任务", "");
          return;
        }
        rendererDownloading = true;
        show("renderer-progress");
        setText("renderer-progress-stage", "下载");
        setText("renderer-progress-text", "0%");
        try {
          await invoke("download_renderer", { id: r.id });
        } catch (e) {
          rendererDownloading = false;
          hide("renderer-progress");
          toast(String(e), "err");
        }
      });
      row.appendChild(btn);
    }
    if (r.installed && !r.active) {
      const btn = document.createElement("button");
      btn.className = "btn btn-sm btn-ghost";
      btn.textContent = "切换";
      btn.addEventListener("click", async () => {
        try {
          await invoke("set_active_renderer", { id: r.id });
          toast(`渲染层已切换为 ${r.name}`, "ok");
          await refreshRenderers();
        } catch (e) {
          toast(String(e), "err");
        }
      });
      row.appendChild(btn);
    }
    list.appendChild(row);
  }
}

function onRendererProgress(p: DownloadProgress) {
  if (!rendererDownloading) return;
  if (p.stage === "error") {
    rendererDownloading = false;
    hide("renderer-progress");
    appendLog("quartz", `[quartz] 渲染层下载失败：${p.error ?? "未知错误"}`);
    toast("渲染层下载失败，可重试", "err");
    return;
  }
  if (p.stage === "done") {
    rendererDownloading = false;
    hide("renderer-progress");
    appendLog("quartz", `[quartz] 渲染层就绪：${p.file}`);
    toast("渲染层就绪", "ok");
    refreshRenderers();
    return;
  }
  const fill = el<HTMLDivElement>("renderer-progress-fill");
  setText("renderer-progress-file", p.file ?? "");
  if (p.total > 0) {
    fill.style.width = `${Math.min(100, p.percent).toFixed(1)}%`;
    setText(
      "renderer-progress-text",
      `${p.percent.toFixed(1)}%（${bytes(p.current)} / ${bytes(p.total)}）`,
    );
  } else {
    fill.style.width = "100%";
    setText("renderer-progress-text", String(p.current));
  }
}

async function refreshLwjglState() {
  const state = el<HTMLSpanElement>("lwjgl-state");
  const btn = el<HTMLButtonElement>("lwjgl-dl-btn");
  try {
    const st = await invoke<{ installed: boolean; path: string }>("get_lwjgl_status");
    if (st.installed) {
      state.textContent = "已就绪";
      state.style.color = "var(--ok, #4ade80)";
      btn.classList.add("hidden");
    } else {
      state.textContent = "未下载（启动时自动获取）";
      state.style.color = "var(--warn, #fbbf24)";
      btn.classList.remove("hidden");
    }
  } catch {
    state.textContent = "检测失败";
  }
}

function onLwjglProgress(p: DownloadProgress) {
  if (p.stage === "error") {
    lwjglDownloading = false;
    hide("renderer-progress");
    toast(`LWJGL 下载失败：${p.error ?? ""}`, "err");
    return;
  }
  if (p.stage === "done") {
    lwjglDownloading = false;
    hide("renderer-progress");
    appendLog("quartz", `[quartz] LWJGL 安卓包就绪：${p.file}`);
    toast("LWJGL 安卓包就绪", "ok");
    refreshLwjglState();
    return;
  }
  if (!lwjglDownloading) return;
  show("renderer-progress");
  setText("renderer-progress-stage", "LWJGL");
  const fill = el<HTMLDivElement>("renderer-progress-fill");
  setText("renderer-progress-file", p.file ?? "");
  if (p.total > 0) {
    fill.style.width = `${Math.min(100, p.percent).toFixed(1)}%`;
    setText("renderer-progress-text", `${p.percent.toFixed(1)}%`);
  }
}

/* ---------- 启动游戏（M0 真实启动链路） ---------- */
async function doLaunch(versionId: string) {
  try {
    setLaunchBadge("启动中…", "warn");
    appendLog("quartz", `[quartz] 启动 Minecraft ${versionId}…`);
    await invoke("launch_game", { versionId });
    setLaunchBadge("运行中", "ok");
    show("stop-btn");
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
        hide("rt-progress");
        toast(String(e2), "err");
        setLaunchBadge("启动失败", "err");
      }
      return;
    }
    setLaunchBadge("启动失败", "err");
    hide("stop-btn");
    appendLog("quartz", `[quartz] 启动失败：${msg}`);
    toast(msg, "err");
  }
}

async function launch() {
  const versionId = el<HTMLSelectElement>("version-select").value;
  if (!versionId) {
    toast("请先选择版本", "err");
    return;
  }
  if (!installed.has(versionId)) {
    toast("该版本尚未下载，请先在「下载游戏」页获取", "err");
    return;
  }
  try {
    const pf = await invoke<Preflight>("launch_preflight", { versionId });
    if (!pf.accountOk) {
      toast("请先在「账户」页登录", "err");
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
    await doLaunch(versionId);
  } catch (e) {
    toast(String(e), "err");
  }
}

/* ---------- 页面切换 ---------- */
let activePage = "home";
function showPage(page: string) {
  activePage = page;
  for (const p of ["home", "download", "account", "settings"]) {
    const sec = el<HTMLElement>(`page-${p}`);
    sec.classList.toggle("hidden", p !== page);
  }
  for (const nav of [el<HTMLElement>("nav"), el<HTMLElement>("bottom-nav")]) {
    for (const item of nav.querySelectorAll<HTMLButtonElement>(".nav-item")) {
      item.classList.toggle("active", item.dataset.page === page);
    }
  }
}

/* ---------- 进入主界面 ---------- */
function enterApp() {
  hide("login-screen");
  show("main-screen");
  renderAccountChips();
  loadEnvAndMeta();
  loadAccounts();
  loadVersions();
  refreshInstalled();
  refreshRuntimeBadge();
  refreshPermission();
  refreshRenderers();
  refreshLwjglState();
}

/* ---------- 事件绑定 ---------- */
function bindEvents() {
  // 导航（侧栏 + 底栏共用 data-page）
  for (const nav of ["nav", "bottom-nav"]) {
    el<HTMLElement>(nav).addEventListener("click", (e) => {
      const target = (e.target as HTMLElement).closest<HTMLButtonElement>(".nav-item");
      if (target?.dataset.page) showPage(target.dataset.page);
    });
  }
  el<HTMLElement>("topbar-account").addEventListener("click", () => showPage("account"));

  // 登录
  el<HTMLButtonElement>("login-btn").addEventListener("click", doLogin);
  el<HTMLInputElement>("username").addEventListener("keydown", (e) => {
    if ((e as KeyboardEvent).key === "Enter") doLogin();
  });

  // 主页
  el<HTMLSelectElement>("version-select").addEventListener("change", (e) => {
    storage.selected = (e.target as HTMLSelectElement).value;
    updateInstallBadge();
  });
  el<HTMLButtonElement>("launch-btn").addEventListener("click", launch);

  // 下载页
  el<HTMLInputElement>("version-search").addEventListener("input", (e) => {
    searchText = (e.target as HTMLInputElement).value;
    shownCount = PAGE_SIZE;
    renderVersionList();
  });
  el<HTMLElement>("kind-chips").addEventListener("click", (e) => {
    const chip = (e.target as HTMLElement).closest<HTMLButtonElement>(".chip");
    if (!chip?.dataset.kind) return;
    kindFilter = chip.dataset.kind;
    shownCount = PAGE_SIZE;
    for (const c of el<HTMLElement>("kind-chips").querySelectorAll<HTMLButtonElement>(".chip")) {
      c.classList.toggle("active", c === chip);
    }
    renderVersionList();
  });
  el<HTMLButtonElement>("load-more-btn").addEventListener("click", () => {
    shownCount += PAGE_SIZE;
    renderVersionList();
  });

  // 账户页
  el<HTMLButtonElement>("add-account-btn").addEventListener("click", addAccount);
  el<HTMLInputElement>("new-account-name").addEventListener("keydown", (e) => {
    if ((e as KeyboardEvent).key === "Enter") addAccount();
  });

  // 完整文件权限（登录屏 / 主界面横幅 / 设置页三处入口）
  for (const id of ["login-perm-btn", "perm-banner-btn", "settings-perm-btn"]) {
    el<HTMLButtonElement>(id).addEventListener("click", requestPermission);
  }

  // 启动控制台
  el<HTMLButtonElement>("console-clear-btn").addEventListener("click", clearConsole);
  el<HTMLButtonElement>("lwjgl-dl-btn").addEventListener("click", async () => {
    if (lwjglDownloading) {
      toast("已有 LWJGL 下载任务", "");
      return;
    }
    lwjglDownloading = true;
    show("renderer-progress");
    setText("renderer-progress-stage", "LWJGL");
    setText("renderer-progress-text", "0%");
    try {
      await invoke("download_lwjgl");
    } catch (e) {
      lwjglDownloading = false;
      hide("renderer-progress");
      toast(String(e), "err");
    }
  });
  el<HTMLButtonElement>("stop-btn").addEventListener("click", async () => {
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
  });

  // 授权后回到应用时自动重检权限（覆盖从系统设置返回的场景）
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "visible") refreshPermission();
  });
  window.addEventListener("focus", refreshPermission);
}

/* ---------- 启动 ---------- */
async function boot() {
  bindEvents();
  await ensureProgressListener();
  await refreshPermission();
  try {
    const acc = await invoke<Account | null>("get_account");
    if (acc && acc.username) {
      currentAccount = acc;
      enterApp();
      return;
    }
  } catch {
    /* 未登录 */
  }
  el<HTMLInputElement>("username").focus();
}

boot();
