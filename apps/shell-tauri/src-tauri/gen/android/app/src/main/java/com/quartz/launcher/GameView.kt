package com.quartz.launcher

import android.annotation.SuppressLint
import android.content.Context
import android.view.MotionEvent
import android.view.Surface
import android.view.SurfaceHolder
import android.view.SurfaceView
import kotlin.math.abs

/**
 * 游戏 Surface 视图（v0.1.5 游戏显示接入，v0.1.7 改走 :game 进程 IPC）：
 * - Surface/尺寸事件经 GameIpc（Messenger）转发给 ：game 进程的 GameService，
 *   由游戏进程内 QuartzBridge（ANativeWindow_fromSurface）接管；
 * - 触摸 -> 鼠标映射（v0.2.1-f 真实鼠标语义，与 FCL 对齐）：
 *   · 未 grab（菜单/UI）：触摸即指针（光标经 GameControls 可视化），
 *     按下=左键按下（支持按住拖拽：滚动条/拖物品），抬起=释放；
 *     长按（>=600ms 未移动）= 右键（左键先释放），抬起时释放右键；
 *     第二指上下滑动 = 滚轮；
 *   · grab（游戏内）：拖动=视角（虚拟光标累加增量上报），
 *     轻点=左键单击（攻击/交互），第二指=右键。
 * - 物理键盘（含蓝牙键鼠）由 Activity.dispatchKeyEvent 统一转发。
 */
@SuppressLint("ViewConstructor")
class GameView(context: Context) : SurfaceView(context, null, 0), SurfaceHolder.Callback {

    /** 控制层引用（光标可视化/灵敏度面板由其承载；MainActivity 装配时注入）。 */
    var controls: GameControls? = null

    // 虚拟光标（grab 模式下的绝对位置；游戏经光标回调取增量）
    private var virtualX = 0.0
    private var virtualY = 0.0
    // 未 grab 模式的点击锚点
    private var downX = 0f
    private var downY = 0f
    private var moved = false
    private var leftDown = false
    // 未 grab 长按右键
    private var longPressFired = false
    private val longPressRunnable = Runnable {
        if (GameHost.grabbed || leftDown == false) return@Runnable
        if (abs(lastX - downX) > 24 || abs(lastY - downY) > 24) return@Runnable
        longPressFired = true
        GameIpc.mouse(0, 0, 0)          // 先释放左键
        GameIpc.mouse(1, 1, 0)          // 右键按下（按住）
    }
    private var lastX = 0f
    private var lastY = 0f
    // 未 grab 双指滚轮
    private var secondPointerId = -1
    private var secondAnchorY = 0f
    // grab 模式
    private var anchorX = 0f
    private var anchorY = 0f
    private var rightHeld = false
    private var leftClickArmed = false
    /** 触摸 -> 视角灵敏度（px 增量放大系数）。 */
    var lookSensitivity = 1.0f

    init {
        holder.addCallback(this)
        setFocusable(true)
        setFocusableInTouchMode(true)
    }

    override fun surfaceCreated(holder: SurfaceHolder) {
        // v0.2.14：代次递增（取消未决的 surface 恢复计时）+ 交接新 Surface
        GameHost.surfaceSeq++
        val surface: Surface = holder.surface
        GameIpc.sendSurface(surface)
    }

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        // v0.2.10 自愈：游戏层存活期间 surface 变成竖屏（高>宽）说明 ROM 把窗口
        // 瞬时转了方向 —— 立即把 Activity 锁回横屏，避免 MC 收到竖屏尺寸重算
        // GUI 缩放（分辨率/DPI 突变、控件错位、虚拟光标出界）。
        if (height > width) {
            (context as? MainActivity)?.ensureGameLandscape("surfaceChanged ${width}x${height}")
        }
        GameIpc.size(width, height)
    }

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        GameIpc.releaseSurface()
        // v0.2.14：游戏会话进行中丶Surface 被壳层销毁（Activity 重建/ROM 布局抖动）
        // —— 2.5s 内无新 Surface 则由 MainActivity 强制重挂游戏层，
        // :game 侧（v0.2.14 桥修复）会安全切 pbuffer 丢帧等待换绑，不再崩溃。
        if (GameHost.gameActive) {
            (context as? MainActivity)?.scheduleSurfaceRestore(GameHost.surfaceSeq)
        }
    }

    /** 重置虚拟光标（窗口尺寸变化后由 Activity 调用）。 */
    fun resetCursor() {
        virtualX = 0.0
        virtualY = 0.0
    }

    private fun cancelLongPressTimer() {
        removeCallbacks(longPressRunnable)
    }

    @SuppressLint("ClickableViewAccessibility")
    override fun onTouchEvent(event: MotionEvent): Boolean {
        val grabbed = GameHost.grabbed
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                lastX = event.x
                lastY = event.y
                downX = event.x
                downY = event.y
                moved = false
                longPressFired = false
                secondPointerId = -1
                if (grabbed) {
                    anchorX = event.x
                    anchorY = event.y
                    leftClickArmed = true
                    // 虚拟光标跨触摸持续累加（MC 取相邻回调增量作为视角旋转，
                    // 重置会引入跳变）
                    GameIpc.cursor(virtualX.toFloat(), virtualY.toFloat())
                } else {
                    // 真实鼠标语义：按下即左键 down（支持按住拖拽）
                    controls?.moveCursor(event.x, event.y)
                    GameIpc.cursor(event.x, event.y)
                    GameIpc.mouse(0, 1, 0)
                    leftDown = true
                    cancelLongPressTimer()
                    postDelayed(longPressRunnable, 600)
                }
            }
            MotionEvent.ACTION_POINTER_DOWN -> {
                if (grabbed) {
                    // 第二指 = 右键
                    if (event.pointerCount >= 2 && !rightHeld) {
                        rightHeld = true
                        leftClickArmed = false
                        GameIpc.mouse(1, 1, 0)
                    }
                } else if (event.pointerCount >= 2 && secondPointerId < 0) {
                    // 第二指 = 滚轮（菜单里滚动列表）
                    cancelLongPressTimer()
                    if (leftDown) {
                        GameIpc.mouse(0, 0, 0)
                        leftDown = false
                    }
                    secondPointerId = event.getPointerId(event.actionIndex)
                    secondAnchorY = event.getY(event.actionIndex)
                }
            }
            MotionEvent.ACTION_MOVE -> {
                if (grabbed) {
                    val dx = event.x - anchorX
                    val dy = event.y - anchorY
                    anchorX = event.x
                    anchorY = event.y
                    virtualX += (dx * lookSensitivity).toDouble()
                    virtualY += (dy * lookSensitivity).toDouble()
                    GameIpc.cursor(virtualX.toFloat(), virtualY.toFloat())
                    if (abs(event.x - downX) > 24 || abs(event.y - downY) > 24) {
                        moved = true
                    }
                } else if (secondPointerId >= 0) {
                    // 双指纵向滑动 = 滚轮
                    val idx = event.findPointerIndex(secondPointerId)
                    if (idx >= 0) {
                        val dy = event.getY(idx) - secondAnchorY
                        secondAnchorY = event.getY(idx)
                        if (abs(dy) >= 1f) {
                            GameIpc.scroll(0.0, (dy * 0.12).toDouble())
                        }
                    }
                    lastX = event.x
                    lastY = event.y
                } else {
                    controls?.moveCursor(event.x, event.y)
                    GameIpc.cursor(event.x, event.y)
                    if (abs(event.x - downX) > 24 || abs(event.y - downY) > 24) {
                        moved = true
                    }
                    lastX = event.x
                    lastY = event.y
                }
            }
            MotionEvent.ACTION_POINTER_UP -> {
                if (grabbed) {
                    if (rightHeld && event.pointerCount <= 2) {
                        rightHeld = false
                        GameIpc.mouse(1, 0, 0)
                    }
                } else if (event.getPointerId(event.actionIndex) == secondPointerId) {
                    secondPointerId = -1
                }
            }
            MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                if (grabbed) {
                    if (rightHeld) {
                        rightHeld = false
                        GameIpc.mouse(1, 0, 0)
                    }
                    // 轻点 = 左键单击（攻击/交互）
                    if (leftClickArmed && !moved && event.actionMasked == MotionEvent.ACTION_UP) {
                        GameIpc.mouse(0, 1, 0)
                        GameIpc.mouse(0, 0, 0)
                    }
                    leftClickArmed = false
                } else {
                    cancelLongPressTimer()
                    if (longPressFired) {
                        GameIpc.mouse(1, 0, 0)   // 长按右键：释放
                    } else {
                        if (leftDown) GameIpc.mouse(0, 0, 0)
                        // 抬起时光标留在最后位置（真实鼠标不回弹）
                        if (event.actionMasked == MotionEvent.ACTION_UP) {
                            controls?.moveCursor(event.x, event.y)
                            GameIpc.cursor(event.x, event.y)
                        }
                    }
                    longPressFired = false
                    leftDown = false
                    secondPointerId = -1
                }
            }
        }
        return true
    }
}
