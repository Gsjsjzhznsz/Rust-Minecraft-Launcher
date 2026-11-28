# Quartz v0.2.27 — 「修复修好了但没生效」：内嵌显示桥刷新漏刷（size 判定 → 内容判定）

## 现象（game-1795870655964 实机，Mali-G57 MC2，v0.2.26 发布后 12 分钟）

- 六件预载全过、EGL 预检通过（zink，EGL 1.5）、JVM 创建成功、游戏走到建窗口：
  `[quartz-bridge] GLBridge: eglChooseConfig_p() failed: 0x3004`
  → LWJGL `GL.createCapabilities` 抛 `There is no OpenGL context current in the current thread` → 退出码 -1。
- 崩溃签名与 v0.2.25 的实机日志（game-1795866976312）**逐行相同** —— v0.2.26「EGL_NONE 修复」发布后，设备行为纹丝未动。

## 根因：修复在 APK 里，但从未落盘到设备（漏刷）

三重实锤链：

1. **v0.2.26 发布 APK 逐字节 diff（v0.2.25 ↔ v0.2.26 两个 Release APK 实物，315 个差异区域全部分类）**：
   修复字节 `0x53 → 0x38` 确实存在于壳层 `libquartz_shell_lib.so` 的 R 段内嵌
   `include_bytes!("../bridge/libpojavexec.so")` blob 中（rodata 常量池
   `0x3040,4,0x3053,0 → 0x3040,4,0x3038,0`），且为**等长单字节改动**；
   另有版本串 `0.2.25 → 0.2.26`。v0.2.26 APK sha256 与 Release API digest 吻合 —— 修复**确实随 APK 发布**。
2. **`ensure_bridge_lib` 以 `m.len() != BRIDGE_SO.len()` 判定新鲜度**：
   新旧桥等长 → `stale = false` → 跳过重写 → 设备 `jres/lwjgl3/natives/libpojavexec.so`
   继续保留 v0.2.25 旧桥（EGL_NONE=0x3053，属性表永不终止）→ Mesa 越界解析 →
   `eglChooseConfig` 返 `EGL_FALSE + EGL_BAD_ATTRIBUTE(0x3004)` → 无 GL 上下文崩溃。
   历史巧合：v0.2.24→v0.2.25 桥改动**变长**（阶梯删改），size 判定恰好有效，掩盖了缺陷；
   v0.2.26 是第一个**等长**修复，缺陷立即现形。
3. **Mesa 侧反汇编对照（ZL2 2.8.2 官方 libEGL_mesa.so 实物，capstone 全函数还原）**：
   `eglChooseConfig` → `_eglParseConfigAttribList`（37 项校验表 + 0x128 字节 attr→offset 跳转表 +
   `_eglValidateConfig` 位掩码 0x4F/0x667 + epilogue 三检查）逐项模拟结论：
   **0x3038 正确终结的合法属性表（BLUE8 GREEN8 RED8 ALPHA8 DEPTH24 SURFACE_TYPE W|P RENDERABLE ES2）在该实现上必然 PASS**。
   设备仍 0x3004 ⇒ 设备运行的桥不是修复版 —— 与漏刷结论互为印证。

## 改动

1. `game_bridge.rs` 新增 `write_if_changed(dst, bytes)`：**内容级比对**（逐字节）+
   临时文件 `rename` 原子替换（老 :game 进程若仍 mmap 旧 .so，就地覆写会破坏其映射；
   rename 只换目录项）+ 返回是否刷新。
2. 三处内嵌资产全部收编同一通道（同类隐患一并清零）：
   - 显示桥 `libpojavexec.so`（本次事故主通道）；
   - 退出捕获 `quartz-game-hook.jar`；
   - tagfix shim `libqztagfix.so`（保留 0o644 权限契约）。
3. 显示桥刷新/一致状态进 `[quartz]` 日志（「显示桥：内容变更，已刷新」/「显示桥：内容一致」）
   —— 实机日志从此自证桥版本，不再需要 APK diff 定位。
4. 契约测试 `embedded_assets_content_refresh_contract`（quartz-core 77 之一）：
   ①`write_if_changed` 内容比对 + rename 原子替换本体；
   ②桥/hook jar/tagfix shim 三处全部走该通道；
   ③size 判定写法（`m.len() != BRIDGE_SO.len()` 等）剥注释后**禁入**；
   ④显示桥刷新必须打日志。

## 版本

- tauri.conf 0.2.26 → 0.2.27。

## 不变（1:1 承诺）

- `bridge.rs`（EGL_NONE=0x3038 等 ZL2 gl_bridge.c 1:1 上下文链）、渲染层六件库制品、
  load_vulkan、TextureView 链、pack rev=3 全部零改动 —— 本版只修「修复如何落盘」的资产刷新通道。

## 验收

- 本地：`cargo fmt --check` 干净；`cargo clippy --workspace --all-targets -- -D warnings` 过；
  `cargo test --workspace` 77 绿（含新契约测试）。
- 实机判据（覆盖安装 v0.2.27 后首启）：
  1. 日志出现 `[quartz] 显示桥：内容变更，已刷新 …/jres/lwjgl3/natives/libpojavexec.so`（旧桥漏刷证明 + 本版必刷新）；
  2. zink 路径不再出现 `GLBridge: eglChooseConfig_p() failed: 0x3004`；
  3. 应出现 `EGLBridge: Binding to OpenGL` → 桌面 GL 4.6 compat 上下文 → 进入世界；
  4. 叠加 v0.2.24 TextureView（无 90° 反转）+ v0.2.26 正确 config（无颜色畸变）。

## 升级

- 覆盖安装 v0.2.27 APK 即可（无需重装渲染层包：v0.2.27 首启会因内容判定把桥强制刷到 0x3038 版）。
