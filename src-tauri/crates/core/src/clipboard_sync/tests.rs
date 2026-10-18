use super::*;
use std::sync::mpsc::Receiver as StdReceiver;

#[derive(Default)]
struct TestBackend {
    writes: Mutex<Vec<ClipboardContent>>,
}

impl HostBackend for TestBackend {
    fn show_notification(
        &self,
        _payload: &Value,
        _wait_rx: Option<StdReceiver<()>>,
    ) -> CoreResult<String> {
        Ok(String::new())
    }
    fn emit(&self, _event: &str, _payload: Value) {}
    fn apply_native_effect(&self, _name: &str, _settings: &SettingsFile) -> CoreResult<()> {
        Ok(())
    }
    fn request_exit(&self) {}
    fn has_gui(&self) -> bool {
        false
    }
    fn show_main_window(&self) -> CoreResult<()> {
        Ok(())
    }
    fn clipboard_available(&self) -> bool {
        true
    }
    fn clipboard_write(&self, content: &ClipboardContent) -> CoreResult<()> {
        self.writes.lock().unwrap().push(content.clone());
        Ok(())
    }
}

fn test_settings() -> SettingsFile {
    let mut settings = SettingsFile::default();
    settings.clipboard.enabled = true;
    settings.clipboard.lan_enabled = true;
    settings.clipboard.limits.files_mb = 64;
    settings
}

fn test_inner(root: &Path, name: &str, backend: Option<Arc<dyn HostBackend>>) -> Arc<Inner> {
    let settings_file = test_settings();
    let settings = normalized(&settings_file.clipboard);
    let secret = SecretKey::generate();
    Arc::new(Inner {
        layout: Layout::new(root.to_path_buf()),
        settings: settings.clone(),
        net: ClipboardNet::from_settings(&settings_file),
        backend,
        signature: "test".to_string(),
        cancel: AtomicBool::new(false),
        notify: Notify::new(),
        refresh: Notify::new(),
        device_id: secret.public().to_z32(),
        name: name.to_string(),
        save_dir: root.join("received"),
        secret,
        tcp_port: Mutex::new(0),
        endpoint: Mutex::new(None),
        lan_peers: Mutex::new(BTreeMap::new()),
        wan_peers: Mutex::new(BTreeMap::new()),
        disabled: Mutex::new(BTreeSet::new()),
        recent: Mutex::new(BTreeMap::new()),
        last_local: Mutex::new(String::new()),
        last_remote: Mutex::new(String::new()),
        last_error: Mutex::new(String::new()),
        lan_error: Mutex::new(String::new()),
        endpoint_error: Mutex::new(String::new()),
        server_error: Mutex::new(String::new()),
        p2p_error: Mutex::new(String::new()),
        last_event: Mutex::new(None),
        last_status: Mutex::new(String::new()),
        server_token: Mutex::new(None),
        server_ok: AtomicBool::new(false),
        p2p_ok: AtomicBool::new(false),
        sent_count: AtomicU64::new(0),
        received_count: AtomicU64::new(0),
    })
}

#[test]
fn signature_is_stable_across_file_locations() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.txt");
    let nested = dir.path().join("sub");
    std::fs::create_dir_all(&nested).unwrap();
    let b = nested.join("a.txt");
    std::fs::write(&a, b"same").unwrap();
    std::fs::write(&b, b"same").unwrap();
    let one = ClipboardContent {
        text: "x".into(),
        files: vec![ClipboardFile { name: "a.txt".into(), path: Some(a), bytes: None }],
        ..Default::default()
    };
    let two = ClipboardContent {
        text: "x".into(),
        files: vec![ClipboardFile { name: "a.txt".into(), path: Some(b), bytes: None }],
        ..Default::default()
    };
    assert_eq!(one.signature(), two.signature());
    let three = ClipboardContent {
        files: vec![ClipboardFile {
            name: "a.txt".into(),
            path: None,
            bytes: Some(b"different size".to_vec()),
        }],
        ..Default::default()
    };
    assert_ne!(one.signature(), three.signature());
}

#[test]
fn settings_are_normalized_and_wan_requires_credentials() {
    let mut settings = ClipboardSettings::default();
    settings.limits.text_mb = 999;
    settings.limits.files_mb = 0;
    settings.device_name = "  书房 / PC  ".into();
    let value = normalize_settings(&settings).unwrap();
    assert_eq!(value.limits.text_mb, 64);
    assert_eq!(value.limits.files_mb, 1);
    assert_eq!(value.device_name, "书房 / PC");

    // 局域网单开合法；缺广域网凭据也允许保存（旧配置/CLI 分步配置），
    // 由服务只启动能启动的部分，状态里说明缺什么。
    settings.enabled = true;
    settings.lan_enabled = true;
    assert!(normalize_settings(&settings).is_ok());
    settings.lan_enabled = false;
    assert!(normalize_settings(&settings).is_ok(), "保存不应被拒绝，服务层再判断运行条件");
    settings.p2p_enabled = true;
    assert!(normalize_settings(&settings).is_ok());
    settings.account = "user".into();
    settings.password = "secret".into();
    assert!(normalize_settings(&settings).is_ok());
    settings.p2p_enabled = false;
    settings.server_enabled = true;
    settings.server_url = "192.168.1.10".into();
    let value = normalize_settings(&settings).unwrap();
    assert_eq!(value.server_url, "http://192.168.1.10:52177");
    settings.server_url = "http://".into();
    assert!(normalize_settings(&settings).is_err(), "非法服务器地址仍然拒绝");

    // 关闭状态同样允许保存半成品。
    settings.enabled = false;
    settings.password.clear();
    settings.server_url = String::new();
    assert!(normalize_settings(&settings).is_ok());
}

#[test]
fn server_urls_accept_ip_and_keep_explicit_ports() {
    assert_eq!(normalize_server_url("").unwrap(), "");
    assert_eq!(normalize_server_url("192.168.31.102").unwrap(), "http://192.168.31.102:52177");
    assert_eq!(
        normalize_server_url("192.168.31.102:9000").unwrap(),
        "http://192.168.31.102:9000"
    );
    assert_eq!(
        normalize_server_url("http://example.com/").unwrap(),
        "http://example.com:52177"
    );
    assert_eq!(
        normalize_server_url("https://example.com/").unwrap(),
        "https://example.com"
    );
    assert!(normalize_server_url("http://").is_err());
}

#[test]
fn device_name_follows_prefix_and_platform() {
    let id = SecretKey::generate().public().to_z32();
    let default = device_display_name(&id, "");
    assert!(default.starts_with("kxtodo·"), "{default}");
    assert!(default.ends_with(&format!("({})", &id[..8])), "{default}");
    assert_eq!(
        device_display_name(&id, "书房"),
        format!("书房·{}", platform_label())
    );
}

#[test]
fn room_depends_on_account_and_password() {
    let a = clipboard_room("User", "secret-1").unwrap();
    let b = clipboard_room("user", "secret-1").unwrap();
    assert_eq!(a.public(), b.public(), "用户名大小写不敏感");
    let c = clipboard_room("user", "secret-2").unwrap();
    assert_ne!(a.public(), c.public());
    let d = clipboard_room("other", "secret-1").unwrap();
    assert_ne!(a.public(), d.public());
}

#[test]
fn selection_file_roundtrip_and_runtime_update() {
    let root = tempfile::tempdir().unwrap();
    let layout = Layout::new(root.path().to_path_buf());
    set_peer_selected(&layout, "peer-1", false).unwrap();
    assert!(load_disabled(&layout).contains("peer-1"));
    set_peer_selected(&layout, "peer-1", true).unwrap();
    assert!(!load_disabled(&layout).contains("peer-1"));
    // 空 id 拒绝，避免把整个列表写坏。
    assert!(set_peer_selected(&layout, "  ", false).is_err());
}

#[test]
fn status_after_new_lan_peer_does_not_self_deadlock() {
    let root = tempfile::tempdir().unwrap();
    let inner = test_inner(root.path(), "local", None);
    inner.push_lan_peer("peer-1", "另一台", "127.0.0.1:51000".parse().unwrap());
    let (tx, rx) = std::sync::mpsc::channel();
    let task = inner.clone();
    std::thread::spawn(move || {
        let value = task.status_value();
        assert_eq!(value["peers"].as_array().unwrap().len(), 1);
        let _ = tx.send(());
    });
    rx.recv_timeout(Duration::from_secs(3))
        .expect("status_value 不能在持有 lan_peers 锁时再次锁 lan_peers");
}

#[test]
fn unselected_peers_are_not_targets() {
    let root = tempfile::tempdir().unwrap();
    let inner = test_inner(root.path(), "local", None);
    inner.push_lan_peer("peer-1", "另一台", "127.0.0.1:51000".parse().unwrap());
    assert_eq!(inner.targets().len(), 1);
    inner.disabled.lock().unwrap().insert("peer-1".into());
    assert!(inner.targets().is_empty());
}

#[test]
fn discovery_packet_roundtrip() {
    let packet = DiscoverPacket {
        v: PROTOCOL_VERSION,
        app: APP.to_string(),
        device: "abc".into(),
        name: "书房".into(),
        port: 52179,
    };
    let bytes = serde_json::to_vec(&packet).unwrap();
    let parsed: DiscoverPacket = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(parsed.port, 52179);
    assert_eq!(parsed.name, "书房");
}

fn wait_for_write(backend: &Arc<TestBackend>) -> ClipboardContent {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(content) = backend.writes.lock().unwrap().first().cloned() {
            return content;
        }
        assert!(Instant::now() < deadline, "接收超时");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lan_round_trip_carries_text_html_image_and_files() {
    let root = tempfile::tempdir().unwrap();
    let backend = Arc::new(TestBackend::default());
    let receiver = test_inner(root.path(), "receiver", Some(backend.clone()));
    let sender = test_inner(root.path(), "sender", None);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = receiver.clone();
    tokio::spawn(async move {
        lan_accept_loop_with_listener(task, listener).await;
    });
    let content = ClipboardContent {
        text: "同步这段文字".into(),
        html: "<b>同步这段文字</b>".into(),
        image: Some(vec![137, 80, 78, 71]),
        files: vec![ClipboardFile {
            name: "note.txt".into(),
            path: None,
            bytes: Some(b"file body".to_vec()),
        }],
    };
    send_lan(&sender, addr, &content).await.unwrap();
    let received = wait_for_write(&backend);
    assert_eq!(received.text, content.text);
    assert_eq!(received.html, content.html);
    assert_eq!(received.image, content.image);
    assert_eq!(received.files.len(), 1);
    assert_eq!(received.files[0].name, "note.txt");
    let path = received.files[0].path.clone().unwrap();
    assert_eq!(tokio::fs::read(path).await.unwrap(), b"file body");
    receiver.cancel.store(true, Ordering::SeqCst);
    receiver.notify.notify_waiters();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn disabled_types_are_ignored_at_the_receiver() {
    let root = tempfile::tempdir().unwrap();
    let backend = Arc::new(TestBackend::default());
    let mut receiver = test_inner(root.path(), "receiver", Some(backend.clone()));
    {
        let inner = Arc::get_mut(&mut receiver).unwrap();
        inner.settings.types.text = false;
        inner.settings.types.image = false;
        inner.settings.types.files = false;
    }
    let sender = test_inner(root.path(), "sender", None);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = receiver.clone();
    tokio::spawn(async move {
        lan_accept_loop_with_listener(task, listener).await;
    });
    let content = ClipboardContent {
        text: "secret".into(),
        html: "<i>rich</i>".into(),
        image: Some(vec![1, 2, 3]),
        files: vec![],
    };
    send_lan(&sender, addr, &content).await.unwrap();
    let received = wait_for_write(&backend);
    assert_eq!(received.text, "");
    assert_eq!(received.html, "<i>rich</i>");
    assert!(received.image.is_none());
    receiver.cancel.store(true, Ordering::SeqCst);
    receiver.notify.notify_waiters();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn receiver_rejects_clipboard_from_unselected_device() {
    let root = tempfile::tempdir().unwrap();
    let backend = Arc::new(TestBackend::default());
    let receiver = test_inner(root.path(), "receiver", Some(backend.clone()));
    let sender = test_inner(root.path(), "sender", None);
    receiver.disabled.lock().unwrap().insert(sender.device_id.clone());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = receiver.clone();
    tokio::spawn(async move {
        lan_accept_loop_with_listener(task, listener).await;
    });
    let content = ClipboardContent { text: "nope".into(), ..Default::default() };
    let result = send_lan(&sender, addr, &content).await;
    assert!(result.is_err(), "没有互相勾选时不应该落盘");
    assert!(backend.writes.lock().unwrap().is_empty());
    receiver.cancel.store(true, Ordering::SeqCst);
    receiver.notify.notify_waiters();
}

/// 与 `lan_listener_loop` 的 accept 分支同一套处理，但端口由调用方给。
async fn lan_accept_loop_with_listener(inner: Arc<Inner>, listener: TcpListener) {
    loop {
        if inner.cancel.load(Ordering::SeqCst) {
            break;
        }
        tokio::select! {
            _ = inner.notify.notified() => break,
            result = listener.accept() => {
                let Ok((stream, _)) = result else { continue };
                let task = inner.clone();
                inner_runtime_spawn(async move {
                    let (mut reader, mut writer) = stream.into_split();
                    let _ = serve_stream(&task, &mut reader, &mut writer).await;
                });
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn p2p_stream_round_trip_without_pkarr() {
    let root = tempfile::tempdir().unwrap();
    let backend = Arc::new(TestBackend::default());
    let receiver = test_inner(root.path(), "receiver", Some(backend.clone()));
    let sender = test_inner(root.path(), "sender", None);
    let receiver_endpoint = bind_endpoint(&receiver.secret, Some("disabled")).await.unwrap();
    *receiver.endpoint.lock().unwrap() = Some(receiver_endpoint.clone());
    let addr = receiver_endpoint.addr();
    let task = receiver.clone();
    let accepting_endpoint = receiver_endpoint.clone();
    tokio::spawn(async move {
        if let Some(incoming) = accepting_endpoint.accept().await {
            if let Ok(accepting) = incoming.accept() {
                if let Ok(connection) = accepting.await {
                    if let Ok((mut send, mut recv)) = connection.accept_bi().await {
                        let _ = serve_stream(&task, &mut recv, &mut send).await;
                        let _ = tokio::time::timeout(Duration::from_secs(1), connection.closed()).await;
                    }
                }
            }
        }
    });
    let sender_endpoint = bind_endpoint(&sender.secret, Some("disabled")).await.unwrap();
    *sender.endpoint.lock().unwrap() = Some(sender_endpoint);
    let content = ClipboardContent {
        text: "p2p text".into(),
        ..Default::default()
    };
    send_wan(&sender, addr, &content).await.unwrap();
    let received = wait_for_write(&backend);
    assert_eq!(received.text, "p2p text");
    receiver.cancel.store(true, Ordering::SeqCst);
    let _ = receiver_endpoint.close().await;
}
