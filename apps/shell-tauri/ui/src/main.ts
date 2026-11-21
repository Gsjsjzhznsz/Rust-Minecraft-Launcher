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
};
type AppMeta = { name: string; version: string; os: string; arch: string };

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
      env.storageMode === "external"
        ? "外部私有目录（可在文件管理器查看）"
        : env.storageMode === "internal"
          ? "内部私有目录（应用专属）"
          : "桌面环境";
    setText("storage-mode", modeText);
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

/* ---------- 启动游戏（占位） ---------- */
async function launch() {
  const versionId = el<HTMLSelectElement>("version-select").value;
  try {
    const msg = await invoke<string>("launch_game", { versionId: versionId || "" });
    toast(msg, "ok");
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
}

/* ---------- 启动 ---------- */
async function boot() {
  bindEvents();
  await ensureProgressListener();
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
