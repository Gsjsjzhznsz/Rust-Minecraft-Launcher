package com.quartz.launcher

import android.Manifest
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.content.pm.ActivityInfo
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Environment
import android.os.IBinder
import android.os.Message
import android.os.Messenger
import android.provider.Settings
import android.view.KeyEvent
import android.view.View
import android.view.ViewGroup
import android.view.WindowManager
import android.webkit.WebView
import android.widget.FrameLayout
import androidx.activity.enableEdgeToEdge
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat

class MainActivity : TauriActivity() {
  companion object {
    /** 当前前台 MainActivity（Rust 侧 JNI 经静态方法转发到实例）。 */
    @JvmStatic
    var instance: MainActivity? = null
      private set

    /**
     * Rust 侧经 JNI GetStaticMethodID 调用的唯一入口。
     * ndk-context 提供给 Rust 的是 Application 对象，其上不存在自定义实例方法，
     * 直接调用必抛 NoSuchMethodError —— 因此必须走类静态方法转发到当前 Activity。
     */
    @JvmStatic
    fun requestStorageAccessStatic() {
      instance?.requestStorageAccess()
    }

    /**
     * Rust 侧经 JNI 调用的方向切换入口：进游戏切横屏，游戏退出切回竖屏。
     * runOnUiThread：requestedOrientation 只能在主线程改；切方向不重建
     * Activity（Manifest 已声明 configChanges 含 orientation|screenSize）。
     */
    @JvmStatic
    fun setGameOrientationStatic(landscape: Boolean) {
      val act = instance ?: return
      act.runOnUiThread {
        act.requestedOrientation = if (landscape) {
          ActivityInfo.SCREEN_ORIENTATION_SENSOR_LANDSCAPE
        } else {
          ActivityInfo.SCREEN_ORIENTATION_PORTRAIT
        }
      }
    }

    /**
     * 显示游戏层（Rust 显示桥启动前调用）：横屏 + SurfaceView 覆盖 WebView +
     * 虚拟按键层 + 屏幕常亮。Surface 创建回调交接给 quartz-bridge。
     */
    @JvmStatic
    fun showGameSurfaceStatic() {
      val act = instance ?: return
      act.runOnUiThread {
        act.requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_SENSOR_LANDSCAPE
        act.showGameSurface()
      }
    }

    /** 隐藏游戏层并恢复竖屏启动器 UI（游戏退出 / 启动失败时调用）。 */
    @JvmStatic
    fun hideGameSurfaceStatic() {
      val act = instance ?: return
      act.runOnUiThread {
        act.hideGameSurface()
        act.requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_PORTRAIT
      }
    }

    /**
     * 重启应用进程：同进程内 OpenJDK JVM 只能 JNI_CreateJavaVM 一次，
     * 游戏退出后再次启动前用它重置进程（新任务启动后立刻结束旧进程）。
     */
    @JvmStatic
    fun restartAppStatic() {
      val act = instance ?: return
      act.runOnUiThread {
        try {
          val intent = act.packageManager.getLaunchIntentForPackage(act.packageName)
          if (intent != null) {
            intent.addFlags(
              Intent.FLAG_ACTIVITY_NEW_TASK or
                Intent.FLAG_ACTIVITY_CLEAR_TOP or
                Intent.FLAG_ACTIVITY_RESET_TASK_IF_NEEDED
            )
            act.startActivity(intent)
          }
        } catch (_: Throwable) {
        }
        // 新任务已入栈，结束本进程（不经 crash 通道）
        android.os.Process.killProcess(android.os.Process.myPid())
      }
    }

    /**
     * Rust 侧经 JNI 调用：启动 :game 游戏进程服务（传入启动规格路径）。
     * 服务连接后由 GameIpc.connected 补发 MSG_INIT（规格 + 回信通道）。
     */
    @JvmStatic
    fun startGameServiceStatic(specPath: String) {
      val act = instance ?: return
      act.runOnUiThread {
        GameHost.pendingSpec = specPath
        act.startGameService()
      }
    }

    /** Rust 侧经 JNI 调用：停止游戏（向 :game 服务发 MSG_STOP，进程自杀）。 */
    @JvmStatic
    fun stopGameServiceStatic() {
      val act = instance ?: return
      act.runOnUiThread { act.stopGameService() }
    }

    /** QuartzBridge 回调入口（游戏鼠标 grab 状态变化）。 */
    @JvmStatic
    fun onGameGrabStateChangedNative(grabbed: Boolean) {
      val act = instance ?: return
      act.runOnUiThread { act.onGrabStateChanged(grabbed) }
    }
  }

  // ---- 游戏层状态 ----
  private var gameView: GameView? = null
  private var gameControls: GameControls? = null
  private var webView: WebView? = null
  private var webViewVisibleBeforeGame = true

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    // 竖屏 UI 修复配套：禁用 WebView 缩放（viewport meta 同时 user-scalable=no）。
    // Tauri 的 WebView 为异步创建，轮询等待并配置。
    scheduleWebViewZoomConfigure(0)
  }

  override fun onResume() {
    super.onResume()
    instance = this
  }

  override fun onDestroy() {
    super.onDestroy()
    if (instance === this) {
      instance = null
    }
  }

  // ------------------------------------------------------------------
  // WebView 配置（禁缩放）
  // ------------------------------------------------------------------

  private fun scheduleWebViewZoomConfigure(attempt: Int) {
    if (attempt > 30) return
    window.decorView.postDelayed({
      val wv = findWebView(window.decorView)
      if (wv != null) {
        webView = wv
        try {
          wv.settings.apply {
            setSupportZoom(false)
            builtInZoomControls = false
            displayZoomControls = false
            textZoom = 100
          }
        } catch (_: Throwable) {
        }
      } else {
        scheduleWebViewZoomConfigure(attempt + 1)
      }
    }, 150)
  }

  private fun findWebView(root: View): WebView? {
    if (root is WebView) return root
    if (root is ViewGroup) {
      for (i in 0 until root.childCount) {
        findWebView(root.getChildAt(i))?.let { return it }
      }
    }
    return null
  }

  // ------------------------------------------------------------------
  // 游戏层显隐（Rust 经 showGameSurfaceStatic/hideGameSurfaceStatic 调用）
  // ------------------------------------------------------------------

  fun showGameSurface() {
    val content = findViewById<ViewGroup>(android.R.id.content) ?: return
    if (gameView != null) return
    webViewVisibleBeforeGame = webView?.visibility == View.VISIBLE
    webView?.visibility = View.GONE
    window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
    val gv = GameView(this)
    val controls = GameControls(this)
    gameView = gv
    gameControls = controls
    content.addView(
      gv,
      FrameLayout.LayoutParams(
        ViewGroup.LayoutParams.MATCH_PARENT,
        ViewGroup.LayoutParams.MATCH_PARENT
      )
    )
    content.addView(
      controls,
      FrameLayout.LayoutParams(
        ViewGroup.LayoutParams.MATCH_PARENT,
        ViewGroup.LayoutParams.MATCH_PARENT
      )
    )
  }

  fun hideGameSurface() {
    val content = findViewById<ViewGroup>(android.R.id.content) ?: return
    window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
    gameControls?.let {
      (it.parent as? ViewGroup)?.removeView(it)
    }
    // GameView 的 surfaceDestroyed 回调负责 releaseSurface（移除视图即触发）
    gameView?.let {
      (it.parent as? ViewGroup)?.removeView(it)
    }
    gameControls = null
    gameView = null
    if (webViewVisibleBeforeGame) {
      webView?.visibility = View.VISIBLE
    }
  }

  /** 游戏鼠标 grab 状态变化（QuartzBridge 经静态方法回调）。 */
  fun onGrabStateChanged(grabbed: Boolean) {
    gameControls?.alpha = if (grabbed) 1f else 0.85f
  }

  // ------------------------------------------------------------------
  // 物理键盘（蓝牙键盘 / 部分游戏手柄的键盘模式）转发
  // ------------------------------------------------------------------

  override fun dispatchKeyEvent(event: KeyEvent): Boolean {
    if (gameView != null) {
      val glfw = GlfwKeys.toGlfwKey(event.keyCode)
      val action = when (event.action) {
        KeyEvent.ACTION_DOWN -> 1
        KeyEvent.ACTION_UP -> 0
        else -> -1
      }
      if (glfw != 0 && action >= 0) {
        val mods = GlfwKeys.mods(event.metaState)
        val repeats = if (event.action == KeyEvent.ACTION_DOWN) event.repeatCount else 0
        if (action == 1 && repeats == 0) {
          GameIpc.key(glfw, 0, 1, mods)
          // 可打印字符同步发 char 事件（聊天输入）
          if (event.isPrintingKey) {
            val ch = event.unicodeChar
            if (ch > 0) GameIpc.char(ch, mods)
          }
        } else if (action == 0) {
          GameIpc.key(glfw, 0, 0, mods)
        }
        return true
      }
    }
    return super.dispatchKeyEvent(event)
  }

  // ------------------------------------------------------------------
  // :game 游戏进程服务（v0.1.7 崩溃隔离）
  // ------------------------------------------------------------------

  private val gameServiceConnection = object : ServiceConnection {
    override fun onServiceConnected(name: ComponentName?, binder: IBinder?) {
      if (binder != null) {
        GameIpc.connected(Messenger(binder))
      }
    }

    override fun onServiceDisconnected(name: ComponentName?) {
      // :game 进程死亡（崩溃 / 自杀）：binder death 通知 -> Rust 恢复 UI
      GameIpc.disconnected()
      try {
        GameHost.gameProcessGone()
      } catch (_: Throwable) {
      }
    }

    override fun onBindingDied(name: ComponentName?) {
      GameIpc.disconnected()
      try {
        GameHost.gameProcessGone()
      } catch (_: Throwable) {
      }
    }

    override fun onNullBinding(name: ComponentName?) {
      GameIpc.disconnected()
    }
  }

  private fun startGameService() {
    try {
      val intent = Intent(this, GameService::class.java)
      // startService：保证主进程临时 unbind 后 :game 不被销毁
      startService(intent)
      bindService(
        intent,
        gameServiceConnection,
        Context.BIND_AUTO_CREATE or Context.BIND_IMPORTANT
      )
    } catch (_: Throwable) {
    }
  }

  private fun stopGameService() {
    try {
      GameService.serviceMessenger?.send(Message.obtain(null, GameService.MSG_STOP))
    } catch (_: Throwable) {
    }
    try {
      unbindService(gameServiceConnection)
    } catch (_: Throwable) {
    }
    try {
      stopService(Intent(this, GameService::class.java))
    } catch (_: Throwable) {
    }
    GameIpc.disconnected()
  }

  // ------------------------------------------------------------------
  // 存储权限（完整文件权限）
  // ------------------------------------------------------------------

  /**
   * 存储权限（完整文件权限）是否已授权：
   * - Android 11+（API 30+）：「所有文件访问」MANAGE_EXTERNAL_STORAGE
   * - Android 10 及以下：WRITE_EXTERNAL_STORAGE 运行时授权（targetSdk 28 旧模型）
   * 注：Rust 侧权限判定已改为直接调静态 Environment.isExternalStorageManager() /
   * Context.checkSelfPermission（不依赖 Activity 实例）；本方法保留作壳层内部判定。
   */
  fun isStorageAccessGranted(): Boolean {
    return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
      Environment.isExternalStorageManager()
    } else {
      ContextCompat.checkSelfPermission(this, Manifest.permission.WRITE_EXTERNAL_STORAGE) ==
        PackageManager.PERMISSION_GRANTED
    }
  }

  /**
   * 申请完整文件权限：
   * - Android 11+：跳系统「所有文件访问」设置页。不走存储权限弹窗 ——
   *   targetSdk<=29 应用在 Android 11+ 弹窗授权存储时系统会杀进程刷新
   *   FUSE 挂载视图（用户视角 = 授权后闪退）；设置页授权不杀进程。
   * - Android 10 及以下：运行时权限弹窗（旧存储模型，授权即时生效）。
   * 由 Rust 侧经 JNI 调用（request_storage_permission 命令）。
   */
  fun requestStorageAccess() {
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
      // 1) 带包名的直达页（各厂商 ROM 大多支持）
      try {
        startActivity(
          Intent(
            Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION,
            Uri.parse("package:$packageName")
          ).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        )
        return
      } catch (_: Exception) {
      }
      // 2) 通用「所有文件访问」列表页兜底
      try {
        startActivity(
          Intent(Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        )
        return
      } catch (_: Exception) {
      }
    }
    // 3) Android 10-（或 ROM 连设置页都没有时）运行时弹窗
    ActivityCompat.requestPermissions(
      this,
      arrayOf(
        Manifest.permission.WRITE_EXTERNAL_STORAGE,
        Manifest.permission.READ_EXTERNAL_STORAGE
      ),
      42
    )
  }
}
