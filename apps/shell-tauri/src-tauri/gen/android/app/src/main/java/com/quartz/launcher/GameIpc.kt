package com.quartz.launcher

import android.os.Bundle
import android.os.Message
import android.view.Surface

/**
 * 主进程 -> :game 进程的表面/输入转发通道（v0.1.7 崩溃隔离）。
 *
 * - 游戏未连接时缓存 Surface（连接后补发）；输入事件直接丢弃（游戏未运行）；
 * - 主进程内不再直调 QuartzBridge native（桥只在 :game 进程加载），
 *   一切经 Messenger IPC 转发。
 */
object GameIpc {

    /** 连接前收到的 Surface（GameView.surfaceCreated 先于服务连接时暂存）。 */
    private var pendingSurface: Surface? = null

    /** 服务已连接（onServiceConnected）：记录 messenger，补发 MSG_INIT 与暂存 Surface。 */
    fun connected(messenger: android.os.Messenger) {
        GameService.serviceMessenger = messenger
        // MSG_INIT：启动规格 + 主进程回信通道
        val spec = GameHost.pendingSpec
        if (spec != null) {
            val m = Message.obtain(null, GameService.MSG_INIT)
            val b = Bundle()
            b.putString("spec", spec)
            m.data = b
            m.replyTo = GameHost.messenger
            trySend(m)
            GameHost.pendingSpec = null
        }
        pendingSurface?.let { s ->
            pendingSurface = null
            sendSurface(s)
        }
    }

    /** 服务断开（binder death / unbind）。 */
    fun disconnected() {
        GameService.serviceMessenger = null
    }

    /** Surface 创建（GameView.surfaceCreated）。 */
    fun sendSurface(s: Surface) {
        android.util.Log.i("QuartzGame", "sendSurface: valid=${s.isValid} messenger=${GameService.serviceMessenger != null}")
        if (GameService.serviceMessenger == null) {
            pendingSurface = s
            return
        }
        val m = Message.obtain(null, GameService.MSG_SURFACE)
        val b = Bundle()
        b.putParcelable("surface", s)
        m.data = b
        trySend(m)
    }

    /** Surface 销毁（GameView.surfaceDestroyed）。 */
    fun releaseSurface() {
        android.util.Log.i("QuartzGame", "releaseSurface（主进程 surfaceDestroyed）")
        pendingSurface = null
        trySend(Message.obtain(null, GameService.MSG_RELEASE_SURFACE))
    }

    fun cursor(x: Float, y: Float) =
        input(GameService.IN_CURSOR) { putFloat("x", x); putFloat("y", y) }

    fun mouse(button: Int, action: Int, mods: Int) =
        input(GameService.IN_MOUSE) { putInt("b", button); putInt("a", action); putInt("m", mods) }

    fun key(k: Int, scancode: Int, action: Int, mods: Int) =
        input(GameService.IN_KEY) { putInt("k", k); putInt("s", scancode); putInt("a", action); putInt("m", mods) }

    fun char(codepoint: Int, mods: Int) =
        input(GameService.IN_CHAR) { putInt("cp", codepoint); putInt("m", mods) }

    fun scroll(dx: Double, dy: Double) =
        input(GameService.IN_SCROLL) { putDouble("dx", dx); putDouble("dy", dy) }

    fun size(w: Int, h: Int) {
        if (GameService.serviceMessenger == null) return
        val m = Message.obtain(null, GameService.MSG_SIZE)
        val b = Bundle()
        b.putInt("w", w)
        b.putInt("h", h)
        m.data = b
        trySend(m)
    }

    private inline fun input(type: Int, fill: Bundle.() -> Unit) {
        if (GameService.serviceMessenger == null) return
        val m = Message.obtain(null, GameService.MSG_INPUT)
        val b = Bundle()
        b.putInt("t", type)
        b.fill()
        m.data = b
        trySend(m)
    }

    private fun trySend(m: Message) {
        val ms = GameService.serviceMessenger
        if (ms == null) {
            return
        }
        try {
            ms.send(m)
        } catch (_: Throwable) {
        }
    }
}
