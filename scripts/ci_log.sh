#!/bin/sh
# 拉取 CI 失败任务的日志尾部（排障用）
# 用法: sh scripts/ci_log.sh <run_id> [tail行数,默认300]
set -e
cd /home/z/my-project
TOKEN=$(git config --get remote.origin.url)
TOKEN="${TOKEN#https://}"; TOKEN="${TOKEN%%@*}"
REPO="Gsjsjzhznsz/Rust-Minecraft-Launcher"
export TOKEN REPO RID="${1:?usage: ci_log.sh <run_id> [tail行数]}" TAILN="${2:-300}"
python3 <<'EOF'
import json, os, urllib.request
token, repo, run_id, tailn = os.environ["TOKEN"], os.environ["REPO"], os.environ["RID"], int(os.environ["TAILN"])
H = {"Authorization": f"token {token}", "Accept": "application/vnd.github+json"}
def get(url):
    return json.load(urllib.request.urlopen(urllib.request.Request(url, headers=H)))
jobs = get(f"https://api.github.com/repos/{repo}/actions/runs/{run_id}/jobs")
for j in jobs.get("jobs", []):
    print(f"== job: {j['name']} [{j['conclusion']}] ==")
    for s in j.get("steps", []):
        print(f"   {s['name']}: {s['conclusion']}")
    if j["conclusion"] in ("failure", "cancelled"):
        url = f"https://api.github.com/repos/{repo}/actions/jobs/{j['id']}/logs"
        req = urllib.request.Request(url, headers=H)
        try:
            text = urllib.request.urlopen(req).read().decode("utf-8", "replace")
            lines = text.splitlines()
            print(f"---- log tail (total {len(lines)} lines, showing last {tailn}) ----")
            for ln in lines[-tailn:]:
                print(ln)
        except Exception as e:
            print(f"[log] 获取失败：{e}")
EOF