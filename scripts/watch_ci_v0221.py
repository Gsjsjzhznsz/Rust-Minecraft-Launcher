#!/usr/bin/env python3
"""盯 android.yml 的 v0.2.21 构建状态，直到结束或超时（本进程最长 ~9.5 分钟）。"""
import json, sys, time, urllib.request

TOKEN = ""
for line in open("/home/z/my-project/.git/config", encoding="utf-8"):
    if "https://" in line and "@" in line:
        TOKEN = line.split("https://")[1].split("@")[0].strip()
H = {"Authorization": f"token {TOKEN}", "Accept": "application/vnd.github+json"}
REPO = "Gsjsjzhznsz/Rust-Minecraft-Launcher"

def api(url):
    req = urllib.request.Request(url, headers=H)
    with urllib.request.urlopen(req) as r:
        return json.load(r)

deadline = time.time() + 9.4 * 60
last = None
while time.time() < deadline:
    try:
        runs = api(
            f"https://api.github.com/repos/{REPO}/actions/workflows/android.yml/runs?per_page=5"
        )["workflow_runs"]
    except Exception as e:
        print("api err:", e, flush=True)
        time.sleep(30)
        continue
    TAG = sys.argv[1] if len(sys.argv) > 1 else "v0.2.21"
    run = next((r for r in runs if r.get("head_branch") == TAG), None)
    if run is None:
        print("尚未看到 v0.2.21 的 run，稍候重试…", flush=True)
        time.sleep(30)
        continue
    cur = f"{run['status']} / {run.get('conclusion')}  started={run['created_at']}  id={run['id']}"
    if cur != last:
        print(cur, flush=True)
        last = cur
    if run["status"] == "completed":
        print("RESULT:", run.get("conclusion"), "run:", run["html_url"], flush=True)
        sys.exit(0 if run.get("conclusion") == "success" else 1)
    time.sleep(45)
print("TIMEOUT: 本轮盯梢到点，CI 仍在", last, flush=True)
