//! Remote pages live in capability-free windows, never in the application's WebView.
use kxtodo_core::linkmeta::LinkMeta;
use tauri::AppHandle;

#[cfg(desktop)]
fn local_asset_allowed(label: &str) -> bool {
    label == "main" || label.starts_with("notification-")
}

#[cfg(desktop)]
pub fn local_asset_response(
    app: &AppHandle,
    label: &str,
    request: tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Vec<u8>> {
    use tauri::Manager;
    let denied = || tauri::http::Response::builder().status(403).body(Vec::new()).unwrap();
    if !local_asset_allowed(label) {
        return denied();
    }
    let Some(window) = app.get_webview_window(label) else { return denied() };
    let Ok(url) = window.url() else { return denied() };
    let origin = if cfg!(windows) && !matches!(url.scheme(), "http" | "https") {
        format!("http://{}.localhost", url.scheme())
    } else {
        format!("{}://{}{}", url.scheme(), url.host_str().unwrap_or("localhost"),
            url.port().map(|port| format!(":{port}")).unwrap_or_default())
    };
    if request.headers().get("origin").is_some_and(|value| value.as_bytes() != origin.as_bytes()) {
        return denied();
    }
    let Ok(path) = percent_encoding::percent_decode_str(request.uri().path().strip_prefix('/').unwrap_or_default()).decode_utf8() else {
        return denied();
    };
    let path = std::path::PathBuf::from(path.as_ref());
    if tauri::path::SafePathBuf::new(path.clone()).is_err() || !app.asset_protocol_scope().is_allowed(&path) {
        return denied();
    }
    let Ok(bytes) = std::fs::read(&path) else { return denied() };
    tauri::http::Response::builder()
        .header("Access-Control-Allow-Origin", origin)
        .header("Content-Type", crate::sniff_image_mime(&bytes, &path))
        .body(bytes)
        .unwrap()
}

/// App commands have no per-command ACL manifest. Deny these windows even if a remote
/// page manages to embed a local-origin frame; plugin commands remain capability-gated.
#[cfg(desktop)]
pub fn without_remote_link_ipc(
    handler: impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static,
) -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    move |invoke| {
        let label = invoke.message.webview_ref().label();
        if label == "link-preview" || label.starts_with("link-meta-") {
            invoke
                .resolver
                .reject("Remote link windows cannot invoke native commands");
            true
        } else {
            handler(invoke)
        }
    }
}

#[tauri::command]
pub async fn open_link_preview(
    app: AppHandle,
    url: String,
    title: Option<String>,
) -> Result<(), String> {
    #[cfg(desktop)]
    {
        desktop::open(app, url, title.unwrap_or_default()).await
    }
    #[cfg(not(desktop))]
    {
        let _ = (app, url, title);
        Err("此平台使用系统浏览器/Custom Tabs".into())
    }
}

#[tauri::command]
pub async fn extract_link_meta(app: AppHandle, url: String) -> Result<LinkMeta, String> {
    #[cfg(desktop)]
    {
        desktop::extract(app, url).await
    }
    #[cfg(not(desktop))]
    {
        let _ = (app, url);
        Err("此平台不支持渲染式链接元数据".into())
    }
}

// WebviewUrl::External is unsupported on Android. Keep the entire implementation desktop-only.
#[cfg(desktop)]
mod desktop {
    use super::*;
    use kxtodo_core::linkmeta::{external_url, normalize_metadata, META_MAX_BYTES};
    use std::sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex, OnceLock,
    };
    use std::time::Duration;
    use tauri::{
        webview::{NewWindowResponse, PageLoadEvent},
        Manager, Url, WebviewUrl, WebviewWindowBuilder,
    };
    use tauri_plugin_opener::OpenerExt;
    use tokio::time::{timeout_at, Instant};

    const PREVIEW: &str = "link-preview";
    const PREFIX: &str = "KXLM:";
    const DEADLINE: Duration = Duration::from_secs(12);

    #[derive(Clone)]
    struct NavigationPolicy {
        app_origin: Option<String>,
        dev_origin: Option<String>,
    }

    impl NavigationPolicy {
        fn new(app: &AppHandle) -> Self {
            Self {
                app_origin: app
                    .get_webview_window("main")
                    .and_then(|w| w.url().ok())
                    .map(|u| u.origin().ascii_serialization()),
                dev_origin: app
                    .config()
                    .build
                    .dev_url
                    .as_ref()
                    .map(|u| u.origin().ascii_serialization()),
            }
        }
        fn allows(&self, url: &Url) -> bool {
            let origin = url.origin().ascii_serialization();
            external_url(url.as_str()).is_ok()
                && self.app_origin.as_ref() != Some(&origin)
                && self.dev_origin.as_ref() != Some(&origin)
        }
    }

    fn window_title(title: &str) -> String {
        title
            .chars()
            .filter(|c| !c.is_control())
            .take(200)
            .collect::<String>()
            .trim()
            .to_owned()
    }

    pub async fn open(app: AppHandle, url: String, title: String) -> Result<(), String> {
        let url = external_url(&url).map_err(|e| e.to_string())?;
        let (tx, mut rx) = tauri::async_runtime::channel(1);
        let handle = app.clone();
        // An async command must schedule construction on the UI thread; never block that thread
        // waiting for its own run_on_main_thread closure (WebView2/GTK construction can deadlock).
        app.run_on_main_thread(move || {
            let result = (|| -> Result<(), String> {
                let policy = NavigationPolicy::new(&handle);
                if !policy.allows(&url) {
                    return Err("不允许打开应用本地地址".into());
                }
                let title = window_title(&title);
                let title = if title.is_empty() {
                    url.host_str().unwrap_or("链接预览").to_owned()
                } else {
                    title
                };
                if let Some(window) = handle.get_webview_window(PREVIEW) {
                    // Typed navigation avoids interpolating URL text into JavaScript.
                    window.navigate(url).map_err(|e| e.to_string())?;
                    window.set_title(&title).map_err(|e| e.to_string())?;
                    window.show().map_err(|e| e.to_string())?;
                    window.unminimize().map_err(|e| e.to_string())?;
                    return window.set_focus().map_err(|e| e.to_string());
                }
                let popup_app = handle.clone();
                let popup_policy = policy.clone();
                WebviewWindowBuilder::new(&handle, PREVIEW, WebviewUrl::External(url))
                    .title(&title)
                    .inner_size(1080.0, 780.0)
                    .center()
                    .disable_drag_drop_handler()
                    // This hook only serves the local tauri protocol, not external web responses.
                    .on_web_resource_request(|_, response| {
                        *response.status_mut() = tauri::http::StatusCode::FORBIDDEN;
                        *response.body_mut() = std::borrow::Cow::Borrowed(&[]);
                    })
                    .on_navigation(move |url| policy.allows(url))
                    .on_document_title_changed(|window, title| {
                        let title = window_title(&title);
                        if !title.is_empty() {
                            let _ = window.set_title(&title);
                        }
                    })
                    .on_new_window(move |url, _| {
                        // No child remote WebView may escape the navigation guard/capability boundary.
                        if popup_policy.allows(&url) {
                            let app = popup_app.clone();
                            tauri::async_runtime::spawn_blocking(move || {
                                let _ = app.opener().open_url(url.as_str(), None::<&str>);
                            });
                        }
                        NewWindowResponse::Deny
                    })
                    .build()
                    .map_err(|e| e.to_string())?;
                Ok(())
            })();
            let _ = tx.try_send(result);
        })
        .map_err(|e| e.to_string())?;
        rx.recv()
            .await
            .ok_or_else(|| "链接窗口创建已取消".to_owned())?
    }

    type MetaResult = Result<LinkMeta, String>;
    struct Reply {
        done: AtomicBool,
        sender: tauri::async_runtime::Sender<MetaResult>,
        document: Mutex<Option<String>>,
    }
    impl Reply {
        fn finish(&self, result: MetaResult) {
            if !self.done.swap(true, Ordering::SeqCst) {
                let _ = self.sender.try_send(result);
            }
        }
    }

    /// Dropping the future (not only timeout/normal completion) also destroys the extractor.
    /// A queued construction checks `done` before AND after building, so a delayed UI thread
    /// cannot create an orphan window after the caller has timed out.
    struct Cleanup {
        app: AppHandle,
        label: String,
        reply: Arc<Reply>,
    }
    impl Drop for Cleanup {
        fn drop(&mut self) {
            self.reply.done.store(true, Ordering::SeqCst);
            let app = self.app.clone();
            let label = self.label.clone();
            let _ = self.app.run_on_main_thread(move || {
                if let Some(window) = app.get_webview_window(&label) {
                    let _ = window.destroy();
                }
            });
        }
    }

    pub async fn extract(app: AppHandle, request: String) -> MetaResult {
        static SERIAL: OnceLock<tauri::async_runtime::Mutex<()>> = OnceLock::new();
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let url = external_url(&request).map_err(|e| e.to_string())?;
        let deadline = Instant::now() + DEADLINE;
        let _serial = timeout_at(
            deadline,
            SERIAL
                .get_or_init(|| tauri::async_runtime::Mutex::new(()))
                .lock(),
        )
        .await
        .map_err(|_| "链接元数据等待超时".to_owned())?;
        // Unique labels also avoid stale title events and window-state restoration across requests.
        let label = format!(
            "link-meta-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let (sender, mut receiver) = tauri::async_runtime::channel(1);
        let reply = Arc::new(Reply {
            done: AtomicBool::new(false),
            sender,
            document: Mutex::new(None),
        });
        let cleanup = Cleanup {
            app: app.clone(),
            label: label.clone(),
            reply: reply.clone(),
        };
        let handle = app.clone();
        app.run_on_main_thread(move || {
            if reply.done.load(Ordering::SeqCst) || Instant::now() >= deadline {
                return;
            }
            let policy = NavigationPolicy::new(&handle);
            if !policy.allows(&url) {
                reply.finish(Err("不允许解析应用本地地址".into()));
                return;
            }
            let nav_reply = reply.clone();
            let load_reply = reply.clone();
            let title_reply = reply.clone();
            let load_policy = policy.clone();
            let title_policy = policy.clone();
            let built = WebviewWindowBuilder::new(&handle, &label, WebviewUrl::External(url))
                .title("Link metadata")
                .inner_size(960.0, 720.0)
                .visible(false)
                .focused(false)
                .skip_taskbar(true)
                .position(-32000.0, -32000.0)
                .incognito(true)
                .disable_drag_drop_handler()
                .on_web_resource_request(|_, response| {
                    *response.status_mut() = tauri::http::StatusCode::FORBIDDEN;
                    *response.body_mut() = std::borrow::Cow::Borrowed(&[]);
                })
                .on_navigation(move |url| {
                    let allowed = policy.allows(url);
                    if !allowed {
                        nav_reply.finish(Err("页面跳转到不支持的地址".into()));
                    }
                    allowed
                })
                // Hidden extraction NEVER opens a browser, including malicious window.open calls.
                .on_new_window(|_, _| NewWindowResponse::Deny)
                .on_download(|_, _| false)
                .on_page_load(move |window, payload| {
                    if load_reply.done.load(Ordering::SeqCst) {
                        return;
                    }
                    match payload.event() {
                        PageLoadEvent::Started => {
                            *load_reply
                                .document
                                .lock()
                                .unwrap_or_else(|p| p.into_inner()) = None;
                        }
                        PageLoadEvent::Finished => {
                            if !load_policy.allows(payload.url()) {
                                return;
                            }
                            let expected = payload.url().as_str();
                            *load_reply
                                .document
                                .lock()
                                .unwrap_or_else(|p| p.into_inner()) = Some(expected.to_owned());
                            // eval returns no value. A bounded document-title callback is our only result channel.
                            let encoded = match serde_json::to_string(expected) {
                                Ok(value) => value,
                                Err(error) => {
                                    load_reply.finish(Err(error.to_string()));
                                    return;
                                }
                            };
                            if let Err(error) =
                                window.eval(format!("({EXTRACT_SCRIPT})({encoded});"))
                            {
                                load_reply.finish(Err(error.to_string()));
                            }
                        }
                    }
                })
                .on_document_title_changed(move |window, title| {
                    if title_reply.done.load(Ordering::SeqCst) || !title.starts_with(PREFIX) {
                        return;
                    }
                    let Ok(page) = window.url() else {
                        return;
                    };
                    if !title_policy.allows(&page) {
                        return;
                    }
                    let current = title_reply
                        .document
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .clone();
                    if current.as_deref() != Some(page.as_str()) {
                        return;
                    }
                    title_reply.finish(decode_title(&request, &page, &title));
                })
                .build();
            match built {
                Ok(window) => {
                    if reply.done.load(Ordering::SeqCst) || Instant::now() >= deadline {
                        let _ = window.destroy();
                    }
                }
                Err(error) => reply.finish(Err(error.to_string())),
            }
        })
        .map_err(|e| e.to_string())?;
        let result = match timeout_at(deadline, receiver.recv()).await {
            Ok(Some(result)) => result,
            Ok(None) => Err("链接元数据提取已取消".into()),
            Err(_) => Err("链接元数据提取超时".into()),
        };
        // Queue destruction before releasing the serial gate (also covered by Drop on early errors).
        drop(cleanup);
        result
    }

    fn decode_title(request: &str, page: &Url, title: &str) -> MetaResult {
        let raw = title.strip_prefix(PREFIX).ok_or("无效的元数据回调")?;
        if raw.len() > META_MAX_BYTES {
            return Err("链接元数据响应过大".into());
        }
        let mut meta: LinkMeta =
            serde_json::from_str(raw).map_err(|_| "无效的链接元数据".to_owned())?;
        // The actual WebView URL, not a page-provided canonical/og:url, supplies the icon base.
        meta.url = page.to_string();
        normalize_metadata(request, meta).map_err(|e| e.to_string())
    }

    // Normal public-page rendering only: no login automation, CAPTCHA solving or challenge bypass.
    // One observer per final Document. Empty/JS-shell metadata may arrive after the load event.
    const EXTRACT_SCRIPT: &str = r#"function(expected) {
      if (window.top !== window || location.href !== expected || document.__kxLinkMeta) return;
      document.__kxLinkMeta = true;
      const cut = (s, n) => Array.from(String(s || '').slice(0, n * 2)).slice(0, n).join('');
      const pick = (s) => document.querySelector(s)?.getAttribute('content') || '';
      let timer, sent = false;
      const observer = new MutationObserver(() => { clearTimeout(timer); timer = setTimeout(sample, 250); });
      function sample() {
        if (sent || location.href !== expected) return;
        const title = cut(pick('meta[property="og:title"]') || pick('meta[name="twitter:title"]') || document.title, 200).trim();
        const description = cut(pick('meta[property="og:description"]') || pick('meta[name="description"]') || pick('meta[name="twitter:description"]'), 400).trim();
        if ((!title && !description) || title.startsWith('KXLM:') || /^(just a moment[.…]*|checking your browser[.…]*|verify you are human[.!…]*|loading[.…]*|attention required!?\s*\|\s*cloudflare)$/i.test(title)) return;
        const icon = document.querySelector('link[rel~="icon"]') || document.querySelector('link[rel="apple-touch-icon"]');
        const metadata = { url: cut(location.href, 4096), title, description,
          site: cut(pick('meta[property="og:site_name"]'), 100), icon: cut(icon?.href, 4096) };
        sent = true;
        observer.disconnect();
        document.title = 'KXLM:' + JSON.stringify(metadata);
      }
      observer.observe(document.documentElement, { childList: true, subtree: true, attributes: true, characterData: true });
      timer = setTimeout(sample, 250);
    }"#;

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn remote_windows_never_read_local_assets() {
            for label in ["link-preview", "link-meta-123-0", "untrusted", ""] {
                assert!(!local_asset_allowed(label));
            }
            assert!(local_asset_allowed("main"));
            assert!(local_asset_allowed("notification-0"));
        }

        #[test]
        fn title_callback_is_bounded_and_cannot_replace_the_request_key() {
            let page = Url::parse("https://redirect.example/posts/final").unwrap();
            let meta = decode_title("https://request.example/a#section", &page,
                r#"KXLM:{"url":"https://victim.example/","title":"Page","description":"","site":"","icon":"../fav.png"}"#).unwrap();
            assert_eq!(meta.url, "https://request.example/a");
            assert_eq!(meta.icon, "https://redirect.example/fav.png");
            assert!(decode_title("https://request.example", &page, "KXLM:{bad").is_err());
            assert!(decode_title(
                "https://request.example",
                &page,
                &format!("KXLM:{}", "x".repeat(META_MAX_BYTES + 1))
            )
            .is_err());
        }

        #[test]
        fn result_channel_is_first_wins() {
            let (sender, mut receiver) = tauri::async_runtime::channel(1);
            let reply = Reply {
                done: AtomicBool::new(false),
                sender,
                document: Mutex::new(None),
            };
            reply.finish(Err("first".into()));
            reply.finish(Ok(LinkMeta::default()));
            assert_eq!(receiver.try_recv().unwrap().unwrap_err(), "first");
            assert!(receiver.try_recv().is_err());
        }

        #[test]
        fn navigation_rejects_native_origins_and_non_web_schemes() {
            let policy = NavigationPolicy {
                app_origin: Some("https://app.example".into()),
                dev_origin: None,
            };
            for url in [
                "https://app.example/path",
                "http://tauri.localhost",
                "file:///secret",
                "tauri://localhost",
                "javascript:alert(1)",
            ] {
                assert!(!policy.allows(&Url::parse(url).unwrap()));
            }
            assert!(policy.allows(&Url::parse("https://public.example/page").unwrap()));
        }
    }
}
