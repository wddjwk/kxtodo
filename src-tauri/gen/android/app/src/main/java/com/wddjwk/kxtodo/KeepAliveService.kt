package com.wddjwk.kxtodo

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat

/**
 * 后台常驻（v0.9.0）：一条前台服务 + 一条常驻通知，把进程留在前台组，好让 Rust 侧
 * 的 500ms 调度/提醒节拍与同步引擎在应用退到后台之后继续跑。
 *
 * 它**不做任何业务**：不调度、不轮询、不持锁，只是一张「进程还活着」的门票。
 * 系统仍可能因为电量优化或厂商省电策略把进程收掉——这不是本服务能承诺的事，
 * 设置页也照实说明。
 *
 * 标语存在 SharedPreferences 里：被系统重建（START_STICKY）时 WebView 可能还没起来，
 * 服务必须自己能读到要显示什么。
 */
class KeepAliveService : Service() {

  override fun onBind(intent: Intent?): IBinder? = null

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    if (intent?.action == ACTION_STOP) {
      running = false
      ServiceCompat.stopForeground(this, ServiceCompat.STOP_FOREGROUND_REMOVE)
      stopSelf()
      return START_NOT_STICKY
    }
    val notice = intent?.getStringExtra(EXTRA_NOTICE)?.takeIf { it.isNotBlank() } ?: readNotice(this)
    postForeground(notice)
    running = true
    // 被系统收掉后允许重建；标语从 SharedPreferences 读回，不依赖 WebView 还活着
    return START_STICKY
  }

  override fun onDestroy() {
    running = false
    super.onDestroy()
  }

  private fun postForeground(notice: String) {
    val manager = getSystemService(Context.NOTIFICATION_SERVICE) as? NotificationManager ?: return
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
      val channel = NotificationChannel(
        CHANNEL_ID,
        "后台常驻",
        NotificationManager.IMPORTANCE_LOW
      ).apply {
        description = "让同步与提醒在应用退到后台后继续工作"
        setShowBadge(false)
        enableLights(false)
        enableVibration(false)
      }
      manager.createNotificationChannel(channel)
    }

    val openIntent = Intent(this, MainActivity::class.java).apply {
      action = Intent.ACTION_MAIN
      addCategory(Intent.CATEGORY_LAUNCHER)
    }
    var pendingFlags = PendingIntent.FLAG_UPDATE_CURRENT
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
      pendingFlags = pendingFlags or PendingIntent.FLAG_IMMUTABLE
    }
    val pending = PendingIntent.getActivity(this, 0, openIntent, pendingFlags)

    val notification = NotificationCompat.Builder(this, CHANNEL_ID)
      .setSmallIcon(R.drawable.ic_launcher_foreground)
      .setContentTitle("KXToDo")
      .setContentText(notice)
      .setStyle(NotificationCompat.BigTextStyle().bigText(notice))
      .setOngoing(true)
      .setOnlyAlertOnce(true)
      .setPriority(NotificationCompat.PRIORITY_LOW)
      .setCategory(NotificationCompat.CATEGORY_SERVICE)
      .setContentIntent(pending)
      .build()

    // Android 14+ 必须声明前台服务类型；我们在 manifest 里报的是 specialUse
    val type = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
        ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE
      } else {
        0
      }
    } else {
      0
    }
    ServiceCompat.startForeground(this, NOTIFICATION_ID, notification, type)
  }

  companion object {
    const val ACTION_STOP = "com.wddjwk.kxtodo.keepalive.STOP"
    const val EXTRA_NOTICE = "notice"
    const val CHANNEL_ID = "kxtodo-keep-alive"
    const val NOTIFICATION_ID = 4201
    const val PREFS = "kxtodo-keep-alive"
    const val KEY_ENABLED = "enabled"
    const val KEY_NOTICE = "notice"
    const val DEFAULT_NOTICE = "今天有什么要做的吗~"

    @Volatile
    var running = false
      private set

    fun readNotice(context: Context): String {
      val stored = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        .getString(KEY_NOTICE, null)?.trim().orEmpty()
      return if (stored.isEmpty()) DEFAULT_NOTICE else stored
    }

    fun readEnabled(context: Context): Boolean =
      context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getBoolean(KEY_ENABLED, false)

    fun writeState(context: Context, enabled: Boolean, notice: String) {
      val trimmed = notice.trim().ifEmpty { DEFAULT_NOTICE }
      context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit()
        .putBoolean(KEY_ENABLED, enabled)
        .putString(KEY_NOTICE, trimmed)
        .apply()
    }

    /**
     * 幂等地把服务对齐到设置：开就拉起前台服务，关就发一条 STOP 收掉通知。
     *
     * Android 12+ 不许从后台启动前台服务（抛
     * ForegroundServiceStartNotAllowedException）——那种情况下什么都不做，
     * 下次回到前台由 MainActivity.onResume 再对账一次。
     */
    fun reconcile(context: Context, enabled: Boolean, notice: String) {
      writeState(context, enabled, notice)
      val intent = Intent(context, KeepAliveService::class.java)
      try {
        if (enabled) {
          intent.putExtra(EXTRA_NOTICE, notice.trim().ifEmpty { DEFAULT_NOTICE })
          if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            context.startForegroundService(intent)
          } else {
            context.startService(intent)
          }
        } else {
          intent.action = ACTION_STOP
          context.startService(intent)
        }
      } catch (error: Exception) {
        // 拉不起来（后台启动限制 / 服务被禁）不影响应用本身：常驻只是加分项
        android.util.Log.w("kxtodo", "keep-alive reconcile failed: $error")
      }
    }
  }
}
