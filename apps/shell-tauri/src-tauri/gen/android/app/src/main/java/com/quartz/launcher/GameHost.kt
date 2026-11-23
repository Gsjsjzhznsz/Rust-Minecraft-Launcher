package com.quartz.launcher

import android.os.Handler
import android.os.Looper
import android.os.Message
import android.os.Messenger

/**
 * 主进程侧游戏宿主（v0.1.7 崩溃隔离）：
 *
 * 1. 接收 :game 进程回调（MSG_GRAB / MSG_EXIT），更新 UI 状态 / 调用 Rust；
 * 2. external fun 绑定 libquartz_shell_lib.so 的
 *    Java_com_quartz_launcher_GameHost_* 导出符号（Rust 侧 finish_game_exit）。
 */
object GameHost {

    /** 待发送的启动规格路径（startGameServiceStatic 记录，连接后由 GameIpc 发出）。 */
    @JvmStatic
    var pendingSpec: String? = null

    /** 鼠标 grab 状态镜像（:game 上报；GameView 触摸映射读取）。 */
    @JvmStatic
    var grabbed: Boolean = false

    /** :game -> 主进程 回信通道（MSG_INIT replyTo）。 */
    val messenger = Messenger(object : Handler(Looper.getMainLooper()) {
        override fun handleMessage(msg: Message) {
            when (msg.what) {
                GameService.MSG_GRAB -> {
                    grabbed = msg.arg1 == 1
                    MainActivity.instance?.onGrabStateChanged(grabbed)
                }
                GameService.MSG_EXIT -> gameExited(msg.arg1)
            }
        }
    })

    /** :game 正常结束（退出码来自服务；Rust 侧恢复 UI + 上报 launch://exit）。 */
    external fun gameExited(code: Int)

    /** :game 进程意外死亡（binder death；原生崩溃 / 停止请求）。 */
    external fun gameProcessGone()
}
