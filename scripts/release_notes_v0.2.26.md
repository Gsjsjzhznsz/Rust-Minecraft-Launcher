# Quartz v0.2.26 — zink「无 GL 上下文崩溃」修复：v0.2.25 误改 EGL_NONE（0x3053 → 0x3038 恢复）

## 现象（game-1795866976312 实机，Mali-G57 MC2）

- 渲染层六件预载全过、EGL 预检通过（zink，EGL 1.5）、JVM 创建成功、游戏走到建窗口：
  `[quartz-bridge] GLBridge: eglChooseConfig_p() failed: 0x3004`
  → LWJGL `GL.createCapabilities` 抛 `There is no OpenGL context current in the current thread` → 退出码 -1。
- 时间线实锤：该日志拍摄于 v0.2.25 发布**之后**（19:56 vs 发布 19:47）——v0.2.25「颜色畸变终局修复」自身引入回归，从「颜色畸变」恶化成「连上下文都建不出来」。

## 根因（唯一错值：EGL_NONE）

- v0.2.25 把 7 个 0x12xx 系属性 id 全部改对（SURFACE_TYPE/BLUE/GREEN/RED/ALPHA/DEPTH/RENDERABLE_TYPE → 0x3033/0x3022/0x3023/0x3024/0x3021/0x3025/0x3040），但「EGL_NONE 0x3038≠0x3053」这条**改反了**：
  - **0x3038 才是 Khronos EGL_NONE**（GLFW `egl_context.h` 行级实锤 `#define EGL_NONE 0x3038`，全 GitHub 万级 vendored Khronos 头一致）；
  - **0x3053 是 EGL_MATCH_NATIVE_PIXMAP**。
- 后果：ZL2 属性档（BLUE8 GREEN8 RED8 ALPHA8 DEPTH24 SURFACE_TYPE W|P RENDERABLE ES2 尾接 EGL_NONE）末项变成 0x3053，属性表**永不终止** → Mesa 解析器越过 16 元数组越界续读 → 未知属性 → `eglChooseConfig` 返 `EGL_FALSE` + `EGL_BAD_ATTRIBUTE(0x3004)` → 按 ZL2 蓝本（gl_bridge.c）失败即放弃、绝不落兜底 config → 无上下文 → 崩溃。
- 错误码表同步勘误：EGL 错误枚举含 `EGL_BAD_ATTRIBUTE=0x3004`（BAD_CONFIG=0x3005、BAD_MATCH=0x3009、BAD_PARAMETER=0x300C、**BAD_SURFACE=0x300D**）——`EGL_BAD_SURFACE=0x300D` 本来就对（GLFW 行级实锤 `#define EGL_BAD_SURFACE 0x300d`），本版保持不动。这也统一解释了 v0.2.25 前后日志里反复出现的 `0x3004`（未知属性 id）与 profile 失败的 `0x3009`（BAD_MATCH）。
- 与前两段 bug 的关系：v0.2.25 前（0x12xx 属性 id 全错）= 错配 config → kopper 交换链格式错配 → **颜色畸变**；v0.2.25（EGL_NONE 错）= 属性表不终止 → **无上下文崩溃**。两段错值同源（常量表失准），本版后常量表与 ZL2 编译锚（Khronos egl.h）逐值一致，两条一并收口。

## 改动

1. `crates/quartz-bridge/src/bridge.rs`：`EGL_NONE 0x3053 → 0x3038`（**唯一代码值变更**）；错值史注释重写为两段实锤。
2. 测试解钉（上一版把错值钉死、还把正确值 0x3038 列入禁值清单——测试在为 bug 背书）：
   - bridge `egl_constants_match_khronos_spec`：EGL_NONE 断言改 0x3038；
   - bridge `wrong_const_values_banned`：禁值清单 `0x3038(12344)` → `0x3053(12371)`；
   - core `zl2_gl_bridge_1_1_contract`：①EGL_NONE 真值 needle 改 0x3038；⑤禁值清单 `0x3038` → `0x3053`。
3. 版本：tauri.conf 0.2.25 → 0.2.26。

## 不变（1:1 承诺）

- ZL2 `gl_bridge.c` 上下文链 1:1 逻辑零改动（19 字符前缀 `opengles3_desktopgl` 分流 / 单次 eglChooseConfig / 单调用上下文 / v9 ES 安全网仅在 ZL2 失败之后）。
- 制品层零改动：六件库与 ZL2 2.8.2 官方 APK 逐字节一致、load_vulkan 1:1、TextureView 链、pack rev=3（渲染层包无需重装）。
- 升级：直接覆盖安装 v0.2.26 APK（常量修在 APK 内 libpojavexec.so，与渲染层包无关）。

## 验收

- 本地：quartz-core / quartz-bridge 全部测试绿（含 `zl2_gl_bridge_1_1_contract`、`egl_constants_match_khronos_spec`、`wrong_const_values_banned`、`zink_libcutils_jni_libs_contract`、`game_surface_textureview_contract`），`cargo fmt --check` 干净，clippy 零新增。
- 实机判据：日志不再出现 `GLBridge: eglChooseConfig_p() failed: 0x3004`；zink 路径应出现 `EGLBridge: Binding to OpenGL` → 桌面 GL 4.6 compat 上下文 → 进入世界；叠加 v0.2.24 TextureView（无 90° 反转）与本版正确 config（无颜色畸变）。
