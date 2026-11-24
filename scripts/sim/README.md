# Quartz 主机模拟启动环境

不用装 APK 到手机，直接在 x86_64 主机上验证「游戏侧流水线」：
Mojang 版本流水线下载、classpath 完整性、启动参数、主类、LWJGL natives、
GLFW 窗口、OpenGL 上下文、资源加载 —— 全链路可测，迭代周期从
「改代码 → 构建 APK → 传手机 → 看闪退」缩短为「改脚本 → 跑一次」。

## 组成

| 文件 | 作用 |
| --- | --- |
| `fetch_mojang.py` | 拉取 Mojang 官方 1.20.4 流水线（客户端 jar / 88 个库 / natives-linux 解包 / 资源子集），目录布局与设备 `.minecraft` 完全同构 |
| `run_sim.py` | 模拟启动器：复刻设备日志中的 9 个 JVM 参数 + 22 个游戏参数（路径重映射），在 Xvfb + Mesa llvmpipe 软件 GL 上运行，按里程碑判定走到了哪一步 |
| `diff_device_log.py` | 设备启动日志审计：把设备 classpath / JVM 参数 / 游戏参数与官方流水线逐项 diff，标注每项差异的预期性 |

产物目录 `sim/`（~190MB，已 gitignore）：`minecraft/`（与设备同构）、`natives/`、
`classpath.txt`、`summary.json`、`run.log`（模拟运行日志）。

## 用法

```sh
# 1) 拉取流水线（首次约 1-2 分钟）
python3 scripts/sim/fetch_mojang.py

# 2) 模拟启动（默认 300s 超时；软件渲染慢，验证标题画面建议 600s）
python3 scripts/sim/run_sim.py --timeout 600

# 3) 审计设备日志（对照官方流水线）
python3 scripts/sim/diff_device_log.py upload/launch-XXXX.log
```

## 里程碑判定

进程存活到超时 = 标题画面持续运行（正常游戏不会退出）。里程碑：

1. `Setting user` —— 主类入口、参数解析
2. `Bootstrap` —— 注册表/数据修复器
3. `Backend library: LWJGL` —— natives 装载
4. `Couldn't set icon` —— GLFW 窗口已创建（图标警告属正常）
5. `textures/atlas/blocks.png-atlas` —— OpenGL 上下文正常
6. `textures/atlas/gui.png-atlas` —— GUI 图集（渲染管线深处）
7. `SoundSystem` —— 声音引擎（无声卡环境自动降级）

## 已验证结论（2026-11-24）

- 1.20.4 官方 jar 集 + 设备同款参数在主机上 **7/7 里程碑全通，标题画面持续运行**。
- 设备 classpath 相比官方仅缺 LWJGL 组（自研显示桥替代包，预期内）与 mac 专属
  `java-objc-bridge`（无害）——组装逻辑无误。
- 因此设备端 `:game` 进程退出码 252 与游戏侧流水线无关，锁定在
  **该进程内 `JNI_CreateJavaVM` 失败**；v0.1.9 已在 `game_bridge.rs` 加
  rc 符号化 + 内存态势诊断 + 最小参数集自动重试，并修复退出日志 tail 竞态。

## 边界（模拟不了的部分）

Android bionic 下的 JVM 构建行为、ART 与 OpenJDK 并存、Surface/ANativeWindow、
渲染层 GL 转译（gl4es 等）——这些只能在设备上验证；模拟环境的价值是把
「游戏侧」与「JVM 宿主侧」彻底解耦，出问题时一眼定位是哪一半。
