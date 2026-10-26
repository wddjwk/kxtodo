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
import android.provider.DocumentsContract
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
      var dropped = 0
      val cap = 128L * 1024 * 1024
      for (index in 0 until clip.itemCount) {
        val item = clip.getItemAt(index)
        if (Build.VERSION.SDK_INT >= 24) {
          item.htmlText?.toString()?.let { if (it.isNotBlank()) html = it }
        }
        // 文本第一份为准；带 uri 的项也可能同时带文本（分享的说明文字），别丢。
        if (text.isEmpty()) {
          item.text?.toString()?.let { if (it.isNotEmpty()) text = it }
        }
        val uri = item.uri ?: continue
        val mime = mimeOf(clip, uri)
        if (mime.startsWith("image/")) {
          if (imagePath != null) {
            // 多图剪切板：第一张走图片档，其余按文件走（宁可作为文件也不丢）。
            val copied = copyFile(uri)
            if (copied == null) dropped++
            else {
              total += copied.size
              if (total > cap) break
              files.put(JSONObject().put("name", copied.name).put("path", copied.path))
            }
          } else {
            val copied = copyImage(uri, mime)
            if (copied == null) {
              // 解不出位图也别丢：按普通文件走文件档（v0.9.1 之前是 continue 静默吞掉）。
              val asFile = copyFile(uri)
              if (asFile == null) dropped++
              else {
                total += asFile.size
                if (total > cap) break
                files.put(JSONObject().put("name", asFile.name).put("path", asFile.path))
              }
            } else {
              imagePath = copied.path
              total += copied.size
            }
          }
        } else {
          val copied = copyFile(uri)
          if (copied == null) dropped++
          else {
            total += copied.size
            if (total > cap) break
            files.put(JSONObject().put("name", copied.name).put("path", copied.path))
          }
        }
      }
      // HTML 当文件处理（v0.9.0 bugfix 4.1）：折成一个固定路径的 .html 文件进 files
      // 通道（路径固定 = 同一段剪切板的签名稳定，不会每轮轮询都重发）。
      // 已经有图片时不折（v0.9.1）：图文混合的复制里 html 只是包装纸，折成文件会让
      // 桌面接收端「文件优先」把图挤掉——和桌面 read() 的同一处修复对齐。
      if (html.isNotEmpty() && imagePath == null) {
        spoolHtml(html)?.let { entry ->
          files.put(JSONObject().put("name", entry.first).put("path", entry.second))
        }
      }
      val result = JSONObject()
      if (text.isNotEmpty()) result.put("text", text)
      if (imagePath != null) result.put("imagePath", imagePath)
      if (files.length() > 0) result.put("files", files)
      if (dropped > 0) result.put("dropped", dropped)
      result.toString()
    } catch (error: Exception) {
      JSONObject().put("error", error.message ?: error.toString()).toString()
    }
  }

  /**
   * URI 的 mime 判定（v0.9.0 bugfix 4.1）：`getType` 对部分提供者为 null（某些
   * FileProvider / 未登记的 media URI），早先 null 就落进文件分支——复制的图片被当成
   * 文件发出去。四级兜底：resolver → 复制方在描述里登记的 mime → 扩展名 → 魔数。
   *
   * ClipDescription 一级只在**单项剪切板**时用（v0.9.1）：多项剪切板的描述里混着
   * 所有项的 mime，拿它给「getType 为 null 的那一项」定性会把非图片项误判成图片，
   * 位图解码失败后整项被静默丢掉。
   */
  private fun mimeOf(clip: ClipData, uri: Uri): String {
    context.contentResolver.getType(uri)?.takeIf { it.isNotEmpty() }?.let { return it }
    if (clip.itemCount == 1) {
      for (index in 0 until clip.description.mimeTypeCount) {
        clip.description.getMimeType(index)?.takeIf { it.startsWith("image/") }?.let { return it }
      }
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
      var imageFailed = false
      if (imagePath.isNotEmpty()) {
        val imageItem = fileUri(imagePath, "image/png")
        if (imageItem != null) items.add(imageItem) else imageFailed = true
      }
      var filesFailed = 0
      val files = json.optJSONArray("files")
      if (files != null) {
        for (index in 0 until files.length()) {
          val path = files.optJSONObject(index)?.optString("path", "") ?: ""
          if (path.isEmpty()) continue
          val extension = File(path).extension.lowercase()
          val mime = MimeTypeMap.getSingleton().getMimeTypeFromExtension(extension)
            ?: "application/octet-stream"
          val item = fileUri(path, mime)
          if (item != null) items.add(item) else filesFailed++
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
      // 有内容没写进去就必须报错让前端弹 Toast，绝不静默（v0.9.1 之前：收到的图片
      // 落在内部数据目录 runtime/clipboard-inbox，过不了 fileUri 的白名单，整个被
      // 跳过还返回 ""——「安卓收不到图」的最大单点）。
      val problems = mutableListOf<String>()
      if (imageFailed) problems.add("图片")
      if (filesFailed > 0) problems.add("$filesFailed 个文件")
      if (problems.isNotEmpty()) {
        val wrote = items.isNotEmpty() || text.isNotEmpty()
        return problems.joinToString("、") + "没能写进剪切板" +
          if (wrote) "（其余内容已写入）" else ""
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
   * 打开保存目录（v0.9.1 重写，尽力而为）：
   * ① 配置目录在应用外部文件目录或公共外部存储下 → 构造 DocumentsUI 的文档 URI，
   *    打开的就是**这个目录本身**；
   * ② DocumentsUI 不接（没有可用 Activity）→ 退 FileProvider 目录 URI，第三方文件
   *    管理器可能接；
   * ③ 都不行（内部数据目录等打不开的位置）→ 返回错误文案，前端弹 Toast 说明，
   *    绝不再打开别的地方充数。
   *
   * v0.9.0 的实现有两处「打开的不是配置的目录」：路径含 Pictures 就特判成
   * MediaStore 的**全局图片集合**（那是相册，不是配置的文件夹）；其余把 FileProvider
   * 目录 URI 直接丢给 ACTION_VIEW，系统 DocumentsUI 不认这种 URI，落到哪个界面全看
   * 设备上装了什么。
   */
  @JavascriptInterface
  fun openFolder(token: String, path: String): String {
    if (!authorized(token)) return "未授权"
    return try {
      val dir = File(path).canonicalFile
      if (!dir.isDirectory) return "目录不存在：${dir.absolutePath}"
      documentsUriFor(dir)?.let { uri ->
        if (startOpenDirectory(uri)) return ""
      }
      val roots = listOfNotNull(context.getExternalFilesDir(null), context.cacheDir)
        .map { it.canonicalFile }
      if (roots.any { dir == it || dir.path.startsWith(it.path + File.separator) }) {
        val uri = FileProvider.getUriForFile(
          context,
          context.packageName + ".fileprovider",
          dir
        )
        if (startOpenDirectory(uri)) return ""
      }
      "安卓限制：打不开这个目录（${dir.absolutePath}）"
    } catch (error: Exception) {
      error.message ?: error.toString()
    }
  }

  private fun startOpenDirectory(uri: Uri): Boolean {
    return try {
      val intent = Intent(Intent.ACTION_VIEW).apply {
        setDataAndType(uri, "vnd.android.document/directory")
        addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
      }
      context.startActivity(intent)
      true
    } catch (_: ActivityNotFoundException) {
      false
    } catch (_: SecurityException) {
      false
    }
  }

  /**
   * 目录 → DocumentsUI 文档 URI。应用外部文件目录映射成
   * `primary:Android/data/<包名>/files/…`，公共外部存储映射成 `primary:…`；
   * 其余位置（内部数据目录等）SAF 根本表达不了，返回 null。
   */
  private fun documentsUriFor(dir: File): Uri? {
    val authority = "com.android.externalstorage.documents"
    val appFiles = context.getExternalFilesDir(null)?.canonicalFile
    val publicRoot = try {
      android.os.Environment.getExternalStorageDirectory().canonicalFile
    } catch (_: Exception) {
      null
    }
    val docId = when {
      appFiles != null && (dir == appFiles || dir.path.startsWith(appFiles.path + File.separator)) ->
        "primary:Android/data/${context.packageName}/files" + relativeSuffix(appFiles, dir)
      publicRoot != null && dir.path.startsWith(publicRoot.path + File.separator) ->
        "primary" + relativeSuffix(publicRoot, dir)
      else -> return null
    }
    return try {
      val tree = DocumentsContract.buildTreeDocumentUri(authority, docId)
      DocumentsContract.buildDocumentUriUsingTree(tree, docId)
    } catch (_: Exception) {
      null
    }
  }

  private fun relativeSuffix(root: File, dir: File): String {
    if (dir == root) return ""
    val relative = dir.relativeTo(root).invariantSeparatorsPath
    return if (relative.isEmpty()) "" else "/$relative"
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

  /**
   * 把剪切板图片落成缓存里的 PNG。
   *
   * 本来就是 PNG 的**直接拷字节，不解码重编**（v0.9.1）：重编让「同一段剪切板」每次
   * 读出的字节都不一样，core 的内容签名跟着不稳——最直接的后果是收到桌面发来的图片
   * 写回剪切板后，下一次前台读回的签名对不上 last_local，整张图又被回声发回去；
   * 也让「同一张图连复制两次」的去重失效。JPEG 等其它格式仍解码转 PNG
   * （线上约定 image 段就是 PNG，桌面接收端按 PNG 解码）。
   * 解码失败返回 null，由调用方退回文件档（绝不静默丢项）。
   */
  private fun copyImage(uri: Uri, mime: String): CopiedFile? {
    return try {
      val dir = File(context.cacheDir, "clipboard-out/${UUID.randomUUID()}").apply { mkdirs() }
      val target = File(dir, "clipboard.png")
      if (mime.equals("image/png", ignoreCase = true)) {
        val size = context.contentResolver.openInputStream(uri)?.use { input ->
          target.outputStream().use { output -> input.copyTo(output) }
        } ?: return null
        return CopiedFile("clipboard.png", target.absolutePath, size)
      }
      val bitmap = context.contentResolver.openInputStream(uri)?.use { BitmapFactory.decodeStream(it) }
        ?: return null
      target.outputStream().use { out -> bitmap.compress(Bitmap.CompressFormat.PNG, 100, out) }
      val size = target.length()
      bitmap.recycle()
      CopiedFile("clipboard.png", target.absolutePath, size)
    } catch (_: Exception) {
      null
    }
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
    if (!target.isFile) return null
    val allowed = allowedRoots().any { target.path.startsWith(it.path + File.separator) }
    val effective = if (allowed) {
      target
    } else {
      // FileProvider 暴露不了的路径（core 把收到的图片落在内部数据目录的
      // runtime/clipboard-inbox，不在 cache-path / external-files-path 里）：
      // 先拷进缓存目录再发 URI。v0.9.1 之前这里直接返回 null——收到的图片
      // 从来没写进过安卓剪切板，而且没有任何提示。
      val dir = File(context.cacheDir, "clipboard-out/${UUID.randomUUID()}").apply { mkdirs() }
      val copy = File(dir, sanitizeName(target.name))
      try {
        target.copyTo(copy, overwrite = true)
      } catch (_: Exception) {
        return null
      }
    }
    val uri = FileProvider.getUriForFile(
      context,
      context.packageName + ".fileprovider",
      effective
    )
    return uri to mime
  }
}
