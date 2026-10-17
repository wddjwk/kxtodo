use super::*;
use std::sync::mpsc::Receiver;

#[derive(Default)]
struct TestBackend {
    writes: Mutex<Vec<ClipboardContent>>,
}

impl HostBackend for TestBackend {
    fn show_notification(
        &self,
        _payload: &Value,
        _wait_rx: Option<Receiver<()>>,
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

fn test_settings(code: &str) -> SettingsFile {
    let mut settings = SettingsFile::default();
    settings.clipboard.enabled = true;
    settings.clipboard.code = code.to_string();
    settings.clipboard.lan_enabled = true;
    settings.clipboard.limits.files_mb = 64;
    settings
}

fn test_inner(
    root: &Path,
    code: &str,
    name: &str,
    backend: Option<Arc<dyn HostBackend>>,
) -> Arc<Inner> {
    let settings = normalized(&test_settings(code).clipboard);
    let secret = SecretKey::generate();
    Arc::new(Inner {
        layout: Layout::new(root.to_path_buf()),
        settings: settings.clone(),
        net: ClipboardNet::from_settings(&test_settings(code)),
        backend,
        signature: "test".to_string(),
        cancel: AtomicBool::new(false),
        notify: Notify::new(),
        device_id: secret.public().to_z32(),
        name: name.to_string(),
        group: group_key(code),
        save_dir: root.join("received"),
        secret,
        tcp_port: Mutex::new(0),
        direct_addrs: Vec::new(),
        endpoint: Mutex::new(None),
        lan_peers: Mutex::new(BTreeMap::new()),
        p2p_peers: Mutex::new(BTreeMap::new()),
        direct_peers: Mutex::new(BTreeMap::new()),
        recent: Mutex::new(BTreeMap::new()),
        last_local: Mutex::new(String::new()),
        last_remote: Mutex::new(String::new()),
        last_error: Mutex::new(String::new()),
        last_event: Mutex::new(None),
        last_status: Mutex::new(String::new()),
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
    // 大小不同必须换指纹，否则回声抑制会把另一份内容当成同一份。
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
fn settings_are_normalized_and_short_codes_rejected() {
    let mut settings = ClipboardSettings::default();
    settings.limits.text_mb = 999;
    settings.limits.files_mb = 0;
    settings.code = "  12345678  ".into();
    let normalized = normalize_settings(&settings).unwrap();
    assert_eq!(normalized.code, "12345678");
    assert_eq!(normalized.limits.text_mb, 64);
    assert_eq!(normalized.limits.files_mb, 1);
    settings.code = "short".into();
    // 关闭状态允许先留着没填完的口令；开启后才必须 ≥8 位
    assert!(normalize_settings(&settings).is_ok());
    settings.enabled = true;
    assert!(normalize_settings(&settings).is_err());
}

#[test]
fn direct_addresses_accept_ips_ports_and_ipv6() {
    let parsed = parse_direct_addresses("192.168.1.20, 10.0.0.2:6000\n[::1]\ninvalid");
    assert_eq!(parsed[0], "192.168.1.20:52179".parse().unwrap());
    assert_eq!(parsed[1], "10.0.0.2:6000".parse().unwrap());
    assert_eq!(parsed[2], "[::1]:52179".parse().unwrap());
    assert_eq!(parsed.len(), 3);
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
    let receiver = test_inner(root.path(), "clipboard-test-code", "receiver", Some(backend.clone()));
    let sender = test_inner(root.path(), "clipboard-test-code", "sender", None);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = receiver.clone();
    tokio::spawn(async move {
        lan_accept_loop(task, listener).await;
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
    let (peer, name) = send_lan(&sender, addr, &content).await.unwrap();
    assert_eq!(peer, receiver.device_id);
    assert_eq!(name, "receiver");
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
    let mut receiver = test_inner(root.path(), "clipboard-test-code", "receiver", Some(backend.clone()));
    {
        let inner = Arc::get_mut(&mut receiver).unwrap();
        inner.settings.types.text = false;
        inner.settings.types.image = false;
        inner.settings.types.files = false;
    }
    let sender = test_inner(root.path(), "clipboard-test-code", "sender", None);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = receiver.clone();
    tokio::spawn(async move {
        lan_accept_loop(task, listener).await;
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
async fn p2p_stream_round_trip_without_pkarr() {
    let root = tempfile::tempdir().unwrap();
    let backend = Arc::new(TestBackend::default());
    let receiver = test_inner(root.path(), "clipboard-test-code", "receiver", Some(backend.clone()));
    let sender = test_inner(root.path(), "clipboard-test-code", "sender", None);
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
    send_p2p(&sender, addr, &content).await.unwrap();
    let received = wait_for_write(&backend);
    assert_eq!(received.text, "p2p text");
    receiver.cancel.store(true, Ordering::SeqCst);
    let _ = receiver_endpoint.close().await;
}
