# Quartz v0.2.9 —— 控件布局 / 虚拟鼠标 / 渲染层体验修复

实机反馈三连（截图 + 双份 launch/game 日志定位）：

## 1. 游戏内控件位置乱了（已修）
- **沉浸全屏**：游戏层现在隐藏状态栏/导航栏 + 刘海短边穿透。此前系统栏常驻游戏上层，
  控制层被 insets 打了 padding —— 顶部功能键被推下、左缘挖孔把系统键列右推
  （实测 ~150px）、热键横排在「padding 内容区」居中导致整体右偏。
- **W 键归位**：W 行改为 MATCH_PARENT + 行内居中，修复 W 左对齐在 A 正上方、
  十字散架的问题（实机截图确认），现在 W 居中在 S 正上方（FCL 同款）。
- 退出游戏自动恢复系统栏，启动器 UI 不受影响。

## 2. 虚拟鼠标下半屏/右缘不显示（已修）
- **根因**：触摸坐标是 GameView 全屏坐标，而光标视图挂在带 insets padding 的
  控制层内容区 —— FrameLayout 子视图位置 = padding + translation，指针被整体
  偏移，触摸越靠下/靠右越偏，到底部直接越界出屏（叠加默认 clipChildren 裁剪）。
- **修复**：moveCursor/居中都扣除容器 padding（坐标 1:1 对齐触摸点），
  控制层 clipChildren/clipToPadding=false，边缘触摸不再裁剪；
  insets 变化时光标自动重新居中（已跟随触摸则不扰动）。

## 3. 选 Zink 直接崩溃（体验改造）
- **预检增强**：EGL 预检失败前先探测 /dev/dri/renderD* 可访问性并打印
  （Mesa 建不了 GPU 设备 = zink 在该设备不可用的典型原因，日志直读根因）；
  默认显示初始化失败后按 EGL_MESA_platform_surfaceless 重试一次；
  zink 探测期注入 MESA_DEBUG/LIBGL_DEBUG，Mesa 失败原因行进游戏日志。
- **失败即时重试**：预检失败 → 自动回退 MobileGlues → 摘除旧 :game 绑定 →
  约 1 秒后用回退渲染层自动重跑本次启动（GameIpc 记住最近有效 Surface，
  新进程连接时补发）。不再「下次启动生效」，用户无需再点一次开始。
- 每次启动至多重试一次；窗口期内按停止仍按用户意图终止（137 通道）。

## 其他
- ci.yml push 分支修复（`ain]` → `[main]`），core 校验恢复自动触发。
- 版本号 0.2.8 → 0.2.9。
