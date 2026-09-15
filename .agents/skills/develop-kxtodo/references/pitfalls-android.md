# Android 开发与构建经验（v0.2.0 实战沉淀）

> **这份文件是什么**：Android（APK）端的构建与开发经验 20 条——gen/android 的所有权、返回键、Kotlin 桥（JS→原生）、tauri-plugin-dialog 的 content:// URI、触摸长按语义、坐标与缩放、模块循环 TDZ 白屏、用 Playwright 模拟移动端做 UX 验证、签名与升级、APK 产物策略、图标同步、通知、能力门控优先于 isMobile、`isMobile` 是 writable store、夹行只写 `-webkit-` 三件套、ContextMenu 限高、app-shell transform 缩放影响一切 rect、首屏量尺寸全是 0、软键盘 resize+scroll 两连击、菜单限高不能顶到视口顶部。
> **什么时候读它**：要构建 APK、改 `src-tauri/gen/android/` 下任何东西、写 Kotlin 桥、改移动端手势/浮层/菜单/测量逻辑、没有真机要验证移动端 UX、APK 签名或升级链出问题。
> 移动端**界面**细节（三级导航、返回键桥、卡片手势、下拉同步、钻入式二级菜单、链接预览标题栏、安全区…）在 `ui-patterns.md`「移动端（Android）」；能力门控层在 `frontend.md`「capabilities.ts」；Android 同栈的进程模型在 `architecture.md`「进程拓扑」。

## 构建环境与入口

本机环境：`ANDROID_HOME=D:\software\Android\sdk`、`NDK_HOME=...\ndk\30.0.14904198`、JDK 23（keytool 在 PATH）。构建唯一入口 `.\release.ps1 android|all`（内部 gradle + cargo-ndk 自配工具链）；**不要手工跑 gradlew**。

## 目录

- [1. gen/android 的所有权](#1-genandroid-的所有权)
- [2. 返回键](#2-返回键)
- [3. Kotlin 桥（JS→原生）模式](#3-kotlin-桥js原生模式)
- [4. tauri-plugin-dialog 在 Android 的 save() 返回 content:// URI](#4-tauri-plugin-dialog-在-android-的-save-返回-content-uri)
- [5. 触摸长按语义](#5-触摸长按语义)
- [6. 坐标与缩放](#6-坐标与缩放)
- [7. 模块循环 TDZ 白屏](#7-模块循环-tdz-白屏)
- [8. 移动端 UX 验证没有真机就用 Playwright 模拟](#8-移动端-ux-验证没有真机就用-playwright-模拟)
- [9. 签名与升级](#9-签名与升级)
- [10. APK 产物策略](#10-apk-产物策略)
- [11. 图标同步](#11-图标同步)
- [12. 通知](#12-通知)
- [13. 能力门控优先于 isMobile 散落判断](#13-能力门控优先于-ismobile-散落判断)
- [14. `isMobile` 是 writable store，不是布尔](#14-ismobile-是-writable-store不是布尔)
- [15. 夹行只写 `-webkit-` 三件套](#15-夹行只写--webkit--三件套)
- [16. ContextMenu 重新收敛时别量自己的 rect 高度](#16-contextmenu-重新收敛时别量自己的-rect-高度)
- [17. app-shell 的 transform 缩放影响一切 getBoundingClientRect](#17-app-shell-的-transform-缩放影响一切-getboundingclientrect)
- [18. 首屏量尺寸全是 0](#18-首屏量尺寸全是-0)
- [19. 软键盘 = resize + scroll 两连击](#19-软键盘--resize--scroll-两连击)
- [20. 菜单限高绝不能把菜单顶到视口顶部](#20-菜单限高绝不能把菜单顶到视口顶部)

## 二十条经验

### 1. gen/android 的所有权

`app/build.gradle.kts`、`app/src/main/**`（Manifest、Kotlin、res）、`app/proguard-rules.pro` 是用户文件可改；`generated/`、`tauri.build.gradle.kts`、`build/` 每次构建再生成，别改。`tauri.properties` 会被 tauri CLI 再生成且内容可能过期（曾残留 versionName 8.2.1）——**版本一律走 package.ps1 注入的 `KXTODO_VERSION` 环境变量**，别信 tauri.properties。

### 2. 返回键

生成的 `TauriActivity` 把 `handleBackNavigation` 固定为 false（webview 历史不接管），必须在 `MainActivity.onCreate` 自己注册 `OnBackPressedCallback`（canGoBack→goBack 否则 finish），否则硬件返回直接退应用。goBack 触发的 popstate 由 platform.ts 路由消费。

### 3. Kotlin 桥（JS→原生）模式

`MainActivity.onWebViewCreate` 里 `addJavascriptInterface`；方法标 `@JavascriptInterface`、同步返回 `""`=成功/错误串；**release 开了 minify，必须在 proguard-rules.pro 加 keep 规则**否则桥方法被摇掉。文件分享/安装走 FileProvider：authority = `packageName + ".fileprovider"`（Manifest 已声明，`res/xml/file_paths.xml` 的 cache-path "." 覆盖 cacheDir）；installApk 前校验路径 canonical 后落在 cacheDir 内。

### 4. tauri-plugin-dialog 在 Android 的 save() 返回 content:// URI

，Rust `fs::write` 写不了——移动端导出/落盘一律改 Kotlin 桥（shareText 写 cacheDir + ACTION_SEND）或 `<input type=file>` + dataURL 命令；导入用 file input 的 `file.text()`。

### 5. 触摸长按语义

Chromium/WebView 在触摸长按后会补发原生 `contextmenu`，自定义长按 handler 要用抑制窗（longpress.ts 的 `isLongPressSuppressed`）去重，否则一次手势开两次菜单；嵌套容器（行 + 外层 nav）会各武装一个长按定时器，外层 handler 必须检查抑制标志/内层菜单已开，否则行菜单被空白区菜单顶替。长按抬手补发的 click 也要吞掉，否则会冒泡关掉刚开的菜单。文本选中放大镜靠 mobile.css 的 `user-select:none + -webkit-touch-callout:none` 抑制。

### 6. 坐标与缩放

app-shell 是 transform 缩放的，`position:fixed` 子元素（菜单/浮层）的坐标系跟着缩放——所有用 clientX/Y 定位的浮层都要除以 `uiScaleValue()` 换算逻辑坐标（TaskCard 日期弹窗、ContextMenu 同套路）；移动端开启界面缩放后这条对所有浮层生效。

### 7. 模块循环 TDZ 白屏

platform↔stores/backend/capabilities 存在循环依赖，任何在模块顶层订阅 stores 的代码（如历史栈路由）会在启动时 ReferenceError 白屏（桌面+移动全平台）。订阅类初始化必须封装成函数由 App onMount 调用；验证手段：`node` 直接 eval 生产 bundle（配 DOM stub）能复现 TDZ，比肉眼看代码快。

### 8. 移动端 UX 验证没有真机就用 Playwright 模拟

`scripts/mobile-ux-test.mjs`（playwright-core + `channel:"msedge"`，Android UA + hasTouch + isMobile context 连 vite dev）。合成 PointerEvent（pointerType:"touch"）可驱动长按 action；浏览器 dev 走 localStorage legacy 路径，足够验证导航/菜单/设置页等纯前端逻辑。APK 原生侧（Kotlin 桥、安装器、返回键）只能靠代码评审 + `aapt dump badging` / `apksigner verify --print-certs` 核验产物元数据与签名。

### 9. 签名与升级

keystore 在 `src-tauri/gen/android/keystore/release.jks`（gitignore，密码 kxtodo，package.ps1 首跑自动生成；同一份已 base64 存仓库 secret `ANDROID_KEYSTORE_BASE64` 供 release.yml 云端签名，兼作异地备份）——**丢了它旧装就无法覆盖升级**。versionCode 公式 `900000000 + X*1000000 + Y*1000 + Z`（gradle 内），基线高于历史脏值 8002001；改公式前先确认单调递增，否则安装器报降级拒装。

### 10. APK 产物策略

release 资产固定名 `KXToDo.apk`（不带版本，覆盖安装不留历史包）；应用内更新 = 下载固定名到 cacheDir → 桥接 installApk 拉系统安装器。桌面（Windows/Linux）更新同为“下载固定名制品→替换→重启”，不再做 shim/带版本旧包/更新日志那套；publish 每次重建三平台产物，已不再用 `.version` sidecar 识别旧产物。**v0.8.0 起只构建 aarch64 + armv7 两个 ABI**（x86/x86_64 纯为模拟器服务却占掉 APK 一半体积，砍掉；armv7 刻意保留覆盖真机；移动端回归验证走 Playwright + 系统 Edge，不依赖 x86 APK）；给了显式 `--target` 后 gradle 可能出逐 ABI 的包，产物收集分档兜底 `universal/` → `arm64-v8a/` → 最新的那个。细节与体积基线（`[profile.release]`）见 `build-and-release.md`。

### 11. 图标同步

`scripts/make-icon.py` 同时生成桌面 icons 与 android mipmap 五密度（launcher/round/foreground，foreground 按 108dp 画布 66% 安全区）；换 logo 后跑一次即全平台同步，package.ps1 每次构建前会自动跑。

### 12. 通知

移动端走 tauri-plugin-notification（Rust 注册插件 + capabilities/mobile.json 的 `notification:default` + Manifest `POST_NOTIFICATIONS`）；Android 13+ 需运行时权限，前端发送前 `isPermissionGranted/requestPermission`，拒绝则降级 Toast。桌面自绘通知窗逻辑不动。

### 13. 能力门控优先于 isMobile 散落判断

平台差异收敛到 `src/lib/capabilities.ts`（scheduler/trayLifecycle/globalShortcuts/windowZoom/popupNotificationWindow/systemNotifications/nativeFileDialogs/updateChannel/desktop）；Rust 侧对应 `#[cfg(desktop)]`/`#[cfg(not(desktop))]` 命令面。新增平台只扩这两处 + CSS 命名空间，不改业务组件。

### 14. `isMobile` 是 writable store，不是布尔

（platform.ts）。组件里 `import { isMobile }` 后当布尔用永远为真——v0.6.2 就因此让桌面端也走了移动端手势（双击既展开又开编辑器）。组件内要么 `$isMobile` 订阅成局部布尔，要么 `get(isMobile)`。

### 15. 夹行只写 `-webkit-` 三件套

（`display:-webkit-box` + `-webkit-box-orient:vertical` + `-webkit-line-clamp:N`）。无缀 `line-clamp` 在 Chromium 里会把 `display` 的计算值顶成 `flow-root`，夹行直接失效（实测折叠态标题没被夹成两行）；别「两个都写以求兼容」。

### 16. ContextMenu 重新收敛时别量自己的 rect 高度

菜单自己带着 inline `max-height` 时 `getBoundingClientRect().height` 永远等于被夹住的高度，子菜单开合后重测就再也发现不了溢出（限高静默失效、菜单伸出屏幕）。内容高度取 `max(rect.height, scrollHeight)`（两者都要除以 uiScale）。

### 17. app-shell 的 transform 缩放影响一切 getBoundingClientRect

（移动端默认 `--ui-scale` 就不是 1）：rect 是视觉像素、`offsetHeight`/`scrollHeight` 是布局像素。Playwright 断言「几行高」「是否超出视口」时两边别混用——混用会算出 1.5 行这种不存在的值，把真 bug 判成假 bug 或反之。

### 18. 首屏量尺寸全是 0

移动端首屏是 list 视图，`.app-shell.mobile.view-list .workspace` 是 `display:none`，此时挂载的卡片量 `scrollHeight/clientWidth` 全是 0；用户点进内容页变可见时**没有任何 window 事件**（resize 不发），只挂 window resize 的测量逻辑会永久停留在首屏的错误结论（v0.6.3 的折叠态省略号就栽在这）。测量类 action 一律配 ResizeObserver 观察元素自身。

### 19. 软键盘 = resize + scroll 两连击

安卓聚焦菜单里的输入框，键盘弹出会触发 window resize、浏览器还会把输入框 scrollIntoView 触发 scroll——两者都曾是 ContextMenu 的关菜单信号，表现是「点输入框菜单就消失、点颜色圆圈没事」。菜单内有聚焦的可编辑元素时这两个信号都要吞掉（resize 改做重新收敛）。

### 20. 菜单限高绝不能把菜单顶到视口顶部

旧逻辑「菜单比视口-16 高就 top=8」在菜单较高（列表菜单加一项就超）时会让菜单**盖住唤起它的按钮**——点按钮的 mousedown 打开菜单、菜单罩在按钮上，随后的 click/松手就落在菜单项上（误触 + 立刻关闭）。正确做法：按**锚点向下的空间**限高并保持 `top=锚点`（菜单永远从按钮/触点下方展开、内部滚动），只有向上空间明显更大时才向上翻转。钻入式隐藏同理要用兜底选择器（`sub-open > *:not(...)`），逐个列举子元素类型必漏裸 div/label/input。
