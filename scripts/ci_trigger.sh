#!/bin/sh
# 触发 Android APK CI 构建（GitHub Actions workflow_dispatch）
# 用法: sh scripts/ci_trigger.sh <tag> [notes文件]
#   tag 为空字符串则只构建不发 Release；notes 文件为 Release 说明（Markdown）
set -e
cd /home/z/my-project
TAG="${1:-}"
NOTES_FILE="${2:-}"
TOKEN=$(git config --get remote.origin.url)
TOKEN="${TOKEN#https://}"; TOKEN="${TOKEN%%@*}"
REPO="Gsjsjzhznsz/Rust-Minecraft-Launcher"
export TAG NOTES_FILE TOKEN REPO
python3 <<'EOF'
import json, sys, os, urllib.request
tag, notes_file, token, repo = os.environ["TAG"], os.environ["NOTES_FILE"], os.environ["TOKEN"], os.environ["REPO"]
notes = ""
if notes_file:
    try:
        notes = open(notes_file, encoding="utf-8").read()
        print(f"[trigger] Release 说明 {len(notes)} 字符")
    except FileNotFoundError:
        print(f"[trigger] notes 文件不存在：{notes_file}，忽略")
inputs = {"release_tag": tag, "release_notes": notes}
data = json.dumps({"ref": "main", "inputs": inputs}).encode()
req = urllib.request.Request(
    f"https://api.github.com/repos/{repo}/actions/workflows/android.yml/dispatches",
    data=data,
    headers={"Authorization": f"token {token}", "Accept": "application/vnd.github+json"},
    method="POST",
)
try:
    r = urllib.request.urlopen(req)
    print(f"[trigger] HTTP {r.status} — 已触发（tag='{tag}'）")
except Exception as e:
    print(f"[trigger] 失败：{e}")
    sys.exit(1)
EOF