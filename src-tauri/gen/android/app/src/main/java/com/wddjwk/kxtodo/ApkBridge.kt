package com.wddjwk.kxtodo

import android.content.ClipData
import android.content.ClipDescription
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.Uri
import android.os.Build
import android.provider.OpenableColumns
import android.webkit.JavascriptInterface
import android.webkit.MimeTypeMap
import androidx.core.content.FileProvider
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.util.UUID

/**
 * JS 桥（window.kxtodoAndroid）：APK 安装、系统分享与剪切板同步。
 * 方法均为同步返回字符串："" 表示成功，非空为错误信息。
 *
 * **每个方法都要求 token**：Android WebView 的 addJavascriptInterface 会注入到所有
 * iframe，应用内链接预览打开外部网页时，远端页面也能摸到这个桥。token 由 MainActivity
 * 只往主 frame 里写（evaluateJavascript），外部页面跨源读不到父窗口的变量，于是
 * 拿不到 token，也就不能读剪切板/安装 APK/分享文件。
 *
 * FileProvider authority 与 AndroidManifest 中声明的 `${applicationId}.fileprovider` 一致，
 * res/xml/file_paths.xml 已含 cache-path、external-files-path。
 */
class ApkBridge(private val context: Context) {
  private val token = UUID.randomUUID().toString()

  /** 只给 MainActivity 用（不带 @JavascriptInterface，网页调不到）。 */
  fun tokenForHost(): String = token

  private fun authorized(candidate: String): Boolean =
    candidate.isNotEmpty() && candidate == token

  @JavascriptInterface
  fun installApk(token: String, path: String): String {
    if (!authorized(token)) return "未授权"
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
  fun shareText(token: String, filename: String, mime: String, text: String): String {
    if (!authorized(token)) return "未授权"
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

  @JavascriptInterface
  fun shareFile(token: String, path: String, mime: String): String {
    if (!authorized(token)) return "未授权"
    return try {
      val cacheDir = context.cacheDir.canonicalFile
      val target = File(path).canonicalFile
      if (!target.path.startsWith(cacheDir.path + File.separator) || !target.isFile) {
        return "文件不在应用缓存目录内"
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

  // -------------------------------------------------------------------------
  // 剪切板同步（v0.8.9）。只在前台被 JS 调用；Android 10+ 背景读剪切板会拿到空值，
  // 所以 MainActivity 只在 onResume 注册 OnPrimaryClipChangedListener。
  // -------------------------------------------------------------------------

  @JavascriptInterface
  fun readClipboard(token: String): String {
    if (!authorized(token)) return JSONObject().put("error", "未授权").toString()
    cleanupOutgoing()
    return try {
      val manager = context.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
        ?: return "{}"
      val clip = manager.primaryClip ?: return "{}"
      var text = ""
      var html = ""
      var imagePath: String? = null
      val files = JSONArray()
      var total = 0L
      val cap = 128L * 1024 * 1024
      for (index in 0 until clip.itemCount) {
        val item = clip.getItemAt(index)
        if (Build.VERSION.SDK_INT >= 24) {
          item.htmlText?.toString()?.let { if (it.isNotBlank()) html = it }
        }
        val uri = item.uri
        if (uri != null) {
          val mime = context.contentResolver.getType(uri) ?: ""
          if (mime.startsWith("image/") && imagePath == null) {
            val copied = copyImage(uri) ?: continue
            imagePath = copied.path
            total += copied.size
          } else {
            val copied = copyFile(uri) ?: continue
            total += copied.size
            if (total > cap) break
            files.put(JSONObject().put("name", copied.name).put("path", copied.path))
          }
        } else {
          item.text?.toString()?.let { if (it.isNotEmpty()) text = it }
        }
      }
      val result = JSONObject()
      if (text.isNotEmpty()) result.put("text", text)
      if (html.isNotEmpty()) result.put("html", html)
      if (imagePath != null) result.put("imagePath", imagePath)
      if (files.length() > 0) result.put("files", files)
      result.toString()
    } catch (error: Exception) {
      JSONObject().put("error", error.message ?: error.toString()).toString()
    }
  }

  @JavascriptInterface
  fun writeClipboard(token: String, payload: String): String {
    if (!authorized(token)) return "未授权"
    return try {
      val json = JSONObject(payload)
      val text = json.optString("text", "")
      val html = json.optString("html", "")
      val imagePath = json.optString("imagePath", "")
      val items = mutableListOf<Pair<Uri, String>>()
      if (imagePath.isNotEmpty()) {
        fileUri(imagePath, "image/png")?.let { items.add(it) }
      }
      val files = json.optJSONArray("files")
      if (files != null) {
        for (index in 0 until files.length()) {
          val path = files.optJSONObject(index)?.optString("path", "") ?: ""
          if (path.isEmpty()) continue
          val extension = File(path).extension.lowercase()
          val mime = MimeTypeMap.getSingleton().getMimeTypeFromExtension(extension)
            ?: "application/octet-stream"
          fileUri(path, mime)?.let { items.add(it) }
        }
      }
      val manager = context.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
        ?: return "剪切板不可用"
      when {
        items.isNotEmpty() -> {
          val mimes = items.map { it.second }.distinct().toTypedArray()
          val clip = ClipData(ClipDescription("KXToDo", mimes), ClipData.Item(items[0].first))
          for (index in 1 until items.size) clip.addItem(ClipData.Item(items[index].first))
          manager.setPrimaryClip(clip)
        }
        html.isNotEmpty() && Build.VERSION.SDK_INT >= 24 ->
          manager.setPrimaryClip(ClipData.newHtmlText("KXToDo", text, html))
        text.isNotEmpty() -> manager.setPrimaryClip(ClipData.newPlainText("KXToDo", text))
      }
      ""
    } catch (error: Exception) {
      error.message ?: error.toString()
    }
  }

  /** 读取剪切板时临时落下的文件只服务一次发送；一小时前的清掉，避免缓存无限长。 */
  private fun cleanupOutgoing() {
    val root = File(context.cacheDir, "clipboard-out")
    val cutoff = System.currentTimeMillis() - 60L * 60L * 1000L
    root.listFiles()?.forEach { entry ->
      if (entry.lastModified() < cutoff) entry.deleteRecursively()
    }
  }

  private data class CopiedFile(val name: String, val path: String, val size: Long)

  private fun allowedRoots(): List<File> = listOfNotNull(
    context.cacheDir,
    context.getExternalFilesDir(null)
  ).map { it.canonicalFile }

  private fun copyFile(uri: Uri): CopiedFile? {
    val name = displayName(uri)
    val dir = File(context.cacheDir, "clipboard-out/${UUID.randomUUID()}").apply { mkdirs() }
    val target = File(dir, sanitizeName(name))
    val size = context.contentResolver.openInputStream(uri)?.use { input ->
      target.outputStream().use { output -> input.copyTo(output) }
    } ?: return null
    return CopiedFile(sanitizeName(name), target.absolutePath, size)
  }

  private fun copyImage(uri: Uri): CopiedFile? {
    val bitmap = context.contentResolver.openInputStream(uri)?.use { BitmapFactory.decodeStream(it) }
      ?: return null
    val dir = File(context.cacheDir, "clipboard-out/${UUID.randomUUID()}").apply { mkdirs() }
    val target = File(dir, "clipboard.png")
    val size = target.outputStream().use { out ->
      bitmap.compress(Bitmap.CompressFormat.PNG, 100, out)
      target.length()
    }
    bitmap.recycle()
    return CopiedFile("clipboard.png", target.absolutePath, size)
  }

  private fun displayName(uri: Uri): String {
    val fallback = uri.lastPathSegment?.substringAfterLast('/')?.takeIf { it.isNotEmpty() } ?: "file"
    return try {
      context.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)
        ?.use { cursor ->
          if (!cursor.moveToFirst()) return@use fallback
          val index = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
          if (index >= 0) cursor.getString(index) ?: fallback else fallback
        } ?: fallback
    } catch (_: Exception) {
      fallback
    }
  }

  private fun sanitizeName(raw: String): String {
    val cleaned = raw.substringAfterLast('/').substringAfterLast('\\')
      .filter { !it.isISOControl() && it !in "<>:\"|?*" }
      .take(120)
      .trim()
      .trim('.')
    return if (cleaned.isEmpty()) "file" else cleaned
  }

  private fun fileUri(path: String, mime: String): Pair<Uri, String>? {
    val target = File(path).canonicalFile
    val allowed = allowedRoots().any { target.path.startsWith(it.path + File.separator) }
    if (!allowed || !target.isFile) return null
    val uri = FileProvider.getUriForFile(
      context,
      context.packageName + ".fileprovider",
      target
    )
    return uri to mime
  }
}
