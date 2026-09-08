package com.wddjwk.kxtodo

import android.os.Bundle
import android.webkit.WebView
import androidx.activity.OnBackPressedCallback
import androidx.activity.enableEdgeToEdge

class MainActivity : TauriActivity() {
  private var webViewRef: WebView? = null

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    // 生成的 TauriActivity 将 handleBackNavigation 置为 false，不会注册竞争回调。
    // 硬件返回键：先问前端覆盖层（搜索态等，window.kxtodoBackHandler）要不要消费；
    // 不消费才回退 WebView 历史（前端移动端路由的层级条目），否则结束 Activity。
    // 搜索态刻意不往历史栈压层：pushState 条目进 WebView 的会话恢复后，
    // 「搜索过再退出、重开必闪退一次」就是从那来的。
    onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
      override fun handleOnBackPressed() {
        val wv = webViewRef
        if (wv == null) {
          finish()
          return
        }
        wv.evaluateJavascript(
          "(function(){try{return !!(window.kxtodoBackHandler && window.kxtodoBackHandler());}catch(e){return false;}})()"
        ) { consumed ->
          if (consumed == "true") return@evaluateJavascript
          runOnUiThread {
            if (wv.canGoBack()) {
              wv.goBack()
            } else {
              finish()
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
}
