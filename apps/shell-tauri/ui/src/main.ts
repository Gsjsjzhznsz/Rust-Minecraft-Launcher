import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

/* ---------- 类型 ---------- */
type Account = { kind: string; username: string; uuid: string };
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

/* ---------- 工具 ---------- */
const $ = (id: string) => document.getElementById(id) as HTMLElement;

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

function showError(el: HTMLElement, msg: string) {
  el.textContent = msg;
  el.classList.remove("hidden");
}
function clearError(el: HTMLElement) {
  el.textContent = "";
  el.classList.add("hidden");
}

/* ---------- 进度事件（Rust download_game -> download://progress） ---------- */
let listening = false;

async function ensureProgressListener() {
  if (listening) return;
  listening = true;
  await listen<DownloadProgress>("download://progress", (event) => onProgress(event.payload));
}

function onProgress(p: DownloadProgress) {
  const wrap = $("progress-wrap");
  const status = $("download-status");
  wrap.classList.remove("hidden");

  if (p.stage === "done") {
    ($("progress-fill") as HTMLElement).style.width = "100%";
    $("progress-text").textContent = "100%";
    $("progress-stage").textContent = "完成";
    $("progress-file").textContent = "";
    status.textContent = "下载完成！游戏文件已写入标准 .minecraft 目录。";
    status.className = "status ok";
    ($("download-btn") as HTMLButtonElement).disabled = false;
    return;
  }

  if (p.stage === "error") {
    status.textContent = `下载失败（可点击「下载」重试，已下载文件会自动跳过）：${p.error ?? "未知错误"}`;
    status.className = "status err";
    ($("download-btn") as HTMLButtonElement).disabled = false;
    return;
  }

  const percent = Math.max(0, Math.min(100, p.percent || 0));
  ($("progress-fill") as HTMLElement).style.width = `${percent.toFixed(1)}%`;
  $("progress-text").textContent = `${percent.toFixed(1)}%（${bytes(p.current)} / ${bytes(p.total)}）`;
  $("progress-stage").textContent = STAGE_LABELS[p.stage] ?? p.stage;
  $("progress-file").textContent = p.file ?? "";
  status.textContent = "正在下载…";
  status.className = "status";
  ($("download-btn") as HTMLButtonElement).disabled = true;
}

/* ---------- 版本列表（manifest；release 优先；失败红字 + 重试，禁止静默） ---------- */
const KIND_GROUPS: Record<string, string> = {
  release: "正式版",
  snapshot: "快照",
  old_beta: "旧测试版（Beta）",
  old_alpha: "旧经典版（Alpha）",
};

async function loadVersions() {
  const sel = $("version-select") as HTMLSelectElement;
  clearError($("version-error"));
  $("retry-versions-btn").classList.add("hidden");
  sel.innerHTML = '<option value="">版本列表加载中…</option>';
  sel.disabled = true;
  ($("download-btn") as HTMLButtonElement).disabled = true;
  try {
    const versions = await invoke<VersionInfo[]>("list_versions");
    if (versions.length === 0) throw new Error("版本清单为空");
    sel.innerHTML = "";
    for (const kind of ["release", "snapshot", "old_beta", "old_alpha"]) {
      const list = versions.filter((v) => v.type === kind);
      if (list.length === 0) continue;
      const group = document.createElement("optgroup");
      group.label = KIND_GROUPS[kind];
      for (const v of list) {
        const opt = document.createElement("option");
        opt.value = v.id;
        opt.textContent = kind === "release" ? v.id : `${v.id}（${KIND_GROUPS[kind]}）`;
        group.appendChild(opt);
      }
      sel.appendChild(group);
    }
    sel.disabled = false;
    ($("download-btn") as HTMLButtonElement).disabled = false;
  } catch (e) {
    sel.innerHTML = '<option value="">版本列表获取失败</option>';
    showError($("version-error"), `版本列表拉取失败：${String(e)}`);
    $("retry-versions-btn").classList.remove("hidden");
  }
}

/* ---------- 界面流转 ---------- */
function showMain(acc: Account | null) {
  $("login-screen").classList.add("hidden");
  $("main-screen").classList.remove("hidden");
  $("player-name").textContent = acc ? `玩家：${acc.username}` : "未登录";
  initMain();
}

async function initMain() {
  try {
    $("game-dir").textContent = await invoke<string>("get_game_dir");
  } catch (e) {
    $("game-dir").textContent = `获取失败：${String(e)}`;
  }
  loadVersions();
}

async function doLogin() {
  const name = ($("username") as HTMLInputElement).value.trim();
  clearError($("login-error"));
  if (!name) {
    showError($("login-error"), "请输入用户名");
    return;
  }
  ($("login-btn") as HTMLButtonElement).disabled = true;
  try {
    const acc = await invoke<Account>("offline_login", { name });
    showMain(acc);
  } catch (e) {
    showError($("login-error"), String(e));
  } finally {
    ($("login-btn") as HTMLButtonElement).disabled = false;
  }
}

async function startDownload() {
  const sel = $("version-select") as HTMLSelectElement;
  const versionId = sel.value;
  if (!versionId) {
    showError($("version-error"), "请先选择一个版本");
    return;
  }
  clearError($("version-error"));
  $("progress-wrap").classList.remove("hidden");
  ($("progress-fill") as HTMLElement).style.width = "0%";
  $("progress-text").textContent = "0%";
  $("progress-stage").textContent = "准备中";
  $("progress-file").textContent = "";
  const status = $("download-status");
  status.textContent = "正在开始下载…";
  status.className = "status";
  ($("download-btn") as HTMLButtonElement).disabled = true;
  await ensureProgressListener();
  try {
    await invoke("download_game", { versionId });
  } catch (e) {
    status.textContent = `下载启动失败：${String(e)}`;
    status.className = "status err";
    ($("download-btn") as HTMLButtonElement).disabled = false;
  }
}

async function launch() {
  const versionId = ($("version-select") as HTMLSelectElement).value;
  const status = $("download-status");
  try {
    const msg = await invoke<string>("launch_game", { versionId: versionId || "" });
    status.textContent = msg;
    status.className = "status ok";
  } catch (e) {
    status.textContent = String(e);
    status.className = "status err";
  }
}

/* ---------- 启动：已有账号直接进主界面（离线登录解耦） ---------- */
async function boot() {
  ($("login-btn") as HTMLButtonElement).addEventListener("click", doLogin);
  $("username").addEventListener("keydown", (e) => {
    if ((e as KeyboardEvent).key === "Enter") doLogin();
  });
  ($("logout-btn") as HTMLButtonElement).addEventListener("click", () => {
    $("main-screen").classList.add("hidden");
    $("login-screen").classList.remove("hidden");
  });
  ($("download-btn") as HTMLButtonElement).addEventListener("click", startDownload);
  $("retry-versions-btn").addEventListener("click", loadVersions);
  ($("launch-btn") as HTMLButtonElement).addEventListener("click", launch);

  await ensureProgressListener();
  try {
    const acc = await invoke<Account | null>("get_account");
    if (acc && acc.username) {
      showMain(acc);
      return;
    }
  } catch {
    /* 未登录 */
  }
  $("login-screen").classList.remove("hidden");
}

boot();
