# v0.2.24 —— zink 90° 反转修复：游戏表面 SurfaceView → TextureView（ZL2 2.8.2 完全 1:1）

## 现状（launch-1795860301 复盘）

zink 本体已经通了：`load_vulkan（ZL2 1:1）`、`VULKAN_PTR=系统 loader（默认命名空间）`、
设备选中 `Mali-G610 MC6`、`渲染层 EGL 预检通过：zink（EGL 1.5）`、世界可进可玩 ——
v0.2.22/v0.2.23 的装载 1:1 收口生效。新问题只剩一条：**整帧画面 90° 反转**（方向横竖颠倒，
触控层仍在正确方向）。

## 根因（源码级实锤）

Mesa 23.0-kopper-android fork（ZL2 内置 Mesa 制品的源码分支，Swung0x48/mesa）
`src/gallium/drivers/zink/zink_kopper.c`：

```c
cswap->scci.preTransform = cdt->caps.currentTransform;
```

kopper 用**厂商 Vulkan 驱动上报的表面旋转变换**（横屏窗口落在竖屏面板上时 =
ROTATE_90/270）创建 swapchain，等于向驱动声明「应用会自己预旋转内容」；而
zink/Minecraft 根本不做 pre-rotation —— 未旋转的画面被按「已预旋转」标记送合成器，
DPU/SF 再转 90°，整帧反转。

**为什么 SurfaceView 中招、TextureView 不中招（ZL2 官方同款问题与解法）：**

- SurfaceView 的缓冲进**专用合成层**，直接吃上述预旋转标记 → 反转。
- TextureView 的缓冲经 **SurfaceTexture 在应用 UI 层内消费**，缓冲变换在视图渲染时
  相消，方向正确。
- ZL2 官方 issue **#1586**（Mali-G615 竖屏，与我们同族 Mali）/**#1587**（Adreno 750）
  就是同一个 bug；官方答复「**Disable surfaceview rendering**」。
  ZL2 2.4.10 的 `useSurfaceView` 默认 **false**（TextureView），2.8.2 起
  **SurfaceView 路径在源码里整体删除、TextureView 唯一**。
- MobileGlues/gl4es 等走系统 GLES 生产者（不打预旋转标记），此前一直正常。

## 改动（四件）

1. **GameView：SurfaceView → TextureView + SurfaceTextureListener**
   （ZL2 2.8.2 同款：`isOpaque=true`、`alpha=1.0f`）：
   - `onSurfaceTextureAvailable`：代次递增 + `GameIpc.sendSurface(Surface(st))` +
     初始尺寸补发（旧 SurfaceView 首挂 surfaceChanged 必带尺寸，TextureView 语义等价补齐）；
   - `onSurfaceTextureSizeChanged`：竖屏自愈（v0.2.10）+ `setDefaultBufferSize`
     （ZL2 同款）+ 尺寸转发；
   - `onSurfaceTextureDestroyed`：releaseSurface + scheduleSurfaceRestore（v0.2.14 自恢复链），
     返回 true 允许回收（re-attach 必再触发 available，与原 surfaceCreated 等价）；
   - 触控映射（v0.2.1-f 真实鼠标语义）原样保留，一行未动。
2. **MainActivity 装配流程不变**（TextureView re-attach 同样触发新 SurfaceTexture）；
   GameIpc/GameHost 注释同步到新回调名。
3. **契约测试** `game_surface_textureview_contract`：钉死 GameView 必须
   TextureView + SurfaceTextureListener、`Surface(SurfaceTexture)` 交接、
   `setDefaultBufferSize`、剥离注释后零 SurfaceView/SurfaceHolder 引用
   （防止回退；quartz-core **75 测试绿**）。
4. **制品与装载机制零改动**：六件库、load_vulkan、LD_LIBRARY_PATH、libcutils 注入
   全部保留 —— 本版只动呈现层的窗口表面类型。

## 与 ZL2 的一致性

- 游戏表面：TextureView 唯一路径 = ZL2 2.8.2 完全同款；
- 表面交接：`Surface(SurfaceTexture)` = ZL2 `onSurfaceTextureAvailable` 同款；
- 尺寸处理：`setDefaultBufferSize` = ZL2 同款；
- 制品/装载：v0.2.22/v0.2.23 已字节级与 ZL2 对齐，本版未动。

## 验收判据

zink 启动后：
- 世界/主菜单**方向正确**（不再 90° 反转）；
- 原有两行判据保持：`QUARTZ_VK_PROBE:OK:system ... gpu0=Mali-...` +
  `渲染层 EGL 预检通过：zink`；
- 触控方向与画面一致（触控层此前就正确，本版画面追平触控层方向）。

老 APK 注意：本版改的是 APK 内 Kotlin 层 —— **必须重装 v0.2.24 APK**（重下 zink 包无效）。
若反转仍在，回传一行启动日志即可定位。
