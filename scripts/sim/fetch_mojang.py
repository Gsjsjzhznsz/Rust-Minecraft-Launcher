#!/usr/bin/env python3
"""拉取 Mojang 官方 1.20.4 流水线，在主机上复刻设备的 .minecraft 目录布局。

目的：把「游戏侧流水线是否正确」（classpath 完整性 / 启动参数 / 主类 /
natives 装载 / 资源索引）从 Android 设备上剥离出来，在 x86_64 主机上直接
迭代验证 —— 不用每次都装 APK 到手机看闪退。

产出布局（与设备 /storage/emulated/0/Quartz/files/.minecraft 完全同构）：
  sim/minecraft/versions/1.20.4/1.20.4.jar        客户端主 jar
  sim/minecraft/libraries/<maven 路径>.jar         全部 linux 依赖
  sim/minecraft/assets/indexes/12.json            资源索引
  sim/minecraft/assets/objects/<h2>/<hash>        资源对象（字体/语言/GUI 子集）
  sim/natives/*.so                                natives-linux jar 解出的 .so
  sim/classpath.txt                               classpath 顺序（Mojang 顺序）
  sim/version.json                                1.20.4 版本 JSON 原件
"""
import concurrent.futures
import json
import re
import sys
import urllib.request
import zipfile
from pathlib import Path

ROOT = Path("/home/z/my-project/sim")
MC = ROOT / "minecraft"
NATIVES = ROOT / "natives"
TMP = ROOT / "dl"
VER = "1.20.4"
MANIFEST = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json"
ASSET_SUBSET = re.compile(
    r"^minecraft/(font/|texts/|lang/|textures/gui/|textures/misc/|textures/title/)"
)


def fetch(url: str, dst: Path, desc: str = "") -> Path:
    dst.parent.mkdir(parents=True, exist_ok=True)
    if dst.exists() and dst.stat().st_size > 0:
        return dst
    req = urllib.request.Request(url, headers={"User-Agent": "QuartzSim/0.1"})
    try:
        with urllib.request.urlopen(req, timeout=180) as r, open(dst, "wb") as f:
            f.write(r.read())
    except Exception as e:
        print(f"  [FAIL] {desc or dst.name}: {e}", flush=True)
        raise
    return dst


def rules_allow_os_linux(rules) -> bool:
    """Mojang rules 求值：取最后一个匹配条目的 action（与官方启动器一致）。"""
    if not rules:
        return True
    verdict = True
    for rule in rules:
        allowed = rule.get("action") == "allow"
        os_name = (rule.get("os") or {}).get("name")
        if os_name is None:
            verdict = allowed  # 无 os 限定 -> 全平台
        elif os_name == "linux":
            verdict = allowed
        else:
            pass  # 其他平台条目不影响 linux 判定
    return verdict


def dl_many(items):
    """并发下载 [(url, dst, desc)]。"""
    with concurrent.futures.ThreadPoolExecutor(max_workers=12) as ex:
        futs = [ex.submit(fetch, u, d, s) for u, d, s in items]
        done = 0
        for f in concurrent.futures.as_completed(futs):
            done += 1
            if done % 20 == 0 or done == len(futs):
                print(f"  已完成 {done}/{len(futs)}", flush=True)
        for f in futs:  # 抛出失败
            f.result()


def main():
    print(f"[1/5] 版本清单 -> {VER} 版本 JSON", flush=True)
    manifest_path = fetch(MANIFEST, TMP / "manifest.json", "版本清单")
    manifest = json.loads(manifest_path.read_text())
    entry = next(v for v in manifest["versions"] if v["id"] == VER)
    vjson_path = fetch(entry["url"], TMP / f"{VER}.json", "版本 JSON")
    vjson = json.loads(vjson_path.read_text())
    (MC / "versions" / VER).mkdir(parents=True, exist_ok=True)
    (MC / "versions" / VER / f"{VER}.json").write_text(vjson_path.read_text())

    print(f"[2/5] 客户端主 jar（mainClass={vjson['mainClass']}）", flush=True)
    client = vjson["downloads"]["client"]
    fetch(client["url"], MC / "versions" / VER / f"{VER}.jar", "client.jar")

    print("[3/5] 库依赖（linux 规则全集）+ natives 解包", flush=True)
    cp_items, native_jars = [], []
    for lib in vjson["libraries"]:
        if not rules_allow_os_linux(lib.get("rules")):
            continue
        dl = lib.get("downloads", {})
        art = dl.get("artifact")
        if art and art.get("url"):
            dst = MC / "libraries" / art["path"]
            cp_items.append(dst)
            native_jars.append((art["url"], dst, lib.get("natives", {}).get("linux")))
        cls = dl.get("classifiers", {}).get("natives-linux")
        if cls and cls.get("url"):
            dst = MC / "libraries" / cls["path"]
            native_jars.append((cls["url"], dst, None))
    seen_urls = set()
    items = []
    for url, dst, _ in native_jars:
        if url in seen_urls:
            continue
        seen_urls.add(url)
        items.append((url, dst, dst.name))
    dl_many(items)
    NATIVES.mkdir(parents=True, exist_ok=True)
    so_count = 0
    for _, dst, _ in native_jars:
        if not dst.exists() or not dst.name.endswith(".jar"):
            continue
        with zipfile.ZipFile(dst) as z:
            for info in z.infolist():
                if info.filename.endswith(".so") and not info.filename.startswith("META-INF"):
                    out = NATIVES / Path(info.filename).name
                    if not out.exists():
                        out.write_bytes(z.read(info.filename))
                    so_count += 1
    print(f"  natives 解出 {so_count} 个 .so -> {NATIVES}", flush=True)

    print("[4/5] 资源索引 + 字体/语言/GUI 资源子集", flush=True)
    ai = vjson["assetIndex"]
    idx_path = fetch(ai["url"], MC / "assets" / "indexes" / f"{ai['id']}.json", "asset index")
    idx = json.loads(idx_path.read_text())
    res_base = "https://resources.download.minecraft.net"
    items = []
    for key, obj in idx.get("objects", {}).items():
        if not ASSET_SUBSET.match(key):
            continue
        h = obj["hash"]
        items.append((f"{res_base}/{h[:2]}/{h}", MC / "assets" / "objects" / h[:2] / h, key))
    print(f"  子集匹配 {len(items)} 个对象", flush=True)
    dl_many(items)

    print("[5/5] 写 classpath 清单", flush=True)
    # 与设备一致：版本 jar 最后；其余按 Mojang 顺序
    lines = [str(p) for p in cp_items if p.exists()]
    lines.append(str(MC / "versions" / VER / f"{VER}.jar"))
    (ROOT / "classpath.txt").write_text("\n".join(lines) + "\n")
    (ROOT / "summary.json").write_text(json.dumps({
        "version": VER,
        "main_class": vjson["mainClass"],
        "asset_index": ai["id"],
        "jvm_version_arg": vjson.get("javaVersion", {}).get("majorVersion"),
        "libraries": len(lines) - 1,
        "natives_dir": str(NATIVES),
        "minecraft_dir": str(MC),
    }, ensure_ascii=False, indent=2))
    print(f"完成：{len(lines)-1} 个库 jar / 主类 {vjson['mainClass']} / 资源索引 {ai['id']}", flush=True)


if __name__ == "__main__":
    sys.exit(main())
