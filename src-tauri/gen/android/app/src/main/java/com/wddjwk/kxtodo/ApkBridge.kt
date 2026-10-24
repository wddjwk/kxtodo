package com.wddjwk.kxtodo

import android.app.NotificationManager
import android.content.ActivityNotFoundException
import android.content.ClipData
import android.content.ClipDescription
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.content.ContentValues
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.Uri
import android.os.Build
import android.provider.OpenableColumns
import android.provider.MediaStore
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
          val mime = mimeOf(clip, uri)
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
      // HTML 当文件处理（v0.9.0 bugfix 4.1）：折成一个固定路径的 .html 文件进 files
      // 通道（路径固定 = 同一段剪切板的签名稳定，不会每轮轮询都重发）。
      if (html.isNotEmpty()) {
        spoolHtml(html)?.let { entry ->
          files.put(JSONObject().put("name", entry.first).put("path", entry.second))
        }
      }
      val result = JSONObject()
      if (text.isNotEmpty()) result.put("text", text)
      if (imagePath != null) result.put("imagePath", imagePath)
      if (files.length() > 0) result.put("files", files)
      result.toString()
    } catch (error: Exception) {
      JSONObject().put("error", error.message ?: error.toString()).toString()
    }
  }

  /**
   * URI 的 mime 判定（v0.9.0 bugfix 4.1）：`getType` 对部分提供者为 null（某些
   * FileProvider / 未登记的 media URI），早先 null 就落进文件分支——复制的图片被当成
   * 文件发出去。四级兜底：resolver → 复制方在描述里登记的 mime → 扩展名 → 魔数。
   */
  private fun mimeOf(clip: ClipData, uri: Uri): String {
    context.contentResolver.getType(uri)?.takeIf { it.isNotEmpty() }?.let { return it }
    for (index in 0 until clip.description.mimeTypeCount) {
      clip.description.getMimeType(index)?.takeIf { it.startsWith("image/") }?.let { return it }
    }
    val extension = File(displayName(uri)).extension.lowercase()
    MimeTypeMap.getSingleton().getMimeTypeFromExtension(extension)?.let { return it }
    return sniffImageMime(uri)
  }

  /** 读前 12 字节认魔数：PNG / JPEG / GIF / BMP / WebP。读不到就当普通文件。 */
  private fun sniffImageMime(uri: Uri): String {
    return try {
      context.contentResolver.openInputStream(uri)?.use { input ->
        val head = ByteArray(12)
        var read = 0
        while (read < head.size) {
          val n = input.read(head, read, head.size - read)
          if (n <= 0) break
          read += n
        }
        when {
          read >= 8 && head[0] == 0x89.toByte() && head[1] == 'P'.code.toByte() &&
            head[2] == 'N'.code.toByte() && head[3] == 'G'.code.toByte() -> "image/png"
          read >= 3 && head[0] == 0xFF.toByte() && head[1] == 0xD8.toByte() && head[2] == 0xFF.toByte() -> "image/jpeg"
          read >= 6 && head[0] == 'G'.code.toByte() && head[1] == 'I'.code.toByte() && head[2] == 'F'.code.toByte() -> "image/gif"
          read >= 2 && head[0] == 'B'.code.toByte() && head[1] == 'M'.code.toByte() -> "image/bmp"
          read >= 12 && head[0] == 'R'.code.toByte() && head[8] == 'W'.code.toByte() &&
            head[9] == 'E'.code.toByte() && head[10] == 'B'.code.toByte() && head[11] == 'P'.code.toByte() -> "image/webp"
          else -> ""
        }
      } ?: ""
    } catch (_: Exception) {
      ""
    }
  }

  /** HTML 折成文件：固定路径覆盖写，返回 (名字, 绝对路径)。写失败返回 null（宁可不发）。 */
  private fun spoolHtml(html: String): Pair<String, String>? {
    return try {
      val dir = File(context.cacheDir, "clipboard-out").apply { mkdirs() }
      val target = File(dir, "clipboard.html")
      target.writeText(html, Charsets.UTF_8)
      target.name to target.absolutePath
    } catch (_: Exception) {
      null
    }
  }

  @JavascriptInterface
  fun writeClipboard(token: String, payload: String): String {
    if (!authorized(token)) return "未授权"
    return try {
      val json = JSONObject(payload)
      val text = json.optString("text", "")
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
        text.isNotEmpty() -> manager.setPrimaryClip(ClipData.newPlainText("KXToDo", text))
      }
      ""
    } catch (error: Exception) {
      error.message ?: error.toString()
    }
  }

  /**
   * 后台常驻（v0.9.0）：把前台服务对齐到设置。开关与标语都由前端从设置里读出来传，
   * 服务侧只是照办；这样「设置是唯一真源」，Kotlin 不重复存一份语义。
   * 返回 "" 表示成功，非空为错误信息。
   */  @JavascriptInterface
  fun setKeepAlive(token: String, enabled: Boolean, notice: String): String {
    if (!authorized(token)) return "未授权"
    return try {
      KeepAliveService.reconcile(context, enabled, notice)
      ""
    } catch (error: Exception) {
      error.message ?: error.toString()
    }
  }

  /** 常驻状态：{enabled, running, notice, notificationsAllowed}。JSON 字符串。 */
  @JavascriptInterface
  fun keepAliveState(token: String): String {
    if (!authorized(token)) return JSONObject().put("error", "未授权").toString()
    return try {
      val manager = context.getSystemService(Context.NOTIFICATION_SERVICE) as? NotificationManager
      val allowed = if (Build.VERSION.SDK_INT >= 24) manager?.areNotificationsEnabled() == true else true
      JSONObject()
        .put("enabled", KeepAliveService.readEnabled(context))
        .put("running", KeepAliveService.running)
        .put("notice", KeepAliveService.readNotice(context))
        .put("notificationsAllowed", allowed)
        .toString()
    } catch (error: Exception) {
      JSONObject().put("error", error.message ?: error.toString()).toString()
    }
  }

  /**
   * 剪切板保存位置的候选目录（v0.9.0 bugfix 4.2）。Android 的作用域存储只允许应用
   * 用**路径**写自己的外部文件目录，所以「选择路径」给的就是这几个真实可写的目录
   * （文件管理器里也看得见），不给选不到的公共目录。JSON 数组 [{label, path}]。
   */
  @JavascriptInterface
  fun listSaveDirs(token: String): String {
    if (!authorized(token)) return JSONObject().put("error", "未授权").toString()
    val out = JSONArray()
    val entries = listOf(
      "下载" to android.os.Environment.DIRECTORY_DOWNLOADS,
      "文档" to android.os.Environment.DIRECTORY_DOCUMENTS,
      "图片" to android.os.Environment.DIRECTORY_PICTURES,
      "影片" to android.os.Environment.DIRECTORY_MOVIES
    )
    for ((label, kind) in entries) {
      val dir = context.getExternalFilesDir(kind)?.resolve("kxtodo-clipboard") ?: continue
      out.put(JSONObject().put("label", label).put("path", dir.absolutePath))
    }
    return out.toString()
  }

  /**
   * 打开保存目录（v0.9.0 bugfix 4.2，尽力而为）：
   * ① 图片目录直接开相册集合（MediaStore 的 Images 集合永远有 Activity 接，最稳）；
   * ② 其余目录把 FileProvider 的目录 URI 交给 DocumentsUI / 文件管理器；
   * ③ 没人接就返回错误文案，前端弹 Toast——安卓上「打开文件夹」没有桌面那种保证。
   */
  @JavascriptInterface
  fun openFolder(token: String, path: String): String {
    if (!authorized(token)) return "未授权"
    return try {
      val dir = File(path).canonicalFile
      if (!dir.isDirectory) return "目录不存在"
      if (dir.path.contains("${File.separator}Pictures${File.separator}")) {
        val gallery = Intent(Intent.ACTION_VIEW, MediaStore.Images.Media.EXTERNAL_CONTENT_URI)
        context.startActivity(gallery.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        return ""
      }
      val uri = FileProvider.getUriForFile(
        context,
        context.packageName + ".fileprovider",
        dir
      )
      val intent = Intent(Intent.ACTION_VIEW).apply {
        setDataAndType(uri, "vnd.android.document/directory")
        addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
      }
      try {
        context.startActivity(intent)
        ""
      } catch (missing: ActivityNotFoundException) {
        "没有能打开目录的应用：${dir.absolutePath}"
      }
    } catch (error: Exception) {
      error.message ?: error.toString()
    }
  }

  /**
   * 收到的图片另存一份进相册（v0.9.0 bugfix 4.2）：Android 10+ 走 MediaStore 插入
   * `Pictures/kxtodo`（不需要任何权限，相册立刻可见）；9 及以下写公共 Pictures 目录，
   * 需要运行时写存储权限——没给就返回「需要存储权限」，前端申请后重试。
   */
  @JavascriptInterface
  fun exportImageToGallery(token: String, path: String): String {
    if (!authorized(token)) return "未授权"
    return try {
      val source = File(path)
      if (!source.isFile) return "图片不存在"
      if (Build.VERSION.SDK_INT >= 29) {
        val resolver = context.contentResolver
        val values = ContentValues().apply {
          put(MediaStore.Images.Media.DISPLAY_NAME, source.name)
          put(MediaStore.Images.Media.MIME_TYPE, "image/png")
          put(
            MediaStore.Images.Media.RELATIVE_PATH,
            "${android.os.Environment.DIRECTORY_PICTURES}/kxtodo"
          )
          put(MediaStore.Images.Media.IS_PENDING, 1)
        }
        val uri = resolver.insert(MediaStore.Images.Media.EXTERNAL_CONTENT_URI, values)
          ?: return "相册写入失败"
        resolver.openOutputStream(uri)?.use { out -> source.inputStream().use { it.copyTo(out) } }
          ?: return "相册写入失败"
        values.clear()
        values.put(MediaStore.Images.Media.IS_PENDING, 0)
        resolver.update(uri, values, null, null)
        ""
      } else {
        if (androidx.core.content.ContextCompat.checkSelfPermission(
            context,
            android.Manifest.permission.WRITE_EXTERNAL_STORAGE
          ) != android.content.pm.PackageManager.PERMISSION_GRANTED
        ) {
          return "需要存储权限"
        }
        @Suppress("DEPRECATION")
        val dir = File(
          android.os.Environment.getExternalStoragePublicDirectory(android.os.Environment.DIRECTORY_PICTURES),
          "kxtodo"
        ).apply { mkdirs() }
        source.copyTo(File(dir, source.name), overwrite = true)
        // 让老系统的媒体扫描器看见新文件
        context.sendBroadcast(
          Intent(Intent.ACTION_MEDIA_SCANNER_SCAN_FILE, Uri.fromFile(File(dir, source.name)))
        )
        ""
      }
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
