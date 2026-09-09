package com.wddjwk.kxtodo

import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.Process
import android.webkit.WebView
import androidx.activity.OnBackPressedCallback
import androidx.activity.enableEdgeToEdge

class MainActivity : TauriActivity() {
  private var webViewRef: WebView? = null
  private var lastBackAt = 0L
  private val mainHandler = Handler(Looper.getMainLooper())

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    // 不做 Activity 状态恢复：应用的全部状态都在磁盘与 JS 侧，wry 的恢复路径在
    // 「进程异常死亡后重开」时会拿着旧 ACTIVITY_ID 去找已经不存在的 webview。
    // 「搜索过/全屏过再退出，重开必闪退一次、第三次正常」就是这条恢复链在作祟——
    // 传 null 让每次启动都是冷启动，崩过一次也不会再崩第二次。
    super.onCreate(null)
    // 生成的 TauriActivity 将 handleBackNavigation 置为 false，不会注册竞争回调。
    // 硬件返回键：先问前端覆盖层（全屏图预览、搜索态等，window.kxtodoBackHandler）
    // 要不要消费；不消费才回退 WebView 历史（前端移动端路由的层级条目），否则退出。
    // 搜索态刻意不往历史栈压层：pushState 条目会进 WebView 的会话恢复。
    onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
      override fun handleOnBackPressed() {
        if (isFinishing) return
        // 连按去抖：一记返回只走一条链，evaluateJavascript 往返期间再按不叠加
        val now = System.currentTimeMillis()
        if (now - lastBackAt < 400) return
        lastBackAt = now
        val wv = webViewRef
        if (wv == null) {
          exitApp()
          return
        }
        wv.evaluateJavascript(
          "(function(){try{return !!(window.kxtodoBackHandler && window.kxtodoBackHandler());}catch(e){return false;}})()"
        ) { consumed ->
          if (consumed == "true") return@evaluateJavascript
          runOnUiThread {
            if (isFinishing) return@runOnUiThread
            if (wv.canGoBack()) {
              wv.goBack()
            } else {
              exitApp()
            }
          }
        }
      }
    })
  }

  override fun onWebViewCreate(webView: WebView) {
    super.onWebViewCreate(webView)
    webViewRef = webView
    webView.addJavascriptInterface(ApkBridge(applicationContext), "kxtodoAndroid")
  }

  override fun onDestroy() {
    // 销毁链里再进返回回调也不许碰一个正在死的 WebView
    webViewRef = null
    super.onDestroy()
  }

  private fun exitApp() {
    webViewRef = null
    // 任务从最近列表移除：异常死亡后不会留下带 savedInstanceState 的任务记录，
    // 下次启动就不会走恢复链再崩一次
    finishAndRemoveTask()
    // 移动端没有常驻 Host/调度/托盘，退出后进程活着只会攥着数据目录锁与同步端口——
    // 「垂死进程」正是重开时撞锁白屏的窗口。给销毁链一点时间跑完再硬退。
    mainHandler.postDelayed({ Process.killProcess(Process.myPid()) }, 250)
  }
}
