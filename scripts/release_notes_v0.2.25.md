# v0.2.25 zink 颜色畸变终局修复

## 现象

v0.2.24（TextureView）之后 zink 已能渲染出画面，但**整屏颜色畸变**（game-1795863899976 实机，Mali-G610 MC6）。

## 根因（日志实锤）

本日志给出铁证：`eglChooseConfig 阶梯 #0–#6 全部 rc=0 configs=0 err=0x3004（EGL_BAD_ATTRIBUTE）`。其中 **#0 属性档就是 ZL2 `gl_bridge.c` 的原版属性集**——同一份 `libEGL_mesa.so`（与 ZL2 2.8.2 官方 APK 逐字节相同）在 ZL2 里能命中，在我们这里全灭，唯一解释是**我们传给 EGL 的属性 id 本身是错的**。

逐项核对 `quartz-bridge` 自定义的 EGL 常量表，**8 个值全错**（疑似 GLX 系数值混入）：

| 常量 | 错值（旧） | 真值（Khronos EGL/egl.h） |
| --- | --- | --- |
| EGL_SURFACE_TYPE | 0x1233 | 0x3033 |
| EGL_BLUE_SIZE | 0x1222 | 0x3022 |
| EGL_GREEN_SIZE | 0x1223 | 0x3023 |
| EGL_RED_SIZE | 0x1224 | 0x3024 |
| EGL_ALPHA_SIZE | 0x1221 | 0x3021 |
| EGL_DEPTH_SIZE | 0x1225 | 0x3025 |
| EGL_RENDERABLE_TYPE | 0x1240 | 0x3040 |
| EGL_NONE | 0x3038 | 0x3053 |

Mesa 的 libEGL 严格校验属性 id，非法 id 直接 `EGL_BAD_ATTRIBUTE`。gl4es / MobileGlues 等**宽容实现无视属性 id**，把错值掩盖至今；zink（Mesa）是第一个全暴露的渲染层。

**连锁路径**：正经属性档全灭 → 落「垃圾属性档 #7」（attrib_list 以错值 0x3038 打头）拿到**任意 config** → 自造的桌面 GL 4.6/3.3/3.2 core 阶梯在该 config 上 `EGL_BAD_MATCH(0x3009)`×3 全败 → 回退 ES3 兜底（ctx_attrs 以 0x3038 收尾，Mesa 越界读栈属未定义行为，碰巧落成桌面 GL 4.6 compat，游戏才跑起来）→ 但 **config 是错配的**，kopper 按错配 config 选交换链格式 → **颜色畸变**。

## ZL2 蓝本（ZalithLauncher2 2.8.2 源码，main 分支原文）

- zink 渲染器 id = `opengles3_desktopgl_zink_kopper`（`KopperZinkRenderer`：`LIBGL_ES=3`、`libglxshim.so`、`libEGL_mesa.so` —— 与 v0.2.20 已对齐项一致）
- `egl_bridge.c pojavInit`：`ANativeWindow_setBuffersGeometry(R8G8B8X8_UNORM)`（= 我们已有的 `RGBX_8888` ✓）
- `gl_bridge.c gl_init_context`（zink 经 `opengles` 前缀进此桥）：
  - **单次** `eglChooseConfig`（BLUE8 GREEN8 RED8 ALPHA8 DEPTH24 SURFACE_TYPE WINDOW|PBUFFER RENDERABLE_TYPE ES2，顺序逐项）取**首个 config** + `EGL_NATIVE_VISUAL_ID`；
  - bind：**19 字符前缀 `"opengles3_desktopgl"`** → `EGL_OPENGL_API`；
  - 上下文属性**仅 `{EGL_CONTEXT_CLIENT_VERSION, LIBGL_ES}`**（strtol base 0、缺省 2、越界回 2），**无 profile mask、无版本阶梯**——Mesa 在 `EGL_OPENGL_API` 下把 0x3098 读作主版本、缺省 compat 档，配合 `MESA_GL_VERSION_OVERRIDE=4.6` = 桌面 GL 4.6 compat（v0.2.22 已实锤的 ZL2 实际产出）。

## 改动

1. **常量表改 Khronos 真值**（8 处）；`EGL_KHR_create_context` 的 MAJOR/MINOR/PROFILE 常量随自造 core 阶梯一并删除（ZL2 不使用）。
2. **`pojavCreateContext` zink 路径 = ZL2 `gl_bridge.c` 逐行 1:1**：19 字符前缀分流（与 `game_bridge` 注入的 ZL2 全同款渲染器值逐字符咬合）、单次 config（失败即 ZL2 同款放弃，**绝不落兜底 config**——错配 config 正是畸变根源）、bind `EGL_OPENGL_API`、单调用上下文；自造 4.6/3.3/3.2 core 阶梯删除。v9 ES 安全网保留在 ZL2 路径失败**之后**（仅日志区分，正常路径与 ZL2 逐行为一）。
3. **非本尊渲染层**（gl4es/mobileglues/angle/vgpu）阶梯语义不变：真值常量下只会更准（不再 BAD_ATTRIBUTE，落正配 config）。
4. **egl_sim 查询键同步真值**（0x3040/0x3033）。
5. **测试钉死**：
   - `quartz-bridge`：`egl_constants_match_khronos_spec`（常量真值逐项断言）+ `wrong_const_values_banned`（十进制字面量运行时格式化，防自引用）；
   - `quartz-core`：`zl2_gl_bridge_1_1_contract` 钉常量真值 / 19 字符前缀分流 / ZL2 属性序列（顺序+值）/ core 阶梯已删 / 历史错值禁回归 / sim 键同步。

quartz-core **76 测试绿**；quartz-bridge host-sim 2 测试绿；host clippy **84=84 零新增**。

## 制品与装载机制零改动

六件库、load_vulkan、LD_LIBRARY_PATH、TextureView 表面链全保留。老 APK 直接装新版即可（桥 .so 随 APK 更新）。
