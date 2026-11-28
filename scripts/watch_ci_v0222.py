#!/usr/bin/env python3
"""v0.2.22 CI 盯梢（watch_ci_v0221.py 参数化版，tag 作 argv）。"""
import json
import os
import subprocess
import sys
import time

TAG = sys.argv[1] if len(sys.argv) > 1 else "v0.2.22"
REPO = "Gsjsjzhznsz/Rust-Minecraft-Launcher"
TOKEN = subprocess.run(
    ["git", "config", "--get", "remote.origin.url"], capture_output=True, text=True
).stdout.strip()
TOKEN = TOKEN.split("https://")[1].split("@")[0] if "@github.com" in TOKEN else os.environ.get("GH_TOKEN", "")
POLL = 120
DEADLINE = time.time() + 100 * 60


def api(path, params=""):
    import urllib.request

    url = f"https://api.github.com{path}?per_page=20{params}"
    req = urllib.request.Request(url, headers={"Authorization": f"token {TOKEN}"})
    with urllib.request.urlopen(req, timeout=60) as r:
        return json.load(r)


def main():
    run_id = None
    for _ in range(30):
        runs = api(f"/repos/{REPO}/actions/runs", "&event=push")
        for r in runs.get("workflow_runs", []):
            if r["head_branch"] == TAG:
                run_id = r["id"]
                break
        if run_id:
            break
        time.sleep(20)
    if not run_id:
        print(f"ERR: 未找到 tag {TAG} 的 workflow run")
        sys.exit(1)
    print(f"盯梢 run {run_id}（tag {TAG}）")
    seen = set()
    while time.time() < DEADLINE:
        try:
            jobs = api(f"/repos/{REPO}/actions/runs/{run_id}/jobs")
            done = True
            for j in jobs.get("jobs", []):
                st = j["status"]
                if st == "completed":
                    key = (j["name"], j["conclusion"])
                    if key not in seen:
                        seen.add(key)
                        print(f"[完成] {j['name']}: {j['conclusion']}", flush=True)
                        for step in j.get("steps", []):
                            if step["conclusion"] not in ("success", "skipped", None):
                                print(f"    失败步骤: {step['name']}", flush=True)
                else:
                    done = False
                    live = [
                        s["name"]
                        for s in j.get("steps", [])
                        if s["status"] == "in_progress"
                    ]
                    if live:
                        print(f"[运行] {j['name']}: {live[-1]}", flush=True)
            rel_ok = False
            if done:
                try:
                    rel = api(f"/repos/{REPO}/releases/tags/{TAG}")
                    assets = [a["name"] for a in rel.get("assets", [])]
                    print(f"[Release] 资产: {assets}", flush=True)
                    rel_ok = bool(assets)
                except Exception as e:
                    print(f"[Release] 查询异常: {e}", flush=True)
            if done and rel_ok:
                print("CI_COMPLETE_OK", flush=True)
                return
            if done and not rel_ok and len(seen) > 0 and all(
                c == "success" for _, c in seen
            ):
                # jobs 全绿但 release 未就绪 —— 再等几轮
                pass
            if done and any(c == "failure" for _, c in seen):
                print("CI_FAILED", flush=True)
                sys.exit(2)
        except Exception as e:
            print(f"[poll 异常] {e}", flush=True)
        time.sleep(POLL)
    print("TIMEOUT", flush=True)
    sys.exit(3)


if __name__ == "__main__":
    main()
