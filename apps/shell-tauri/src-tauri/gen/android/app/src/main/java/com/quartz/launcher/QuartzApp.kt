package com.quartz.launcher

import android.app.Application
import java.io.File
import java.io.PrintWriter
import java.io.StringWriter

/**
 * 崩溃自愈入口（v8「一打开就闪退」加固，与 Rust 壳层 panic hook 配套）：
 *
 * 1. 全局未捕获异常处理器：任何 Java/Kotlin 崩溃先追加进
 *    <filesDir>/crash/quartz-crash.log（与 Rust panic hook 同一文件、同一标记格式
 *    `==== CRASH @<epoch秒> KIND ====`），Rust 侧 get_diagnostics 经 UI 横幅展示
 *    崩溃原因 —— 闪退不再黑盒。落盘后交回系统默认处理（保持标准崩溃对话框）。
 *
 * 2. WebView 崩溃循环自愈：2 分钟内 ≥2 次崩溃记录（含 Rust panic / 原生崩溃后
 *    重启）时，在 Activity/WebView 创建前清理 app_webview（Chromium profile）。
 *    进程在写 WebView 数据时被杀会造成 IndexedDB/HTTP 缓存损坏，表现为
 *    「一打开就闪退」且每次必现 —— 清 profile 即恢复。只删 app_webview，
 *    不碰 Tauri 应用数据（app_data）与任何游戏文件。
 */
class QuartzApp : Application() {

  override fun onCreate() {
    super.onCreate()
    installCrashHandler()
    recoverWebViewCrashLoop()
  }

  private fun crashFile(): File {
    val dir = File(filesDir, "crash")
    if (!dir.exists()) dir.mkdirs()
    return File(dir, "quartz-crash.log")
  }

  /** 追加崩溃标记（Rust 侧 append_crash_marker 同款格式）。任何失败静默。 */
  fun appendMarker(kind: String, body: String) {
    try {
      val epoch = System.currentTimeMillis() / 1000
      crashFile().appendText("==== CRASH @$epoch $kind ====\n$body\n")
    } catch (_: Throwable) {
    }
  }

  /** 全局未捕获异常 → 落盘后交回系统默认处理。 */
  private fun installCrashHandler() {
    val previous = Thread.getDefaultUncaughtExceptionHandler()
    Thread.setDefaultUncaughtExceptionHandler { thread, throwable ->
      val sw = StringWriter()
      throwable.printStackTrace(PrintWriter(sw))
      val stack = sw.toString()
      appendMarker("JAVA", "thread: ${thread.name}\n${stack.take(6000)}")
      previous?.uncaughtException(thread, throwable)
    }
  }

  /**
   * WebView 崩溃循环自愈：统计崩溃标记中 2 分钟内的条数，≥2 次即清理
   * app_webview 并留 RECOVERY 标记。数据损失仅限 WebView 缓存与站点存储。
   */
  private fun recoverWebViewCrashLoop() {
    try {
      val f = crashFile()
      if (!f.exists()) return
      val nowSec = System.currentTimeMillis() / 1000
      val windowSec = LOOP_WINDOW_MS / 1000
      var recent = 0
      for (line in f.readLines()) {
        if (!line.startsWith("==== CRASH @")) continue
        val epoch = line.removePrefix("==== CRASH @")
          .substringBefore(' ')
          .toLongOrNull()
          ?: continue
        if (nowSec - epoch in 0..windowSec) recent++
      }
      if (recent >= LOOP_THRESHOLD) {
        val webviewDir = File(applicationInfo.dataDir, "app_webview")
        if (webviewDir.exists()) {
          webviewDir.deleteRecursively()
        }
        appendMarker(
          "RECOVERY",
          "检测到 $recent 次 2 分钟内崩溃，已在启动前清理 WebView 数据目录（app_webview）以自愈"
        )
      }
    } catch (_: Throwable) {
    }
  }

  companion object {
    /** 崩溃循环判定窗口与阈值。 */
    const val LOOP_WINDOW_MS: Long = 120_000L
    const val LOOP_THRESHOLD: Int = 2
  }
}
