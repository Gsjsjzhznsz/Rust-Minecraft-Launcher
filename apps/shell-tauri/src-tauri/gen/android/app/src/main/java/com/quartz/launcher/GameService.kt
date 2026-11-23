package com.quartz.launcher

import android.app.Service
import android.content.Intent
import android.os.Bundle
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.Message
import android.os.Messenger
import android.os.Process
import android.view.Surface

/**
 * 游戏运行进程（v0.1.7 崩溃隔离，Manifest 声明 android:process=":game"）：
 *
 * - OpenJDK JVM + quartz-bridge 显示桥全部在本进程内运行；
 * - 主进程（WebView 启动器 UI）经 Messenger 下发启动规格 / Surface / 输入事件；
 * - 游戏崩溃（SIGSEGV/SIGABRT/System.exit/killProcess）只终止本进程，主进程
 *   经 ServiceConnection binder death 回调感知并恢复启动器 UI —— 崩溃不再
 *   陪葬整个 app；
 * - 干净退出路径：nativeRunGame 返回后发 MSG_EXIT（带回退出码），延迟让
 *   binder 送达后自杀进程。
 */
class GameService : Service() {

    companion object {
        // ---- 主进程 -> :game ----
        const val MSG_INIT = 1             // data: spec(String)；replyTo = 主进程 Messenger
        const val MSG_SURFACE = 2          // data: surface(Surface)
        const val MSG_RELEASE_SURFACE = 3
        const val MSG_INPUT = 4            // data: t(Int) + 参数（见 GameIpc）
        const val MSG_SIZE = 5             // data: w,h
        const val MSG_STOP = 6             // 自杀进程（用户点停止）

        // ---- :game -> 主进程 ----
        const val MSG_GRAB = 101           // arg1: 0/1
        const val MSG_EXIT = 102           // arg1: 退出码

        // 输入子类型（MSG_INPUT data "t"）
        const val IN_CURSOR = 0
        const val IN_MOUSE = 1
        const val IN_KEY = 2
        const val IN_CHAR = 3
        const val IN_SCROLL = 4

        /** 主进程持有的、指向本服务的 Messenger（GameIpc 使用，仅主进程非空）。 */
        @JvmStatic
        var serviceMessenger: Messenger? = null

        /** 主进程回信 Messenger（仅 :game 进程内非空）。 */
        @JvmStatic
        var client: Messenger? = null

        /** :game -> 主进程：鼠标 grab 状态变化（QuartzBridge 回调转发）。 */
        @JvmStatic
        fun notifyGrab(grabbed: Boolean) {
            val c = client ?: return
            val m = Message.obtain(null, MSG_GRAB)
            m.arg1 = if (grabbed) 1 else 0
            try {
                c.send(m)
            } catch (_: Throwable) {
            }
        }
    }

    private val handler = object : Handler(Looper.getMainLooper()) {
        override fun handleMessage(msg: Message) {
            when (msg.what) {
                MSG_INIT -> {
                    client = msg.replyTo
                    startGame(msg.data?.getString("spec") ?: "")
                }
                MSG_SURFACE -> {
                    @Suppress("DEPRECATION")
                    val s = msg.data?.getParcelable<Surface>("surface")
                    if (s != null && s.isValid) {
                        try {
                            // :game 进程内直调桥 native（ANativeWindow_fromSurface）
                            QuartzBridge.instance.setupSurface(s)
                        } catch (_: Throwable) {
                        }
                    }
                }
                MSG_RELEASE_SURFACE -> {
                    try {
                        QuartzBridge.instance.releaseSurface()
                    } catch (_: Throwable) {
                    }
                }
                MSG_SIZE -> {
                    val b = msg.data
                    try {
                        QuartzBridge.instance.sendScreenSize(b?.getInt("w") ?: 0, b?.getInt("h") ?: 0)
                    } catch (_: Throwable) {
                    }
                }
                MSG_INPUT -> forwardInput(msg.data)
                MSG_STOP -> {
                    Process.killProcess(Process.myPid())
                }
            }
        }
    }

    private val messenger = Messenger(handler)

    override fun onBind(intent: Intent?): IBinder = messenger.binder

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        // startService 保证主进程 unbind 后 :game 不被销毁（started service）
        return START_NOT_STICKY
    }

    override fun onDestroy() {
        serviceMessenger = null
        super.onDestroy()
    }

    /** 输入事件 -> 桥（:game 进程内直调）。 */
    private fun forwardInput(b: Bundle?) {
        if (b == null) return
        val bridge = QuartzBridge.instance
        try {
            when (b.getInt("t", -1)) {
                IN_CURSOR -> bridge.sendCursorPos(b.getFloat("x"), b.getFloat("y"))
                IN_MOUSE -> bridge.sendMouseButton(b.getInt("b"), b.getInt("a"), b.getInt("m"))
                IN_KEY -> bridge.sendKey(b.getInt("k"), b.getInt("s"), b.getInt("a"), b.getInt("m"))
                IN_CHAR -> bridge.sendChar(b.getInt("cp"), b.getInt("m"))
                IN_SCROLL -> bridge.sendScroll(b.getDouble("dx"), b.getDouble("dy"))
            }
        } catch (_: Throwable) {
        }
    }

    /** 游戏启动线程：装载壳层库 -> nativeRunGame（阻塞至 JVM 退出）-> 回报退出码 -> 自杀。 */
    private fun startGame(specPath: String) {
        Thread {
            try {
                System.loadLibrary("quartz_shell_lib")
            } catch (_: Throwable) {
            }
            val code = try {
                nativeRunGame(specPath)
            } catch (t: Throwable) {
                254
            }
            // 干净退出：回报退出码（崩溃时进程已死，走 binder death 路径）
            client?.let { c ->
                val m = Message.obtain(null, MSG_EXIT)
                m.arg1 = code
                try {
                    c.send(m)
                } catch (_: Throwable) {
                }
            }
            try {
                Thread.sleep(600) // binder 送达窗口
            } catch (_: InterruptedException) {
            }
            Process.killProcess(Process.myPid())
        }.apply { name = "quartz-game-runner" }.start()
    }

    /** JNI -> libquartz_shell_lib.so 的 Java_com_quartz_launcher_GameService_nativeRunGame。 */
    private external fun nativeRunGame(specPath: String): Int
}
