package com.imagemediaviewer.desktop

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
    requestStoragePermissions()
  }

  /**
   * Android ストレージアクセス権限の確認および要求
   *
   * 変更理由: Android 端末内の画像ファイル（DCIM/Camera, Pictures等）を
   * Rust の走査エンジン（walkdir）およびサムネイルパイプラインから直接読み込めるようにするため。
   * Android 13（TIRAMISU, API 33）以降は READ_MEDIA_IMAGES を要求し、
   * Android 12以前は READ_EXTERNAL_STORAGE / WRITE_EXTERNAL_STORAGE を要求する。
   * 起動時に全ファイルアクセス設定画面へ強制リダイレクトする挙動を排除し、
   * 標準的なシステムランタイムダイアログを表示する。
   */
  private fun requestStoragePermissions() {
    val neededPermissions = mutableListOf<String>()

    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
      if (ContextCompat.checkSelfPermission(this, Manifest.permission.READ_MEDIA_IMAGES)
          != PackageManager.PERMISSION_GRANTED) {
        neededPermissions.add(Manifest.permission.READ_MEDIA_IMAGES)
      }
    } else {
      if (ContextCompat.checkSelfPermission(this, Manifest.permission.READ_EXTERNAL_STORAGE)
          != PackageManager.PERMISSION_GRANTED) {
        neededPermissions.add(Manifest.permission.READ_EXTERNAL_STORAGE)
      }
      if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R &&
          ContextCompat.checkSelfPermission(this, Manifest.permission.WRITE_EXTERNAL_STORAGE)
          != PackageManager.PERMISSION_GRANTED) {
        neededPermissions.add(Manifest.permission.WRITE_EXTERNAL_STORAGE)
      }
    }

    if (neededPermissions.isNotEmpty()) {
      ActivityCompat.requestPermissions(this, neededPermissions.toTypedArray(), 1001)
    }
  }
}
