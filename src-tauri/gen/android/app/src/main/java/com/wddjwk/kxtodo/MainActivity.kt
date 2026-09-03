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
    // 硬件返回键：WebView 有历史（前端移动端路由 pushState 的层级条目）则回退，
    // 否则结束 Activity。
    onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
      override fun handleOnBackPressed() {
        val wv = webViewRef
        if (wv != null && wv.canGoBack()) {
          wv.goBack()
        } else {
          finish()
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
