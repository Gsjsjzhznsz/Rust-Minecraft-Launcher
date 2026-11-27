#!/usr/bin/env python3
"""发布说明写回 GitHub Release body（tag 推送默认说明 → 正式版说明）。

用法：update_release_body.py <tag> <notes.md>
例：  update_release_body.py v0.2.21 /home/z/my-project/scripts/release_notes_v0.2.21.md
按 tag 反查 release id（发布流水线先建 release，这里再 PATCH body）。
"""
import json, sys, urllib.request

TOKEN = ""
for line in open("/home/z/my-project/.git/config", encoding="utf-8"):
    if "https://" in line and "@" in line:
        TOKEN = line.split("https://")[1].split("@")[0].strip()
REPO = "Gsjsjzhznsz/Rust-Minecraft-Launcher"
H = {"Authorization": f"token {TOKEN}", "Accept": "application/vnd.github+json"}

TAG = sys.argv[1] if len(sys.argv) > 1 else "v0.2.20"
NOTES = sys.argv[2] if len(sys.argv) > 2 else f"/home/z/my-project/scripts/release_notes_{TAG.lstrip('v')}.md"

body = open(NOTES, encoding="utf-8").read()
req = urllib.request.Request(
    f"https://api.github.com/repos/{REPO}/releases/tags/{TAG}", headers=H)
with urllib.request.urlopen(req) as r:
    rel = json.load(r)
req = urllib.request.Request(
    f"https://api.github.com/repos/{REPO}/releases/{rel['id']}",
    data=json.dumps({"body": body}).encode(),
    headers={**H, "Content-Type": "application/json"}, method="PATCH")
with urllib.request.urlopen(req) as r:
    d = json.load(r)
print("Release body 已更新：", d["html_url"], "长度", len(d["body"]))
