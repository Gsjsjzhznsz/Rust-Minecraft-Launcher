import { invoke } from "@tauri-apps/api/core";

/* ---------- Tauri 环境检测：纯浏览器预览时走本地桩数据 ---------- */
const inTauri = typeof (window as any).__TAURI_INTERNALS__ !== "undefined";

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (inTauri) {
    return invoke<T>(cmd, args);
  }
  return stub<T>(cmd);
}

async function stub<T>(cmd: string): Promise<T> {
  const fixture = await (await fetch("/src/stub.json")).json();
  return (fixture as any)[cmd] as T;
}

/* ---------- 界面装配 ---------- */
const $ = (id: string) => document.getElementById(id) as HTMLElement;

async function boot() {
  try {
    const meta = await call<{ name: string; version: string; os: string; arch: string }>(
      "app_meta"
    );
    $("os-arch").textContent = `${meta.os} · ${meta.arch}`;
    $("log").textContent = `[meta] ${meta.name} v${meta.version} (${meta.os}/${meta.arch})`;
  } catch (e) {
    $("log").textContent = `[meta] 调用失败：${e}`;
  }

  try {
    const list = await call<{
      latest: { release: string; snapshot: string };
      versions: { id: string; kind: string }[];
    }>("versions_offline");
    const sel = $("mc-version") as HTMLSelectElement;
    sel.innerHTML = "";
    for (const v of list.versions) {
      const opt = document.createElement("option");
      opt.value = v.id;
      opt.textContent = `${v.id}（${v.kind}）`;
      sel.appendChild(opt);
    }
    if (sel.options.length > 0) {
      sel.value = list.latest.release;
      sel.dispatchEvent(new Event("change"));
    }
  } catch (e) {
    $("log").textContent += `\n[versions] 调用失败：${e}`;
  }

  ($("mc-version") as HTMLSelectElement).addEventListener("change", async () => {
    const mc = ($("mc-version") as HTMLSelectElement).value;
    try {
      const advice = await call<{
        id: string | null;
        backend?: string;
        note?: string;
        reason?: string;
      }>("renderer_advice", { mc });
      $("renderer").textContent = advice.id ?? "不可用";
      $("renderer-note").textContent = advice.note ?? advice.reason ?? "";
    } catch (e) {
      $("renderer-note").textContent = `渲染路由失败：${e}`;
    }
  });

  $("play").addEventListener("click", async () => {
    const player = ($("player") as HTMLInputElement).value || "Steve";
    const memory = Number(($("memory") as HTMLInputElement).value) || 2048;
    const mc = ($("mc-version") as HTMLSelectElement).value;
    $("log").textContent = `[plan] 正在为 ${mc} 装配启动计划…`;
    try {
      const plan = await call<Record<string, unknown>>("plan_offline", {
        player,
        memoryMb: memory,
      });
      $("log").textContent = `[plan] ${mc} 启动计划：\n` + JSON.stringify(plan, null, 2);
    } catch (e) {
      $("log").textContent = `[plan] 失败：${e}`;
    }
  });
}

boot();
