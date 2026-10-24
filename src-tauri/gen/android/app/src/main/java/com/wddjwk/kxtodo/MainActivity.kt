package com.wddjwk.kxtodo

import android.content.ClipboardManager
import android.content.Context
import android.net.wifi.WifiManager
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.Process
import android.webkit.WebView
import androidx.activity.OnBackPressedCallback
import androidx.activity.enableEdgeToEdge

class MainActivity : TauriActivity() {
  private var webViewRef: WebView? = null
  private var bridgeRef: ApkBridge? = null
  private var clipboardManager: ClipboardManager? = null
  private var clipboardListening = false
  private var multicastLock: WifiManager.MulticastLock? = null
  private var tokenTimer: Runnable? = null
  private var lastBackAt = 0L
  private val mainHandler = Handler(Looper.getMainLooper())

  private val clipboardListener = ClipboardManager.OnPrimaryClipChangedListener {
    notifyClipboardChanged()
  }

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
    val bridge = ApkBridge(applicationContext)
    bridgeRef = bridge
    webView.addJavascriptInterface(bridge, "kxtodoAndroid")
    // token 只写主 frame（evaluateJavascript 的语义）；外部 iframe 跨源读不到父窗口变量。
    // 页面加载与 JS 初始化有先后，重复注入一小段时间，直到主文档稳定下来。
    injectBridgeToken(40)
  }

  override fun onResume() {
    super.onResume()
    acquireMulticastLock()
    startClipboardListening()
    // 常驻服务对账（v0.9.0）：进程被系统重建、或上次是「在后台想拉起前台服务」被
    // Android 12+ 的规则挡掉，都靠回到前台这一次补上。状态存在 SharedPreferences 里，
    // 不依赖 WebView 是否已经起来。
    reconcileKeepAlive()
    injectBridgeToken(40)
    // 切回前台时补一次：后台期间复制的内容（Android 10+ 不让后台读）现在才读得到。
    mainHandler.postDelayed({ notifyClipboardChanged() }, 500)
  }

  private fun reconcileKeepAlive() {
    val app = applicationContext
    if (!KeepAliveService.readEnabled(app)) return
    KeepAliveService.reconcile(app, true, KeepAliveService.readNotice(app))
  }

  override fun onPause() {
    stopClipboardListening()
    releaseMulticastLock()
    super.onPause()
  }

  override fun onDestroy() {
    releaseMulticastLock()
    tokenTimer?.let { mainHandler.removeCallbacks(it) }
    tokenTimer = null
    stopClipboardListening()
    // 销毁链里再进返回回调也不许碰一个正在死的 WebView
    webViewRef?.removeJavascriptInterface("kxtodoAndroid")
    webViewRef = null
    bridgeRef = null
    super.onDestroy()
  }

  private fun injectBridgeToken(attempts: Int) {
    tokenTimer?.let { mainHandler.removeCallbacks(it) }
    if (attempts <= 0) return
    val webView = webViewRef
    val token = bridgeRef?.tokenForHost()
    if (webView != null && token != null) {
      webView.evaluateJavascript("window.__kxtodoAndroidToken='$token';", null)
    }
    val next = Runnable { injectBridgeToken(attempts - 1) }
    tokenTimer = next
    mainHandler.postDelayed(next, 250)
  }

  /** Android Wi-Fi 栈默认过滤组播/广播；局域网剪切板发现需要一把 MulticastLock。 */
  private fun acquireMulticastLock() {
    if (multicastLock != null) return
    val wifi = applicationContext.getSystemService(Context.WIFI_SERVICE) as? WifiManager ?: return
    val lock = wifi.createMulticastLock("kxtodo-clipboard-lan").apply {
      setReferenceCounted(false)
      acquire()
    }
    multicastLock = lock
  }

  private fun releaseMulticastLock() {
    multicastLock?.release()
    multicastLock = null
  }

  private fun startClipboardListening() {
    if (clipboardListening) return
    val manager = applicationContext.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
      ?: return
    clipboardManager = manager
    manager.addPrimaryClipChangedListener(clipboardListener)
    clipboardListening = true
  }

  private fun stopClipboardListening() {
    if (!clipboardListening) return
    clipboardManager?.removePrimaryClipChangedListener(clipboardListener)
    clipboardListening = false
  }

  /** 只报告「变了」，读剪切板由 JS 决定（设置关着时连读都不读，避免系统提示）。 */
  private fun notifyClipboardChanged() {
    if (!clipboardListening) return
    webViewRef?.evaluateJavascript(
      "window.dispatchEvent(new Event('kxtodo-native-clipboard'))",
      null
    )
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
