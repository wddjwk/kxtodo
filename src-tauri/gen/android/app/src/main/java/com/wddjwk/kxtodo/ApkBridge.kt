package com.wddjwk.kxtodo

import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.webkit.JavascriptInterface
import androidx.browser.customtabs.CustomTabsIntent
import androidx.core.content.FileProvider
import java.io.File
import java.net.URI
import java.util.Locale

/**
 * JS 桥（window.kxtodoAndroid）：APK 安装、系统分享与 Custom Tabs。
 * 方法均为同步返回字符串："" 表示成功，非空为错误信息。
 * FileProvider authority 与 AndroidManifest 中声明的 `${applicationId}.fileprovider` 一致，
 * res/xml/file_paths.xml 已含 cache-path "."。
 */
class ApkBridge(private val context: Context) {

  /** Browser-owned activity: remote pages never receive this JS bridge or replace the main WebView. */
  @JavascriptInterface
  fun openCustomTab(url: String): String {
    return try {
      if (url.length > 4096 || url.any { it <= ' ' || it == '\u007f' }) return "非法链接"
      val parsed = URI(url)
      val scheme = parsed.scheme?.lowercase(Locale.ROOT)
      val host = parsed.host?.lowercase(Locale.ROOT)?.trimEnd('.') ?: return "非法链接"
      if (scheme !in listOf("http", "https") || parsed.rawUserInfo != null
        || host == "localhost" || host.endsWith(".localhost")
        || host in listOf("::1", "[::1]", "::", "[::]")
        || host.startsWith("127.") || host.startsWith("0.")
      ) return "只支持外部 http/https 链接"
      val uri = Uri.parse(parsed.toASCIIString())
      val tab = CustomTabsIntent.Builder().setShowTitle(true).build()
      // ApkBridge holds applicationContext, not an Activity.
      tab.intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
      try {
        tab.launchUrl(context, uri)
      } catch (_: ActivityNotFoundException) {
        context.startActivity(Intent(Intent.ACTION_VIEW, uri).apply {
          addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        })
      }
      ""
    } catch (error: Exception) {
      error.message ?: error.toString()
    }
  }

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

  /**
   * 分享一个**已经存在**的文件（日记导出的 zip 由 Rust 写进缓存目录）。
   * 与 installApk 同一套穿越校验：只允许应用缓存目录内的路径。
   */
  @JavascriptInterface
  fun shareFile(path: String, mime: String): String {
    return try {
      if (path.isEmpty()) {
        return "路径为空"
      }
      val cacheDir = context.cacheDir.canonicalFile
      val target = File(path).canonicalFile
      if (!target.path.startsWith(cacheDir.path + File.separator)) {
        return "路径不在应用缓存目录内"
      }
      if (!target.isFile) {
        return "文件不存在"
      }
      val uri = FileProvider.getUriForFile(
        context,
        context.packageName + ".fileprovider",
        target
      )
      val send = Intent(Intent.ACTION_SEND).apply {
        type = mime
        putExtra(Intent.EXTRA_STREAM, uri)
        putExtra(Intent.EXTRA_SUBJECT, target.name)
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
