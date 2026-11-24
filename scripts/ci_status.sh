#!/bin/sh
# 查询 Android CI 状态
# 用法: sh scripts/ci_status.sh [run_id]
#   无参：列出最近 5 个 run；有参：单个 run 详情（含各步骤结论）
set -e
cd /home/z/my-project
TOKEN=$(git config --get remote.origin.url)
TOKEN="${TOKEN#https://}"; TOKEN="${TOKEN%%@*}"
REPO="Gsjsjzhznsz/Rust-Minecraft-Launcher"
export TOKEN REPO RID="${1:-}"
python3 <<'EOF'
import json, os, urllib.request
token, repo, run_id = os.environ["TOKEN"], os.environ["REPO"], os.environ["RID"]
H = {"Authorization": f"token {token}", "Accept": "application/vnd.github+json"}
def get(url):
    return json.load(urllib.request.urlopen(urllib.request.Request(url, headers=H)))
if not run_id:
    d = get(f"https://api.github.com/repos/{repo}/actions/workflows/android.yml/runs?per_page=5")
    for r in d.get("workflow_runs", []):
        print(r["id"], r["status"], r["conclusion"], r["head_sha"][:7], r["display_title"])
else:
    r = get(f"https://api.github.com/repos/{repo}/actions/runs/{run_id}")
    print(f"run {r['id']}: status={r['status']} conclusion={r['conclusion']} commit={r['head_sha'][:7]} {r['display_title']}")
    if r["status"] != "completed":
        jobs = get(f"https://api.github.com/repos/{repo}/actions/runs/{run_id}/jobs")
        for j in jobs.get("jobs", []):
            print(f"  job: {j['name']} [{j['status']}/{j['conclusion']}]")
            for s in j.get("steps", []):
                if s["status"] == "completed":
                    mark = "✘" if s["conclusion"] == "failure" else ("-" if s["conclusion"] == "skipped" else "✔")
                elif s["status"] == "in_progress":
                    mark = "▶"
                else:
                    mark = "·"
                print(f"    {mark} {s['name']}[{s['conclusion']}]")
EOF