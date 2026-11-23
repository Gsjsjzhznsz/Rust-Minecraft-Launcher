#!/usr/bin/env python3
"""主机模拟启动 Minecraft 1.20.4 —— 复刻设备 Quartz 的完整启动参数。

复刻内容（与设备日志 launch-1795453179.log 一一对应）：
  - 9 个 JVM 参数（路径重映射到 sim 布局）
  - 22 个游戏参数（同 username/uuid/userType，路径重映射）
  - -cp 顺序：Mojang 库列表（设备用自研 lwjgl-glfw-classes.jar 替代 lwjgl 组，
    主机模拟用 Mojang 原版 lwjgl + linux natives，版本同源 3.3.1）
  - 版本 jar 在 classpath 末位

运行环境：Xvfb + Mesa llvmpipe 软件 GL（无需真实显卡/显示）。
用 gold 里程碑判定游戏走到了哪一步，等价于设备日志的「进度条」。

用法：
  python3 run_sim.py                 # 正常模拟启动
  python3 run_sim.py --timeout 240   # 自定义超时（秒）
  python3 run_sim.py --fake-crash 252   # 假游戏进程：打印横幅后以指定码退出
"""
import argparse
import json
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path("/home/z/my-project/sim")
MC = ROOT / "minecraft"
JAVA = "/usr/lib/jvm/java-21-openjdk-amd64/bin/java"

# 里程碑 -> （日志匹配片段，中文说明）。字符串对齐 1.20.4 真实输出。
MILESTONES = [
    ("Setting user", "主类入口：参数解析通过"),
    ("Bootstrap", "Bootstrap 阶段（注册表/数据修复器加载）"),
    ("Backend library: LWJGL", "LWJGL natives 装载成功"),
    ("Couldn't set icon", "GLFW 窗口已创建（图标缺失警告属正常路径）"),
    ("textures/atlas/blocks.png-atlas", "OpenGL 上下文正常（方块图集合成）"),
    ("textures/atlas/gui.png-atlas", "GUI 图集合成（渲染管线深处）"),
    ("SoundSystem", "声音引擎阶段（无声卡环境自动降级）"),
]
FAILURE_PATTERNS = [
    ("UnsatisfiedLinkError", "native 库缺失/错位（java.library.path 或 .so 依赖）"),
    ("NoClassDefFoundError", "classpath 缺 jar"),
    ("ClassNotFoundException", "classpath 缺类"),
    ("Exception in thread \"main\"", "主线程未捕获异常"),
    ("Could not initialize class", "类初始化失败"),
]


def build_cmd(fake_crash: int | None, timeout: int):
    summary = json.loads((ROOT / "summary.json").read_text())
    cp = (ROOT / "classpath.txt").read_text().strip().replace("\n", ":")
    natives = str(ROOT / "natives")
    tmp = str(ROOT / "tmp")
    Path(tmp).mkdir(exist_ok=True)
    game_dir = str(MC)

    jvm_opts = [
        "-Xmx1024M",
        f"-Djava.library.path={natives}",
        f"-Djna.tmpdir={tmp}",
        "-Dorg.lwjgl.system.SharedLibraryExtractPath=" + tmp,
        "-Dio.netty.native.workdir=" + tmp,
        "-Dminecraft.launcher.brand=Quartz",
        "-Dminecraft.launcher.version=0.1.0",
        f"-Djava.io.tmpdir={tmp}",
    ]
    game_args = [
        "--username", "yqo3",
        "--version", summary["version"],
        "--gameDir", game_dir,
        "--assetsDir", str(MC / "assets"),
        "--assetIndex", summary["asset_index"],
        "--uuid", "8953bc6e-f478-3814-995a-08dc673e7887",
        "--accessToken", "0",
        "--clientId", "",
        "--xuid", "",
        "--userType", "legacy",
        "--versionType", "Quartz",
    ]

    if fake_crash is not None:
        # 假游戏进程：复刻 :game 侧横幅后以指定码退出（用于验证监控/恢复链路）
        code = fake_crash
        script = (
            "import sys,time\n"
            "print('[stdout] [quartz] :game 进程 99999 启动（fake-crash 模式）', flush=True)\n"
            "print('[stdout] [quartz] 路径重注入完成（fake）', flush=True)\n"
            "print('[stdout] [quartz] Surface 就绪', flush=True)\n"
            "print('[stdout] [quartz] JVM 动态库装载完成', flush=True)\n"
            "time.sleep(1.5)\n"
            f"print('[stdout] [quartz] 模拟错误：JVM 初始化失败（fake）', flush=True)\n"
            f"sys.exit({code})\n"
        )
        return ["python3", "-c", script], jvm_opts, game_args, summary

    # 不用 xvfb-run（依赖缺失的 xauth）：直接起 Xvfb + DISPLAY（见 main）
    return (
        [JAVA, *jvm_opts, "-cp", cp, summary["main_class"], *game_args],
        jvm_opts,
        game_args,
        summary,
    )


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--timeout", type=int, default=300)
    ap.add_argument("--fake-crash", type=int, default=None)
    ap.add_argument("--log", default=str(ROOT / "run.log"))
    args = ap.parse_args()

    cmd, jvm_opts, game_args, summary = build_cmd(args.fake_crash, args.timeout)
    log_path = Path(args.log)
    print(f"=== Quartz 主机模拟启动 {summary['version']} ===", flush=True)
    print(f"JVM 参数 {len(jvm_opts)} 个 / 游戏参数 {len(game_args)} 个 / 库 jar "
          f"{summary['libraries']} 个", flush=True)
    print(f"命令行前 3 段：{cmd[:3]}", flush=True)

    env = dict(__import__("os").environ)
    env.update({
        "LIBGL_ALWAYS_SOFTWARE": "1",
        "GALLIUM_DRIVER": "llvmpipe",
        "MESA_GL_VERSION_OVERRIDE": "3.3",
        "DISPLAY": ":99",
    })
    xvfb = None
    if args.fake_crash is None:
        xvfb = subprocess.Popen(
            ["Xvfb", ":99", "-screen", "0", "1280x800x24", "-nolisten", "tcp"],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
        time.sleep(1.5)
    t0 = time.time()
    with open(log_path, "w") as lf:
        proc = subprocess.Popen(cmd, stdout=lf, stderr=subprocess.STDOUT,
                                env=env, start_new_session=True)
        deadline = t0 + args.timeout
        hit, fails, finished = set(), [], False
        while time.time() < deadline:
            if proc.poll() is not None:
                finished = True
                break
            time.sleep(1)
            try:
                text = log_path.read_text(errors="replace")
            except FileNotFoundError:
                continue
            for pat, desc in MILESTONES:
                if pat not in hit and pat in text:
                    hit.add(pat)
                    print(f"  [{time.time()-t0:6.1f}s] ★ {desc}", flush=True)
            for pat, desc in FAILURE_PATTERNS:
                if pat in text and (pat, desc) not in fails:
                    fails.append((pat, desc))
                    print(f"  [{time.time()-t0:6.1f}s] ✗ 触发失败模式：{desc}", flush=True)
        if not finished:
            print(f"[{args.timeout}s 超时，终止进程]", flush=True)
            subprocess.run(["pkill", "-f", "net.minecraft.client.main.Main"], check=False)
            proc.wait(timeout=20)
    if xvfb is not None:
        # 终止前截屏取证（标题画面若已渲染，可直接看到）
        try:
            subprocess.run(
                ["xwd", "-root", "-display", ":99", "-out", str(ROOT / "shot.xwd")],
                check=False, timeout=10)
            xwd = ROOT / "shot.xwd"
            if xwd.exists() and xwd.stat().st_size > 0:
                out = Path("/home/z/my-project/download/Quartz-sim-title.png")
                out.parent.mkdir(exist_ok=True)
                subprocess.run(
                    ["convert", str(xwd), "-resize", "640x400", str(out)],
                    check=False, timeout=20)
        except Exception:
            pass
        xvfb.terminate()
    code = proc.returncode
    text = log_path.read_text(errors="replace")
    print(f"--- 退出码 {code} / 耗时 {time.time()-t0:.1f}s / 日志 {log_path} ---", flush=True)

    print("\n=== 里程碑判定 ===")
    for pat, desc in MILESTONES:
        mark = "★" if pat in hit else (" " if pat not in text else "·")
        status = "达到" if pat in hit else ("日志有迹" if pat in text else "未到")
        print(f"  {mark} {status}  {desc}")
    if fails:
        print("\n=== 失败模式 ===")
        for pat, desc in fails:
            print(f"  ✗ {desc}（特征：{pat}）")
        tail = "\n".join(text.splitlines()[-25:])
        print("\n--- 日志尾部 ---")
        print(tail)
    if "fake" in str(args.fake_crash) or args.fake_crash is not None:
        print("\n（fake-crash 模式：用于验证启动器侧监控/退出上报链路）")
    print("\n=== 结论 ===")
    if args.fake_crash is not None:
        print(f"  假游戏进程以代码 {code} 退出 —— 监控方应在退出码一致处收到通知")
    elif len(hit) >= 5 and not finished:
        print("  全链路打通（classpath/参数/natives/GLFW/OpenGL/资源/声音降级），"
              "进程存活至超时 —— 等价于已到标题画面并持续运行")
    elif len(hit) >= 5:
        print("  全链路打通但进程自行退出 —— 检查日志尾部的退出原因")
    elif len(hit) >= 3:
        print("  走到渲染阶段后失败 —— 查看日志尾部")
    elif len(hit) >= 1:
        print("  JVM/主类/参数 OK，卡在 natives/GLFW 阶段 —— 查看日志尾部")
    else:
        print("  早期失败（JVM 参数/主类/classpath）—— 查看日志头部")
        print("\n".join(text.splitlines()[:30]))
    return code


if __name__ == "__main__":
    sys.exit(main())
