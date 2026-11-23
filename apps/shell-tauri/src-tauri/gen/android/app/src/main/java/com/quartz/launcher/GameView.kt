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
 * - 触摸 -> 鼠标映射（与 Pojav 同语义）：
 *   · 未 grab（菜单/UI）：触摸即指针，按下=左键，抬起=左键释放；
 *   · grab（游戏内）：拖动=视角（LookDelta 经虚拟光标绝对位置上报），
 *     轻点=左键单击，第二指=右键。
 * - 物理键盘（含蓝牙键鼠）由 Activity.dispatchKeyEvent 统一转发。
 */
@SuppressLint("ViewConstructor")
class GameView(context: Context) : SurfaceView(context, null, 0), SurfaceHolder.Callback {

    // 虚拟光标（grab 模式下的绝对位置；游戏经光标回调取增量）
    private var virtualX = 0.0
    private var virtualY = 0.0
    // 未 grab 模式的点击锚点
    private var downX = 0f
    private var downY = 0f
    private var moved = false
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
        val surface: Surface = holder.surface
        GameIpc.sendSurface(surface)
    }

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        GameIpc.size(width, height)
    }

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        GameIpc.releaseSurface()
    }

    /** 重置虚拟光标（窗口尺寸变化后由 Activity 调用）。 */
    fun resetCursor() {
        virtualX = 0.0
        virtualY = 0.0
    }

    @SuppressLint("ClickableViewAccessibility")
    override fun onTouchEvent(event: MotionEvent): Boolean {
        val grabbed = GameHost.grabbed
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                downX = event.x
                downY = event.y
                moved = false
                if (grabbed) {
                    anchorX = event.x
                    anchorY = event.y
                    leftClickArmed = true
                    // 虚拟光标跨触摸持续累加（MC 取相邻回调增量作为视角旋转，
                    // 重置会引入跳变）
                    GameIpc.cursor(virtualX.toFloat(), virtualY.toFloat())
                } else {
                    GameIpc.cursor(event.x, event.y)
                    GameIpc.mouse(0, 1, 0)
                }
            }
            MotionEvent.ACTION_POINTER_DOWN -> {
                if (grabbed && event.actionMasked == MotionEvent.ACTION_POINTER_DOWN) {
                    // 第二指 = 右键
                    if (event.pointerCount >= 2 && !rightHeld) {
                        rightHeld = true
                        leftClickArmed = false
                        GameIpc.mouse(1, 1, 0)
                    }
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
                } else {
                    GameIpc.cursor(event.x, event.y)
                }
            }
            MotionEvent.ACTION_POINTER_UP -> {
                if (grabbed && rightHeld && event.pointerCount <= 2) {
                    rightHeld = false
                    GameIpc.mouse(1, 0, 0)
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
                    GameIpc.cursor(event.x, event.y)
                    GameIpc.mouse(0, 0, 0)
                }
            }
        }
        return true
    }
}
