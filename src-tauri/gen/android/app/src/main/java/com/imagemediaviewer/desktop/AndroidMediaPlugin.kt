package com.imagemediaviewer.desktop

import android.app.Activity
import android.content.Intent
import android.media.MediaScannerConnection
import android.net.Uri
import android.os.Environment
import android.provider.MediaStore
import android.util.Log
import android.webkit.MimeTypeMap
import android.widget.Toast
import androidx.core.content.FileProvider
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.Plugin
import java.io.File

@InvokeArg
class OpenFileArgs {
  var path: String = ""
  var mimeType: String? = null
  var packageName: String? = null
}

@InvokeArg
class OpenFolderArgs {
  var path: String = ""
}

@InvokeArg
class TrashFileArgs {
  var path: String = ""
}

/**
 * Android ネイティブメディア連携プラグイン
 *
 * 変更理由: Android 7+ (API 24+) における FileUriExposedException の回避、
 * FileProvider による安全な content:// URI 生成、OS アプリ選択シート（Intent Chooser）の表示、
 * DocumentsContract を用いた Files アプリ内フォルダのダイレクト表示、
 * および Scoped Storage 環境下での確実なファイル削除を実現するため。
 */
@TauriPlugin
class AndroidMediaPlugin(private val activity: Activity) : Plugin(activity) {

  /**
   * 画像ファイルを既定アプリまたは外部アプリ（CLIP STUDIO PAINT 等）で開く
   *
   * @param invoke Tauri Invoke ハンドラ
   */
  @Command
  fun openFile(invoke: Invoke) {
    try {
      val args = invoke.parseArgs(OpenFileArgs::class.java)
      if (args.path.isBlank()) {
        invoke.reject("ファイルパスが空です")
        return
      }

      val file = File(args.path)
      if (!file.exists()) {
        invoke.reject("指定されたファイルが存在しません: ${args.path}")
        return
      }

      // FileProvider による安全な content:// URI 生成
      val contentUri: Uri = FileProvider.getUriForFile(
        activity,
        "${activity.packageName}.fileprovider",
        file
      )

      // MIME タイプの判定
      val extension = file.extension.lowercase()
      val detectedMime = when {
        !args.mimeType.isNullOrBlank() -> args.mimeType
        extension == "clip" -> "application/x-clip"
        else -> MimeTypeMap.getSingleton().getMimeTypeFromExtension(extension) ?: "image/*"
      }

      val viewIntent = Intent(Intent.ACTION_VIEW).apply {
        setDataAndType(contentUri, detectedMime)
        addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
      }

      // 特定パッケージ名が指定されている場合（Clip Studio Paint 等）
      val targetPackage = args.packageName
      var targetLaunched = false

      if (!targetPackage.isNullOrBlank()) {
        try {
          activity.packageManager.getPackageInfo(targetPackage, 0)
          viewIntent.setPackage(targetPackage)
          activity.startActivity(viewIntent)
          targetLaunched = true
        } catch (_: Exception) {
          Log.i("AndroidMediaPlugin", "Target package not installed or failed: $targetPackage. Falling back to chooser.")
          viewIntent.setPackage(null)
        }
      }

      if (!targetLaunched) {
        // OS 標準の Intent Chooser（アプリ選択シート）を表示
        val chooserTitle = if (extension == "clip") {
          "CLIP STUDIO PAINT または対応アプリを選択"
        } else {
          "画像を開くアプリを選択"
        }

        val chooser = Intent.createChooser(viewIntent, chooserTitle).apply {
          addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
          addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        activity.startActivity(chooser)
      }

      invoke.resolve()
    } catch (e: Exception) {
      Log.e("AndroidMediaPlugin", "openFile failed: ${e.message}", e)
      invoke.reject("ファイルのオープンに失敗しました: ${e.message}")
    }
  }

  /**
   * ファイルが属する親フォルダをシステムファイルマネージャで開く
   *
   * @param invoke Tauri Invoke ハンドラ
   */
  @Command
  fun openFolder(invoke: Invoke) {
    try {
      val args = invoke.parseArgs(OpenFolderArgs::class.java)
      if (args.path.isBlank()) {
        invoke.reject("フォルダパスが空です")
        return
      }

      val file = File(args.path)
      val folder = if (file.isDirectory) file else (file.parentFile ?: file)

      var opened = false

      // 1. DocumentsUI (Android 標準 Files アプリ) によるダイレクトフォルダオープン
      val externalStorage = Environment.getExternalStorageDirectory()
      if (folder.absolutePath.startsWith(externalStorage.absolutePath)) {
        val relPath = folder.absolutePath.removePrefix(externalStorage.absolutePath).trimStart('/')
        val encodedRel = Uri.encode(relPath, "/")
        val docUri = Uri.parse("content://com.android.externalstorage.documents/document/primary%3A$encodedRel")

        val docIntent = Intent(Intent.ACTION_VIEW).apply {
          setDataAndType(docUri, "vnd.android.document/directory")
          addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
          addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        }

        try {
          activity.startActivity(docIntent)
          opened = true
        } catch (e: Exception) {
          Log.i("AndroidMediaPlugin", "DocumentsUI direct folder open not supported: ${e.message}")
        }
      }

      // 2. FileProvider 経由での汎用ファイルマネージャ起動 (resource/folder)
      if (!opened) {
        try {
          val folderUri = FileProvider.getUriForFile(
            activity,
            "${activity.packageName}.fileprovider",
            folder
          )
          val folderIntent = Intent(Intent.ACTION_VIEW).apply {
            setDataAndType(folderUri, "resource/folder")
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
          }
          val chooser = Intent.createChooser(folderIntent, "フォルダを開くアプリを選択").apply {
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
          }
          activity.startActivity(chooser)
          opened = true
        } catch (e: Exception) {
          Log.i("AndroidMediaPlugin", "FileProvider folder intent failed: ${e.message}")
        }
      }

      // 3. フォールバック: ストレージ設定の表示
      if (!opened) {
        try {
          val settingsIntent = Intent(android.provider.Settings.ACTION_INTERNAL_STORAGE_SETTINGS).apply {
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
          }
          activity.startActivity(settingsIntent)
          opened = true
        } catch (_: Exception) {}
      }

      activity.runOnUiThread {
        Toast.makeText(activity, "フォルダ: ${folder.name}", Toast.LENGTH_SHORT).show()
      }

      invoke.resolve()
    } catch (e: Exception) {
      Log.e("AndroidMediaPlugin", "openFolder failed: ${e.message}", e)
      invoke.reject("フォルダの表示に失敗しました: ${e.message}")
    }
  }

  /**
   * ファイルを安全に削除する（Scoped Storage / MediaStore フォールバック対応）
   *
   * @param invoke Tauri Invoke ハンドラ
   */
  @Command
  fun trashFile(invoke: Invoke) {
    try {
      val args = invoke.parseArgs(TrashFileArgs::class.java)
      if (args.path.isBlank()) {
        invoke.reject("対象ファイルパスが空です")
        return
      }

      val file = File(args.path)
      var deleted = false

      // 1. 通常の File.delete() 試行
      if (file.exists()) {
        deleted = file.delete()
      }

      // 2. 失敗時は MediaStore 経由での削除を試行 (Scoped Storage 対応)
      if (!deleted && file.exists()) {
        try {
          val where = "${MediaStore.MediaColumns.DATA} = ?"
          val whereArgs = arrayOf(file.absolutePath)
          val countImages = activity.contentResolver.delete(
            MediaStore.Images.Media.EXTERNAL_CONTENT_URI,
            where,
            whereArgs
          )
          val countFiles = activity.contentResolver.delete(
            MediaStore.Files.getContentUri("external"),
            where,
            whereArgs
          )
          deleted = countImages > 0 || countFiles > 0 || !file.exists()
        } catch (e: Exception) {
          Log.w("AndroidMediaPlugin", "MediaStore delete error: ${e.message}")
        }
      }

      // メディアスキャナー同期（ギャラリーからの除去を反映）
      MediaScannerConnection.scanFile(activity, arrayOf(args.path), null, null)

      if (deleted || !file.exists()) {
        invoke.resolve()
      } else {
        invoke.reject("ファイルの削除権限がありませんでした")
      }
    } catch (e: Exception) {
      Log.e("AndroidMediaPlugin", "trashFile failed: ${e.message}", e)
      invoke.reject("ファイルの削除に失敗しました: ${e.message}")
    }
  }
}
