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

import java.io.File
import java.io.FileOutputStream
import android.util.Log

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    extractAssetsPlugins()
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    requestStoragePermissions()
  }

  /**
   * APK 内の assets/plugins ディレクトリを展開し、内部ストレージ（files/plugins）へ同期配置する
   *
   * 変更理由: Android では APK 内の assets は圧縮/未展開アーカイブ内に存在するため、
   * Rust のファイル走査（std::fs::read_dir）から直接アクセスできない。
   * 起動時に assets/plugins 内のマニフェストファイル（plugin.json 等）を
   * アプリ内部ストレージ（filesDir/plugins）へ安全に展開することで、
   * プラグイン検出エンジンが全フォーマットプラグインを確実にロードできるようにする。
   * （なお、ネイティブ共有ライブラリ .so は jniLibs より OS が直接ロードするため、不要なバイナリは除外）
   */
  private fun extractAssetsPlugins() {
    try {
      val targetDir = File(filesDir, "plugins")
      copyAssetFolder("plugins", targetDir)
    } catch (e: Exception) {
      Log.e("MainActivity", "Failed to extract plugins from assets: ${e.message}", e)
    }
  }

  /**
   * アセットフォルダを再帰的に内部ストレージへコピーする
   *
   * @param assetPath アセット相対パス
   * @param targetDir コピー先ディレクトリ
   */
  private fun copyAssetFolder(assetPath: String, targetDir: File) {
    val list = assets.list(assetPath)
    if (list != null && list.isNotEmpty()) {
      targetDir.mkdirs()
      for (child in list) {
        val childAssetPath = if (assetPath.isEmpty()) child else "$assetPath/$child"
        val childTarget = File(targetDir, child)
        copyAssetFolder(childAssetPath, childTarget)
      }
    } else {
      // 拡張子が実行バイナリ/共有ライブラリ（.so, .dll, .exe）の場合はストレージ無駄遣い防止のためスキップ
      if (assetPath.endsWith(".so") || assetPath.endsWith(".dll") || assetPath.endsWith(".exe")) {
        return
      }
      try {
        assets.open(assetPath).use { input ->
          targetDir.parentFile?.mkdirs()
          FileOutputStream(targetDir).use { output ->
            input.copyTo(output)
          }
        }
      } catch (_: Exception) {
        // ディレクトリまたは読み込み不可アセットの場合は無視
      }
    }
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
