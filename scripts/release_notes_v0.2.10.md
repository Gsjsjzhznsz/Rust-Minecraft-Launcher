# Quartz v0.2.10 —— 分辨率/DPI 突变自愈 + 虚拟鼠标夹取（附 zink 结论）

实机日志（2026-11-25 launch-1795608201 / launch-1795609025）定位的两组问题：

## 1. 游戏中分辨率/DPI 突然变了（已修：方向自愈）
- **实锤**：12:03:41 窗口 2400x1080 → 12:03:51 变 **1080x2400（竖屏）** 持续 2s
  → 12:03:53 转回。MC 的 GUI 缩放由窗口像素尺寸自动推算，窗口一转竖屏就重算
  —— 这就是「分辨率和 DPI 突变 + 控件乱跳」的机制（触发方是 ROM：HyperOS
  在沉浸 + 刘海模式切换时的瞬时方向抖动，非启动器主动调用）。
- **自愈修复**：
  - `onWindowFocusChanged`：游戏层存在期间焦点恢复即重新锁定
    SENSOR_LANDSCAPE + 重新沉浸（ROM 偷偷改方向/亮系统栏时立刻纠正）；
  - `GameView.surfaceChanged`：高>宽（竖屏）时立即把 Activity 锁回横屏；
  - 方向偏离纠正时写 UI-ORIENTATION marker，下轮日志可直接定位触发路径。

## 2. 虚拟鼠标在下方屏幕不显示（已修：重映射 + 夹取）
- **根因**：光标已跟随触摸后若容器尺寸变化（瞬时旋转/insets 变化），指针
  留在旧坐标 —— 绝对位置超出新尺寸即画到屏外（下方/右缘）。
- **修复**：onSizeChanged 时已跟随光标按新旧尺寸比例重映射中心点，
  并夹取回容器可见范围 [0, W-cw]（v0.2.9 起光标允许画进 padding 区，
  此处只保证不出屏，不回弹触摸终点）。

## 3. zink 结论（本轮日志验证，代码无需再改）
- 两台设备/两次启动一致：**/dev/dri/renderD* 不存在（内核未暴露 DRM render
  node）→ Mesa/zink EGL 初始化必败（eglInitialize=0x3001）**。这是设备/内核
  层面的限制，不是崩溃，也不是启动器 bug；改内核（暴露 render node）或换
  turnip-kgsl 系 Mesa 构建才可能绕过。
- 启动器行为正确：预检拦截 → 干净退出（252）→ 自动回退 MobileGlues →
  回写 renderer.json 持久化 → 同次会话约 1s 后自动重跑成功进游戏。
  （MobileGlues EGL 1.4 预检通过，游戏全程正常。）
- 附加实机信号：LWJGL「Incompatible Java and native library versions」警告
  与 oshi cpuVendor NPE 均为无害噪音（前者是 LWJGL 3.3.3-snapshot 版本串
  误报，游戏 3.3.3 正常加载；后者 Android 无 /proc/cpuinfo vendor 字段）。
