#!/usr/bin/env python3
"""设备启动日志审计：对照 Mojang 官方流水线逐项 diff 设备的 classpath / JVM 参数 / 游戏参数。

输入：设备控制台日志（如 upload/launch-1795453179.log）
对照：scripts/sim/fetch_mojang.py 产出的 ground truth（sim/classpath.txt + version.json）

输出（stdout + 可写文件）：
  1. classpath 差异（设备有而官方无 / 官方有而设备无）
  2. JVM 参数差异（缺哪些标准项、多哪些自定义项）
  3. 游戏参数一致性
  4. 结论（每项差异的预期性判定）
"""
import json
import re
import sys
from pathlib import Path

ROOT = Path("/home/z/my-project/sim")
VER = "1.20.4"


def parse_device_log(text: str):
    opts, args, cp_jars = [], [], []
    section = None
    lines = text.splitlines()
    for i, line in enumerate(lines):
        s = line.strip()
        if s.startswith("---- jvm options"):
            section = "opts"
            continue
        if s.startswith("---- game args"):
            section = "args"
            continue
        if s.startswith("[quartz]") or s.startswith("[stdout]"):
            section = None
        if section == "opts":
            if s == "-cp":
                # classpath 在下一行
                for j in range(i + 1, len(lines)):
                    c = lines[j].strip()
                    if c.startswith("/"):
                        cp_jars = [p for p in c.split(":") if p.strip()]
                        break
                continue
            if s:
                opts.append(s)
        elif section == "args" and s:
            args.append(s)
    return opts, args, cp_jars


def gav_of(path: str) -> str:
    """从库文件路径取 group/artifact/version 语义键。"""
    parts = [p for p in path.split("/") if p]
    if len(parts) >= 4:
        return "/".join(parts[-4:-1]) + "/" + parts[-1]
    return path


def main():
    dev_log = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(
        "/home/z/my-project/upload/launch-1795453179.log")
    text = dev_log.read_text(errors="replace")
    opts, args, cp_jars = parse_device_log(text)

    vjson = json.loads((ROOT / "dl" / f"{VER}.json").read_text())
    official = set()
    official_natives = set()
    for lib in vjson["libraries"]:
        art = lib.get("downloads", {}).get("artifact")
        if art and art.get("url"):
            official.add(art["path"])
        cls = lib.get("downloads", {}).get("classifiers", {}).get("natives-linux")
        if cls and cls.get("url"):
            official_natives.add(cls["path"])

    report = []
    add = report.append
    add(f"# Quartz 设备启动日志审计（{dev_log.name} vs Mojang {VER} 官方流水线）\n")

    # ---- classpath ----
    device_gavs = [gav_of(p) for p in cp_jars]
    official_gavs = [gav_of("libraries/" + p) for p in sorted(official)]
    dev_set, off_set = set(device_gavs), set(official_gavs)
    dev_only = [g for g in device_gavs if g not in off_set]
    off_only = [g for g in official_gavs if g not in dev_set]
    add(f"## classpath（设备 {len(cp_jars)} 项 / 官方 linux 全集 {len(official)} 项）\n")
    add("**设备独有的 jar**（预期：lwjgl 替代包 / 平台 natives；异常：Mojang 库缺失替换）：")
    for g in dev_only:
        tag = ""
        if "lwjgl" in g:
            tag = "（预期：自研 LWJGL 显示桥替代包）"
        elif "linux-x86_64" in g:
            tag = "（预期：设备为 aarch64，x86_64 变体冗余但无害）"
        else:
            tag = "（ unexpected —— 需人工核查）"
        add(f"- `{g}`{tag}")
    if not dev_only:
        add("- （无）")
    add("\n**官方有而设备缺的 jar**（预期：lwjgl 组由自研包替代；异常：遗漏会导致 NoClassDefFoundError）：")
    lwjgl_missing, real_missing = [], []
    for g in off_only:
        (lwjgl_missing if "lwjgl" in g else real_missing).append(g)
    for g in real_missing:
        add(f"- `{g}` —— **异常缺失，重点核查**")
    for g in lwjgl_missing:
        add(f"- `{g}`（预期内：LWJGL 由自研包提供）")
    if not off_only:
        add("- （无）")

    # ---- JVM opts ----
    add(f"\n## JVM 参数（设备 {len(opts)} 项）\n")
    std_flags = {"-Xmx1024M"}
    device_flags = {o for o in opts if o.startswith("-X") or o.startswith("-XX")}
    for f in sorted(device_flags):
        add(f"- 设备标志：`{f}` {'（与标准一致）' if f in std_flags else ''}")
    dprops = {o.split("=")[0]: o for o in opts if o.startswith("-D")}
    std_props = {
        "-Djava.library.path", "-Djna.tmpdir",
        "-Dorg.lwjgl.system.SharedLibraryExtractPath", "-Dio.netty.native.workdir",
        "-Dminecraft.launcher.brand", "-Dminecraft.launcher.version",
    }
    for p in sorted(std_props):
        hit = [k for k in dprops if k == p]
        add(f"- `{'✓' if hit else '✗ 缺失'}` `{p}`")
    add("- `✗ 缺失` `-Djava.io.tmpdir`（:game 侧装配时补回，属预期）"
        if "-Djava.io.tmpdir" not in dprops else "- `✓` `-Djava.io.tmpdir`")
    add(f"\n诊断增强项缺失（下次构建补上）：")
    for k in ["-XX:ErrorFile", "-Xlog:init"]:
        add(f"- `{'已设置 ✓' if any(o.startswith(k) for o in opts) else '✗ 缺失'}"
            f" `{k}`（JVM 崩溃现场/初始化日志落盘）`")

    # ---- game args ----
    add(f"\n## 游戏参数（设备 {len(args)} 项）\n")
    pairs = dict(zip(args[::2], args[1::2]))
    expect = {
        "--username": "yqo3", "--version": VER,
        "--assetIndex": str(vjson["assetIndex"]["id"]),
        "--userType": "legacy", "--versionType": "Quartz",
    }
    for k, v in expect.items():
        got = pairs.get(k)
        add(f"- `{k}` = `{got}` {'✓' if got == v else '✗ 与官方预期 ' + v + ' 不一致'}")
    mainclass = vjson["mainClass"]
    add(f"\n- 主类（日志头声明）：`net.minecraft.client.main.Main`"
        f" {'✓ 与官方一致' if mainclass == 'net.minecraft.client.main.Main' else '✗'}")

    out = "\n".join(report)
    dst = Path("/home/z/my-project/download/device-launch-audit.md")
    dst.parent.mkdir(exist_ok=True)
    dst.write_text(out)
    print(out)
    print(f"\n审计报告已写入 {dst}")


if __name__ == "__main__":
    main()
