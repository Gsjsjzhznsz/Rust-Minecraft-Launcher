package com.quartz.launcher

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.view.Surface

/**
 * ART 侧游戏输入/表面桥（v0.1.5）。
 *
 * external fun 绑定到 libquartz_shell_lib.so（Tauri 壳层库）中的
 * Java_com_quartz_launcher_QuartzBridge_* 导出符号，壳层转发给
 * quartz-bridge（Rust 重构的 libpojavexec.so）的 C ABI。
 *
 * onGrabStateChanged / accessAndroidClipboard 由显示桥经 JNI 反向调用
 * （@JvmStatic 生成类静态方法，CallStaticVoidMethodA 目标）。
 */
class QuartzBridge private constructor() {
    companion object {
        val instance: QuartzBridge = QuartzBridge()

        init {
            // 壳层库已由 Tauri 加载，loadLibrary 幂等
            System.loadLibrary("quartz_shell_lib")
        }

        const val CLIPBOARD_COPY = 1000
        const val CLIPBOARD_PASTE = 1001

        /** 最近一条有效 Surface（MSG_SURFACE 先于 :game 侧显示桥就绪时的兜底缓存）。 */
        private var latest: Surface? = null

        /** 显示桥就绪前收到的 Surface 由 GameService 存入；Rust 侧经 latestSurface() 取回重挂。 */
        @JvmStatic
        fun rememberSurface(s: Surface?) {
            latest = s
        }

        /** 最近一条有效 Surface（Rust 显示桥就绪后调用重挂）。 */
        @JvmStatic
        fun latestSurface(): Surface? = latest

        /** 显示桥回调：游戏进入/退出鼠标 grab（MC 捕获鼠标）。 */
        @JvmStatic
        fun onGrabStateChanged(grabbed: Boolean) {
            // :game 进程：转发给主进程（经 Messenger）；主进程：直接更新 UI。
            // （两处都会被调用：非对应进程的一侧因实例为空自动无操作。）
            GameService.notifyGrab(grabbed)
            MainActivity.onGameGrabStateChangedNative(grabbed)
        }

        /** 显示桥回调：游戏剪贴板读写（COPY 时 data 非 null）。 */
        @JvmStatic
        fun accessAndroidClipboard(action: Int, data: String?): String? {
            return try {
                val cm = QuartzApp.instance?.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
                    ?: return null
                if (data != null) {
                    cm.setPrimaryClip(ClipData.newPlainText("quartz", data))
                    null
                } else {
                    cm.primaryClip?.getItemAt(0)?.coerceToText(QuartzApp.instance!!)?.toString()
                }
            } catch (_: Exception) {
                null
            }
        }
    }

    // ---- 以下为壳层 native 转发（最终落到 Rust 显示桥 C ABI） ----
    external fun setupSurface(surface: Surface): Int
    external fun releaseSurface()
    external fun sendCursorPos(x: Float, y: Float)
    external fun sendMouseButton(button: Int, action: Int, mods: Int)
    external fun sendKey(key: Int, scancode: Int, action: Int, mods: Int)
    external fun sendChar(codepoint: Int, mods: Int)
    external fun sendScroll(dx: Double, dy: Double)
    external fun sendScreenSize(w: Int, h: Int)
    external fun isGrabbed(): Boolean
}
