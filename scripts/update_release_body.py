#!/usr/bin/env python3
"""v0.2.20：把正式发布说明写入 GitHub Release body（tag 推送默认说明 → 正式版）。"""
import json, os, urllib.request

TOKEN = ""
for line in open("/home/z/my-project/.git/config", encoding="utf-8"):
    if "https://" in line and "@" in line:
        TOKEN = line.split("https://")[1].split("@")[0].strip()
REPO = "Gsjsjzhznsz/Rust-Minecraft-Launcher"
H = {"Authorization": f"token {TOKEN}", "Accept": "application/vnd.github+json"}

body = open("/home/z/my-project/scripts/release_notes_v0.2.20.md", encoding="utf-8").read()
data = json.dumps({"body": body}).encode()
req = urllib.request.Request(
    f"https://api.github.com/repos/{REPO}/releases/460473565",
    data=data, headers={**H, "Content-Type": "application/json"}, method="PATCH")
with urllib.request.urlopen(req) as r:
    d = json.load(r)
print("Release body 已更新：", d["html_url"], "长度", len(d["body"]))
