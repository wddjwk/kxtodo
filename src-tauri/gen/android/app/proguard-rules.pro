# Add project specific ProGuard rules here.
# You can control the set of applied configuration files using the
# proguardFiles setting in build.gradle.
#
# For more details, see
#   http://developer.android.com/guide/developing/tools/proguard.html

# If your project uses WebView with JS, uncomment the following
# and specify the fully qualified class name to the JavaScript interface
# class:
#-keepclassmembers class fqcn.of.javascript.interface.for.webview {
#   public *;
#}

# Uncomment this to preserve the line number information for
# debugging stack traces.
#-keepattributes SourceFile,LineNumberTable

# If you keep the line number information, uncomment this to
# hide the original source file name.
#-renamesourcefileattribute SourceFile

# KXToDo JS 桥（window.kxtodoAndroid）：release 构建启用 minify，
# WebView 反射调用 @JavascriptInterface 方法，必须保留类与方法。
-keep class com.wddjwk.kxtodo.ApkBridge { *; }
-keepclassmembers class com.wddjwk.kxtodo.ApkBridge {
    @android.webkit.JavascriptInterface <methods>;
}
