package com.wddjwk.kxtodo

import android.content.Context
import android.content.Intent
import android.webkit.JavascriptInterface
import androidx.core.content.FileProvider
import java.io.File

/**
 * JS 桥（window.kxtodoAndroid）：APK 安装与系统分享面板。
 * 两个方法均为同步返回字符串："" 表示成功，非空为错误信息。
 * FileProvider authority 与 AndroidManifest 中声明的 `${applicationId}.fileprovider` 一致，
 * res/xml/file_paths.xml 已含 cache-path "."。
 */
class ApkBridge(private val context: Context) {

  @JavascriptInterface
  fun installApk(path: String): String {
    return try {
      if (path.isEmpty()) {
        return "路径为空"
      }
      val cacheDir = context.cacheDir.canonicalFile
      val target = File(path).canonicalFile
      // 只允许安装应用缓存目录内的文件，防目录穿越。
      if (!target.path.startsWith(cacheDir.path + File.separator)) {
        return "路径不在应用缓存目录内"
      }
      if (!target.isFile) {
        return "APK 文件不存在"
      }
      val uri = FileProvider.getUriForFile(
        context,
        context.packageName + ".fileprovider",
        target
      )
      val intent = Intent(Intent.ACTION_VIEW).apply {
        setDataAndType(uri, "application/vnd.android.package-archive")
        addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
      }
      context.startActivity(intent)
      ""
    } catch (error: Exception) {
      error.message ?: error.toString()
    }
  }

  @JavascriptInterface
  fun shareText(filename: String, mime: String, text: String): String {
    return try {
      if (filename.isEmpty()
        || filename.contains('/')
        || filename.contains('\\')
        || filename.contains("..")
      ) {
        return "非法文件名"
      }
      val file = File(context.cacheDir, filename)
      file.writeText(text, Charsets.UTF_8)
      val uri = FileProvider.getUriForFile(
        context,
        context.packageName + ".fileprovider",
        file
      )
      val send = Intent(Intent.ACTION_SEND).apply {
        type = mime
        putExtra(Intent.EXTRA_STREAM, uri)
        putExtra(Intent.EXTRA_SUBJECT, filename)
        addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
      }
      val chooser = Intent.createChooser(send, null).apply {
        addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_GRANT_READ_URI_PERMISSION)
      }
      context.startActivity(chooser)
      ""
    } catch (error: Exception) {
      error.message ?: error.toString()
    }
  }
}
