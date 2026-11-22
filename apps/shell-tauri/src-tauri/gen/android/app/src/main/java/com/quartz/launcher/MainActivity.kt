package com.quartz.launcher

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Environment
import android.provider.Settings
import androidx.activity.enableEdgeToEdge
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
  }

  /**
   * 存储权限（完整文件权限）是否已授权：
   * - Android 11+（API 30+）：「所有文件访问」MANAGE_EXTERNAL_STORAGE
   * - Android 10 及以下：WRITE_EXTERNAL_STORAGE 运行时授权（targetSdk 28 旧模型）
   * 由 Rust 侧经 JNI 调用（get_permission_state / reinit_storage 链路）。
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
