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
  freeBytes?: number | null;
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
type LaunchExit = { code: number; version: string; logTail?: string | null };
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
    const code = event.payload.code;
    if (code === 0) {
      setLaunchBadge("已退出（0）", "dim");
      toast("游戏已正常退出", "ok");
      hide("exit-detail");
    } else {
      setLaunchBadge(`已退出（${code}）`, "err");
      toast(`游戏已退出（代码 ${code}），详情见控制台`, "err");
      // v0.1.8：崩溃证据直达 UI —— 日志尾部（游戏进程 + 启动器）展示在控制台下方
      const detail = el<HTMLDivElement>("exit-detail");
      el<HTMLPreElement>("exit-detail-text").textContent =
        event.payload.logTail || "（无日志输出 —— 进程未产出任何内容）";
      detail.classList.remove("hidden");
    }
    appendLog("quartz", `[quartz] 进程退出，代码 ${code}`);
    // 控制台也追加尾部（与详情框互为备份）
    if (event.payload.logTail) {
      for (const line of event.payload.logTail.split("\n").slice(-40)) {
        appendLog("stderr", line);
      }
    }
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
    refreshAssetsStatus(); // 资产下载/修复完成后刷新维护页状态
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
function fmtBytes(n: number): string {
  const mb = Math.floor(n / (1024 * 1024));
  if (mb >= 1024) return (mb / 1024).toFixed(1) + "GB";
  return mb + "MB";
}

/* ---------- 启动诊断（上次崩溃 / 存储不足 / 最近启动日志尾部） ---------- */
async function showDiagnostics() {
  try {
    const d = await invoke<{
      lastCrash: string | null;
      lastLaunchTail: string | null;
      freeBytes: number | null;
    }>("get_diagnostics");
    const parts: string[] = [];
    if (d.lastCrash) parts.push("检测到上次异常退出（闪退证据已落盘）：\n" + d.lastCrash);
    if (typeof d.freeBytes === "number" && d.freeBytes < 500 * 1024 * 1024) {
      parts.push(
        "⚠ 设备存储空间不足（剩余 " +
          fmtBytes(d.freeBytes) +
          "）：下载会失败，应用也可能闪退，建议清理空间后重试"
      );
    }
    if (d.lastLaunchTail) parts.push("最近一次游戏启动日志尾部：\n" + d.lastLaunchTail);
    if (parts.length) {
      el<HTMLElement>("diag-banner-text").textContent = parts.join("\n\n————\n\n");
      show("diag-banner");
    }
    if (typeof d.freeBytes === "number") {
      setText("storage-free", fmtBytes(d.freeBytes) + " 可用");
    }
  } catch {
    /* 诊断失败不影响启动 */
  }
}

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
    if (typeof env.freeBytes === "number") {
      setText("storage-free", fmtBytes(env.freeBytes) + " 可用");
    }
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

/* ---------- 设置页扩展（v9）：内存 / 资产维护 / 日志查看 ---------- */
async function refreshMemorySetting() {
  try {
    const mb = await invoke<number>("get_memory_mb");
    el<HTMLInputElement>("memory-slider").value = String(mb);
    setText("memory-value", `${mb} MB`);
  } catch {
    /* 桌面/异常时静默 */
  }
}

function currentVersionId(): string {
  const sel = el<HTMLSelectElement>("version-select");
  return sel?.value || storage.selected || "";
}

async function refreshAssetsStatus() {
  const badge = el<HTMLSpanElement>("assets-badge");
  const vid = currentVersionId();
  if (!vid) {
    badge.className = "badge badge-dim";
    badge.textContent = "未选版本";
    setText("assets-index", "-");
    setText("assets-objects", "-");
    return;
  }
  try {
    const s = await invoke<{
      indexPresent: boolean;
      indexId: string;
      total: number;
      missing: number;
      missingBytes: number;
    }>("assets_status", { versionId: vid });
    if (!s.indexPresent) {
      badge.className = "badge badge-warn";
      badge.textContent = "无索引";
      setText("assets-index", "未安装（重新下载该版本可修复）");
      setText("assets-objects", "-");
      return;
    }
    setText("assets-index", `${s.indexId}（${s.total} 个对象）`);
    if (s.missing === 0) {
      badge.className = "badge badge-ok";
      badge.textContent = "完整";
      setText("assets-objects", "全部就绪");
    } else {
      badge.className = "badge badge-err";
      badge.textContent = `缺 ${s.missing}`;
      setText("assets-objects", `缺失 ${s.missing} 个（约 ${bytes(s.missingBytes)}）`);
    }
  } catch (e) {
    badge.className = "badge badge-dim";
    badge.textContent = "检测失败";
    setText("assets-index", String(e));
  }
}

async function saveMemorySetting() {
  const mb = Number(el<HTMLInputElement>("memory-slider").value);
  try {
    await invoke("set_memory_mb", { mb });
    setText("memory-value", `${mb} MB`);
    toast(`内存分配已保存为 ${mb} MB（下次启动生效）`, "ok");
  } catch (e) {
    toast(String(e), "err");
  }
}

async function repairAssetsManual() {
  const vid = currentVersionId();
  if (!vid) {
    toast("请先在主页选择版本", "err");
    return;
  }
  const btn = el<HTMLButtonElement>("assets-repair-btn");
  btn.disabled = true;
  show("home-progress");
  show("dl-progress");
  try {
    await ensureProgressListener();
    await invoke("repair_assets", { versionId: vid });
    toast("资产修复已开始，进度见主页/下载页", "");
  } catch (e) {
    btn.disabled = false;
    toast(String(e), "err");
  }
}

let logsCache: { name: string; size: number; modifiedMs: number }[] = [];
async function refreshLogList() {
  try {
    logsCache = await invoke<{ name: string; size: number; modifiedMs: number }[]>("list_logs");
  } catch {
    logsCache = [];
  }
  const sel = el<HTMLSelectElement>("log-select");
  sel.innerHTML = "";
  if (logsCache.length === 0) {
    const o = document.createElement("option");
    o.value = "";
    o.textContent = "暂无日志（先启动一次游戏）";
    sel.appendChild(o);
    return;
  }
  for (const l of logsCache) {
    const o = document.createElement("option");
    o.value = l.name;
    const d = new Date(l.modifiedMs);
    o.textContent = `${l.name}（${bytes(l.size)}，${d.toLocaleString()}）`;
    sel.appendChild(o);
  }
}

async function viewLogTail() {
  const name = el<HTMLSelectElement>("log-select").value;
  if (!name) {
    toast("暂无日志", "err");
    return;
  }
  try {
    const text = await invoke<string>("read_log_tail", { name });
    const pre = el<HTMLPreElement>("log-view");
    pre.textContent = text || "（空）";
    pre.classList.remove("hidden");
  } catch (e) {
    toast(String(e), "err");
  }
}

async function copyLogTail() {
  const name = el<HTMLSelectElement>("log-select").value;
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

/* ---------- 启动游戏（M0 真实启动链路） ---------- */
async function doLaunch(versionId: string) {
  try {
    setLaunchBadge("启动中…", "warn");
    if (selectedInstanceId) {
      appendLog("quartz", `[quartz] 实例隔离模式：${selectedInstanceId}`);
    }
    appendLog("quartz", `[quartz] 启动 Minecraft ${versionId}…`);
    await invoke("launch_game", { versionId, instanceId: selectedInstanceId || null });
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

/* ---------- 实例体系 / 内容管理 / 设置扩展 / 账号扩展（v0.2 对标 FCL/ZL2） ---------- */
type LoaderEntry = { version: string; stable: boolean };
type InstanceEntry = {
  id: string;
  name: string;
  mcVersion: string;
  loaderDesc: string;
  javaMajor: number;
  memoryMb: number;
  renderer: string | null;
  createdAt: string;
  selected: boolean;
};
type ContentEntry = {
  name: string;
  kind: string;
  sizeBytes: number;
  isDir: boolean;
  modifiedAt: number;
};
type SettingsInfo = {
  extraJvmArgs: string[];
  azureClientIdSet: boolean;
  azureClientId: string | null;
  selectedInstance: string | null;
};

let selectedInstanceId: string | null = null;
let loaderKind = "fabric";
let loaderVersions: LoaderEntry[] = [];
let contentKind = "mods";

async function loadSettings() {
  try {
    const s = await invoke<SettingsInfo>("get_settings");
    selectedInstanceId = s.selectedInstance ?? null;
    el<HTMLTextAreaElement>("jvm-args-input").value = (s.extraJvmArgs ?? []).join("\n");
    el<HTMLInputElement>("azure-client-id-input").value = s.azureClientId ?? "";
    setText(
      "azure-state",
      s.azureClientIdSet ? "已配置" : "未配置（微软登录不可用）"
    );
  } catch (e) {
    toast(String(e), "err");
  }
}

/* ---------- 实例 ---------- */
async function refreshInstances() {
  const box = el<HTMLElement>("instance-list");
  try {
    const list = await invoke<InstanceEntry[]>("list_instances");
    box.innerHTML = "";
    if (list.length === 0) {
      box.innerHTML = `<div class="list-note">还没有实例：在上方创建，或直接使用全局 .minecraft</div>`;
      return;
    }
    for (const inst of list) {
      const card = document.createElement("div");
      card.className = "account-card";
      const selCls = inst.selected ? " badge-ok" : " badge-dim";
      card.innerHTML = `
        <span class="avatar">${inst.name.slice(0, 1).toUpperCase()}</span>
        <div class="account-info">
          <div class="account-name">${escapeHtml(inst.name)} <span class="badge${selCls}">${inst.selected ? "当前" : ""}</span></div>
          <div class="account-sub">${escapeHtml(inst.mcVersion)} · ${escapeHtml(inst.loaderDesc)} · Java ${inst.javaMajor} · ${inst.memoryMb}MB</div>
        </div>
        <div class="account-actions">
          ${inst.selected ? "" : `<button class="btn btn-sm btn-primary" data-act="select" data-id="${inst.id}">选用</button>`}
          ${inst.selected ? "" : `<button class="btn btn-sm" data-act="unselect" data-id="${inst.id}">取消隔离</button>`}
          <button class="btn btn-sm btn-danger" data-act="delete" data-id="${inst.id}">删除</button>
        </div>`;
      box.appendChild(card);
    }
  } catch (e) {
    box.innerHTML = `<div class="list-note">实例加载失败：${escapeHtml(String(e))}</div>`;
  }
}

async function createInstance() {
  const name = el<HTMLInputElement>("new-instance-name").value.trim();
  const mc = el<HTMLInputElement>("new-instance-mc").value.trim();
  const loader = el<HTMLSelectElement>("new-instance-loader").value;
  let loaderVer = el<HTMLInputElement>("new-instance-loader-ver").value.trim();
  const err = el<HTMLElement>("instance-error");
  err.classList.add("hidden");
  if (!name || !mc) {
    err.textContent = "实例名称与 MC 版本均为必填";
    err.classList.remove("hidden");
    return;
  }
  try {
    if (loader && !loaderVer) {
      // 留空自动选最新稳定版
      const list = await invoke<LoaderEntry[]>("list_loader_versions", { loader, mc });
      const stable = list.find((v) => v.stable) ?? list[0];
      if (!stable) throw new Error(`没有可用的 ${loader} 版本`);
      loaderVer = stable.version;
    }
    await invoke("create_instance_cmd", {
      name,
      mcVersion: mc,
      loader: loader || null,
      loaderVersion: loaderVer || null,
    });
    toast("实例已创建", "ok");
    el<HTMLInputElement>("new-instance-name").value = "";
    el<HTMLInputElement>("new-instance-mc").value = "";
    el<HTMLInputElement>("new-instance-loader-ver").value = "";
    await refreshInstances();
    await loadSettings();
  } catch (e) {
    err.textContent = String(e);
    err.classList.remove("hidden");
  }
}

async function onInstanceAction(act: string, id: string) {
  try {
    if (act === "select") {
      await invoke("select_instance", { id });
      toast("已切换实例", "ok");
    } else if (act === "unselect") {
      await invoke("select_instance", { id: null });
      toast("已回到全局 .minecraft", "ok");
    } else if (act === "delete") {
      if (!confirm(`删除实例 ${id}（含游戏目录）？`)) return;
      await invoke("delete_instance_cmd", { id });
      toast("实例已删除", "ok");
    }
    await refreshInstances();
    await loadSettings();
  } catch (e) {
    toast(String(e), "err");
  }
}

/* ---------- 内容管理 ---------- */
async function refreshContent() {
  const box = el<HTMLElement>("content-list");
  setText(
    "content-scope-val",
    selectedInstanceId ? `实例 ${selectedInstanceId}` : "全局 .minecraft"
  );
  try {
    const list = await invoke<ContentEntry[]>("list_content_cmd", {
      kind: contentKind,
      instanceId: selectedInstanceId,
    });
    box.innerHTML = "";
    if (list.length === 0) {
      box.innerHTML = `<div class="list-note">暂无内容（${contentKind}）</div>`;
      return;
    }
    for (const item of list) {
      const card = document.createElement("div");
      card.className = "account-card";
      const d = item.modifiedAt ? new Date(item.modifiedAt * 1000).toLocaleDateString() : "";
      card.innerHTML = `
        <span class="avatar">${item.isDir ? "D" : "F"}</span>
        <div class="account-info">
          <div class="account-name">${escapeHtml(item.name)}</div>
          <div class="account-sub">${bytes(item.sizeBytes)}${d ? ` · ${d}` : ""}</div>
        </div>
        <div class="account-actions">
          <button class="btn btn-sm btn-danger" data-name="${escapeHtml(item.name)}">删除</button>
        </div>`;
      box.appendChild(card);
    }
  } catch (e) {
    box.innerHTML = `<div class="list-note">加载失败：${escapeHtml(String(e))}</div>`;
  }
}

async function deleteContent(name: string) {
  if (!confirm(`删除 ${name}？`)) return;
  try {
    await invoke("delete_content_cmd", {
      kind: contentKind,
      name,
      instanceId: selectedInstanceId,
    });
    toast("已删除", "ok");
    await refreshContent();
  } catch (e) {
    toast(String(e), "err");
  }
}

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

/* ---------- Modloader 安装面板 ---------- */
function renderLoaderVersions() {
  const box = el<HTMLElement>("loader-version-list");
  box.innerHTML = "";
  if (loaderVersions.length === 0) {
    box.innerHTML = `<div class="list-note">没有可用版本</div>`;
    return;
  }
  for (const v of loaderVersions.slice(0, 40)) {
    const row = document.createElement("div");
    row.className = "account-card";
    row.innerHTML = `
      <span class="avatar">${loaderKind.slice(0, 1).toUpperCase()}</span>
      <div class="account-info">
        <div class="account-name">${escapeHtml(v.version)}</div>
        <div class="account-sub">${v.stable ? "稳定版" : "测试版"}</div>
      </div>
      <div class="account-actions">
        <button class="btn btn-sm btn-primary" data-ver="${escapeHtml(v.version)}">安装</button>
      </div>`;
    box.appendChild(row);
  }
  if (loaderVersions.length > 40) {
    const note = document.createElement("div");
    note.className = "list-note";
    note.textContent = `仅显示前 40 条（共 ${loaderVersions.length} 条）`;
    box.appendChild(note);
  }
}

async function loadLoaderVersions() {
  const mc = el<HTMLInputElement>("loader-mc-version").value.trim();
  const err = el<HTMLElement>("loader-error");
  err.classList.add("hidden");
  if (!mc) {
    err.textContent = "请先输入 MC 版本";
    err.classList.remove("hidden");
    return;
  }
  const box = el<HTMLElement>("loader-version-list");
  box.innerHTML = `<div class="list-note">查询中…</div>`;
  try {
    loaderVersions = await invoke<LoaderEntry[]>("list_loader_versions", {
      loader: loaderKind,
      mc,
    });
    renderLoaderVersions();
  } catch (e) {
    loaderVersions = [];
    box.innerHTML = "";
    err.textContent = String(e);
    err.classList.remove("hidden");
  }
}

async function installLoader(version: string) {
  const mc = el<HTMLInputElement>("loader-mc-version").value.trim();
  if (!mc) {
    toast("请先输入 MC 版本", "err");
    return;
  }
  show("loader-progress");
  setText("loader-progress-stage", loaderKind);
  setText("loader-progress-text", "0%");
  el<HTMLElement>("loader-progress-status").classList.add("hidden");
  try {
    await invoke("install_loader_cmd", { mc, loader: loaderKind, loaderVersion: version });
    toast("安装任务已开始，进度实时更新", "ok");
  } catch (e) {
    hide("loader-progress");
    toast(String(e), "err");
  }
}

function onLoaderProgress(p: DownloadProgress) {
  show("loader-progress");
  setText("loader-progress-stage", p.stage);
  setText("loader-progress-text", `${Math.round(p.percent)}%`);
  el<HTMLElement>("loader-progress-fill").style.width = `${p.percent}%`;
  setText("loader-progress-file", p.file || "");
  if (p.error) {
    alertIn("loader-progress-status", "alert-error", p.error);
  } else if (p.done) {
    alertIn("loader-progress-status", "alert-ok", "安装完成，可在主页版本选择中找到新版本");
    installed.add(String(p.file || ""));
    void refreshInstalled();
  }
}

/* ---------- 微软设备码登录 ---------- */
function onMsPrompt(dc: { userCode: string; verificationUri: string; message?: string | null }) {
  const prompt = el<HTMLElement>("ms-prompt");
  prompt.innerHTML = `请在浏览器打开 <b>${escapeHtml(dc.verificationUri)}</b> 并输入代码 <b>${escapeHtml(dc.userCode)}</b>，等待授权完成…`;
  prompt.classList.remove("hidden");
  el<HTMLElement>("ms-result").classList.add("hidden");
}

function onMsResult(r: { ok: boolean; error?: string | null; account?: Account | null }) {
  const node = el<HTMLElement>("ms-result");
  if (r.ok && r.account) {
    node.className = "alert alert-ok";
    node.textContent = `微软登录成功：${r.account.username}`;
    void loadAccounts();
    void renderAccountChips();
  } else {
    node.className = "alert alert-error";
    node.textContent = `微软登录失败：${r.error ?? "未知错误"}`;
  }
  node.classList.remove("hidden");
}

/* ---------- authlib 外置登录 ---------- */
async function refreshAuthlib() {
  try {
    const s = await invoke<{ installed: boolean; version: string | null }>("authlib_status");
    setText("authlib-state", s.installed ? `已安装 ${s.version ?? ""}` : "未安装");
  } catch {
    setText("authlib-state", "-");
  }
}

async function downloadAuthlibAgent() {
  try {
    await invoke("download_authlib");
    toast("agent 下载任务已开始", "ok");
  } catch (e) {
    toast(String(e), "err");
  }
}

async function authlibLogin() {
  const server = el<HTMLInputElement>("authlib-server").value.trim();
  const user = el<HTMLInputElement>("authlib-user").value.trim();
  const pass = el<HTMLInputElement>("authlib-pass").value;
  const err = el<HTMLElement>("authlib-error");
  err.classList.add("hidden");
  if (!server || !user || !pass) {
    err.textContent = "服务器地址、账号、密码均为必填";
    err.classList.remove("hidden");
    return;
  }
  try {
    const acc = await invoke<Account>("authlib_login", { server, username: user, password: pass });
    toast(`外置登录成功：${acc.username}`, "ok");
    await loadAccounts();
    await renderAccountChips();
  } catch (e) {
    err.textContent = String(e);
    err.classList.remove("hidden");
  }
}

/* ---------- 设置保存 ---------- */
async function saveJvmArgs() {
  const text = el<HTMLTextAreaElement>("jvm-args-input").value;
  const args = text
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter((l) => l.length > 0);
  try {
    await invoke("set_jvm_args_cmd", { args });
    alertIn("settings-msg", "alert-ok", "JVM 参数已保存");
    toast("JVM 参数已保存", "ok");
  } catch (e) {
    alertIn("settings-msg", "alert-error", String(e));
  }
}

async function saveAzureClientId() {
  const v = el<HTMLInputElement>("azure-client-id-input").value.trim();
  try {
    await invoke("set_azure_client_id", { id: v || null });
    setText("azure-state", v ? "已配置" : "未配置（微软登录不可用）");
    toast("已保存 client_id", "ok");
  } catch (e) {
    toast(String(e), "err");
  }
}

/* ---------- 页面切换 ---------- */
let activePage = "home";
function showPage(page: string) {
  activePage = page;
  for (const p of ["home", "download", "instances", "content", "account", "settings"]) {
    const sec = el<HTMLElement>(`page-${p}`);
    sec.classList.toggle("hidden", p !== page);
  }
  for (const nav of [el<HTMLElement>("nav"), el<HTMLElement>("bottom-nav")]) {
    for (const item of nav.querySelectorAll<HTMLButtonElement>(".nav-item")) {
      item.classList.toggle("active", item.dataset.page === page);
    }
  }
  // 懒加载：切到对应页面时刷新数据
  if (page === "instances") void refreshInstances();
  if (page === "content") void refreshContent();
  if (page === "settings") void loadSettings();
  if (page === "account") void refreshAuthlib();
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
  refreshMemorySetting();
  refreshAssetsStatus();
  refreshLogList();
  loadSettings();
  refreshInstances();
  refreshAuthlib();
  void bindExtraEvents();
}

/* ---------- 事件绑定 ---------- */
function bindEvents() {
  // 启动诊断横幅关闭
  el<HTMLElement>("diag-banner-close").addEventListener("click", () => hide("diag-banner"));

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
  // 退出详情（v0.1.8）：复制 / 关闭
  el<HTMLButtonElement>("exit-detail-copy").addEventListener("click", async () => {
    try {
      await navigator.clipboard.writeText(el<HTMLPreElement>("exit-detail-text").textContent || "");
      toast("已复制退出详情", "ok");
    } catch (_) {
      toast("复制失败，请长按选择文本", "err");
    }
  });
  el<HTMLButtonElement>("exit-detail-close").addEventListener("click", () => hide("exit-detail"));
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

  // 设置页扩展（v9）：内存滑条 / 资产维护 / 日志查看
  el<HTMLInputElement>("memory-slider").addEventListener("input", (e) => {
    setText("memory-value", `${(e.target as HTMLInputElement).value} MB`);
  });
  el<HTMLButtonElement>("memory-save-btn").addEventListener("click", saveMemorySetting);
  el<HTMLButtonElement>("assets-repair-btn").addEventListener("click", repairAssetsManual);
  el<HTMLButtonElement>("log-view-btn").addEventListener("click", viewLogTail);
  el<HTMLButtonElement>("log-copy-btn").addEventListener("click", copyLogTail);
  el<HTMLSelectElement>("version-select").addEventListener("change", () => {
    refreshAssetsStatus();
  });
}

/** 新增事件绑定（v0.2 FCL/ZL2 对标）：实例 / 内容 / loader / 微软 / authlib / 设置 */
let extraEventsBound = false;
async function bindExtraEvents() {
  if (extraEventsBound) return;
  extraEventsBound = true;
  // 事件通道
  await listen<DownloadProgress>("loader://progress", (ev) => onLoaderProgress(ev.payload));
  await listen<{ userCode: string; verificationUri: string }>("ms://prompt", (ev) => onMsPrompt(ev.payload));
  await listen<{ ok: boolean; error?: string | null; account?: Account | null }>("ms://result", (ev) => onMsResult(ev.payload));
  await listen<{ done: boolean; version?: string | null; error?: string | null }>("authlib://progress", (ev) => {
    if (ev.payload.error) {
      toast(`agent 下载失败：${ev.payload.error}`, "err");
    } else {
      toast(`agent 已安装：${ev.payload.version ?? ""}`, "ok");
    }
    void refreshAuthlib();
  });
  await listen<DownloadProgress>("mrpack://progress", (ev) => {
    const p = ev.payload;
    if (p.error) {
      toast(`整合包导入失败：${p.error}`, "err");
      appendLog("quartz", `[quartz] 整合包导入失败：${p.error}`);
    } else if (p.done) {
      toast("整合包导入完成", "ok");
      appendLog("quartz", "[quartz] 整合包导入完成");
      void refreshInstances();
    } else {
      appendLog("quartz", `[quartz] 整合包导入 ${Math.round(p.percent)}%：${p.file || p.stage}`);
    }
  });

  // 下载页 loader 面板
  el<HTMLElement>("loader-kind-chips").addEventListener("click", (e) => {
    const chip = (e.target as HTMLElement).closest<HTMLButtonElement>(".chip");
    if (!chip?.dataset.loader) return;
    loaderKind = chip.dataset.loader;
    for (const c of el<HTMLElement>("loader-kind-chips").querySelectorAll<HTMLButtonElement>(".chip")) {
      c.classList.toggle("active", c === chip);
    }
    if (loaderVersions.length > 0) void loadLoaderVersions();
  });
  el<HTMLButtonElement>("loader-load-btn").addEventListener("click", loadLoaderVersions);
  el<HTMLElement>("loader-version-list").addEventListener("click", (e) => {
    const btn = (e.target as HTMLElement).closest<HTMLButtonElement>("button[data-ver]");
    if (btn?.dataset.ver) void installLoader(btn.dataset.ver);
  });

  // 实例页
  el<HTMLButtonElement>("create-instance-btn").addEventListener("click", createInstance);
  el<HTMLElement>("instance-list").addEventListener("click", (e) => {
    const btn = (e.target as HTMLElement).closest<HTMLButtonElement>("button[data-act]");
    if (btn?.dataset.act && btn?.dataset.id) void onInstanceAction(btn.dataset.act, btn.dataset.id);
  });

  // 内容页
  el<HTMLElement>("content-kind-chips").addEventListener("click", (e) => {
    const chip = (e.target as HTMLElement).closest<HTMLButtonElement>(".chip");
    if (!chip?.dataset.kind) return;
    contentKind = chip.dataset.kind;
    for (const c of el<HTMLElement>("content-kind-chips").querySelectorAll<HTMLButtonElement>(".chip")) {
      c.classList.toggle("active", c === chip);
    }
    void refreshContent();
  });
  el<HTMLElement>("content-list").addEventListener("click", (e) => {
    const btn = (e.target as HTMLElement).closest<HTMLButtonElement>("button[data-name]");
    if (btn?.dataset.name) void deleteContent(btn.dataset.name);
  });

  // 账户页：微软 / authlib
  el<HTMLButtonElement>("ms-login-btn").addEventListener("click", async () => {
    try {
      await invoke("ms_login");
    } catch (e) {
      toast(String(e), "err");
    }
  });
  el<HTMLButtonElement>("authlib-dl-btn").addEventListener("click", downloadAuthlibAgent);
  el<HTMLButtonElement>("authlib-login-btn").addEventListener("click", authlibLogin);

  // 设置页：JVM 参数 / azure client id
  el<HTMLButtonElement>("jvm-args-save-btn").addEventListener("click", saveJvmArgs);
  el<HTMLButtonElement>("azure-client-id-save-btn").addEventListener("click", saveAzureClientId);
}

/* ---------- 启动 ---------- */
async function boot() {
  bindEvents();
  await ensureProgressListener();
  showDiagnostics(); // 异步：诊断失败不阻塞启动
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
