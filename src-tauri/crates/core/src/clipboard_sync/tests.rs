use super::*;
use std::sync::atomic::AtomicUsize;
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
    // HTML 当文件处理（v0.9.0 bugfix 4.1）：html 段不再独立投递，落盘成一个 .html 文件
    assert_eq!(received.html, "");
    assert_eq!(received.image, content.image);
    assert_eq!(received.files.len(), 2);
    let html_entry = received
        .files
        .iter()
        .find(|file| file.name.ends_with(".html"))
        .expect("html 应当折成 .html 文件");
    let html_path = html_entry.path.clone().unwrap();
    assert_eq!(tokio::fs::read(html_path).await.unwrap(), content.html.as_bytes());
    let note = received.files.iter().find(|file| file.name == "note.txt").expect("note.txt");
    let path = note.path.clone().unwrap();
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
        // 文件开着：html 归文件档，应当照收
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
    assert!(received.image.is_none());
    assert_eq!(received.files.len(), 1, "html 折成文件走文件档");
    let html_path = received.files[0].path.clone().unwrap();
    assert_eq!(tokio::fs::read(html_path).await.unwrap(), b"<i>rich</i>");
    receiver.cancel.store(true, Ordering::SeqCst);
    receiver.notify.notify_waiters();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn all_types_off_drops_everything_including_html_as_file() {
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
    // 全关：什么都不写，事件记一条 ignored
    let deadline = Instant::now() + Duration::from_millis(600);
    loop {
        let event = receiver.last_event.lock().unwrap().clone();
        if event.as_ref().and_then(|value| value.get("kind").and_then(|k| k.as_str())) == Some("ignored") {
            break;
        }
        assert!(Instant::now() < deadline, "没等到 ignored 事件：{:?}", event);
        std::thread::sleep(Duration::from_millis(20));
    }
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

// ---------------------------------------------------------------------------
// v0.9.1：图片管线（DIB 编解码 / alpha 修复 / 超限可见性 / 轮询基线）
// ---------------------------------------------------------------------------

#[test]
fn zero_alpha_repair_only_when_fully_transparent() {
    // alpha 全 0（CF_DIBV5 坏数据的经典形态）→ 整体置 255，RGB 一个字节不动
    let mut rgba = vec![10u8, 20, 30, 0, 40, 50, 60, 0];
    repair_zero_alpha(&mut rgba);
    assert_eq!(rgba, vec![10, 20, 30, 255, 40, 50, 60, 255]);
    // 有任何非 0 alpha = 真透明，完全不动
    let mut partial = vec![10u8, 20, 30, 0, 40, 50, 60, 128];
    let before = partial.clone();
    repair_zero_alpha(&mut partial);
    assert_eq!(partial, before);
    // 空 / 长度非法 → 不 panic
    let mut empty: Vec<u8> = Vec::new();
    repair_zero_alpha(&mut empty);
    let mut odd = vec![1u8, 2, 3];
    repair_zero_alpha(&mut odd);
    assert_eq!(odd, vec![1, 2, 3]);
}

#[test]
fn dibv5_encoder_round_trips_through_dib_decoder() {
    // 2x3 非对称图：验证「自下而上」的行序在编码/解码两次翻转后正好还原
    let rgba: Vec<u8> = (0..24u8).collect();
    let dib = cf_dibv5_from_rgba(2, 3, &rgba).unwrap();
    let (width, height, out) = rgba_from_cf_dib(&dib).unwrap();
    assert_eq!((width, height), (2, 3));
    assert_eq!(out, rgba);
    // 头里声明的掩码与 arboard 写入的完全一致（BGRA + alpha 在最高字节）
    assert_eq!(&dib[40..44], &0x00ff0000u32.to_le_bytes());
    assert_eq!(&dib[52..56], &0xff000000u32.to_le_bytes());
    assert_eq!(cf_dibv5_from_rgba(2, 3, &[0u8; 10]), None, "长度不符必须拒绝");
    assert_eq!(cf_dibv5_from_rgba(0, 3, &[]), None);
}

fn dib_header(width: i32, height: i32, bpp: u16, compression: u32) -> Vec<u8> {
    let mut out = vec![0u8; 40];
    out[0..4].copy_from_slice(&40u32.to_le_bytes());
    out[4..8].copy_from_slice(&width.to_le_bytes());
    out[8..12].copy_from_slice(&height.to_le_bytes());
    out[12..14].copy_from_slice(&1u16.to_le_bytes());
    out[14..16].copy_from_slice(&bpp.to_le_bytes());
    out[16..20].copy_from_slice(&compression.to_le_bytes());
    out
}

#[test]
fn dib_decoder_handles_real_world_cf_dib_variants() {
    // BGRA 像素：内存序 B,G,R,A
    let px = |v: u8| [v, v + 1, v + 2, 255u8];

    // ① 32bpp BI_RGB，自下而上（CF_DIB 最常见形态）：2x2
    let mut dib = dib_header(2, 2, 32, 0);
    dib.extend_from_slice(&px(20)); // 底行左
    dib.extend_from_slice(&px(21)); // 底行右
    dib.extend_from_slice(&px(10)); // 顶行左
    dib.extend_from_slice(&px(11)); // 顶行右
    let (width, height, rgba) = rgba_from_cf_dib(&dib).unwrap();
    assert_eq!((width, height), (2, 2));
    assert_eq!(&rgba[0..4], &[12, 11, 10, 255], "顶行左：BGR→RGB");
    assert_eq!(&rgba[4..8], &[13, 12, 11, 255]);
    assert_eq!(&rgba[8..12], &[22, 21, 20, 255], "底行左");
    assert_eq!(&rgba[12..16], &[23, 22, 21, 255]);

    // ② 24bpp BI_RGB：每行 3 字节像素 + 补齐到 4 的倍数
    let mut dib24 = dib_header(1, 2, 24, 0);
    dib24.extend_from_slice(&[0x30, 0x20, 0x10, 0x00]); // 底行 BGR + padding
    dib24.extend_from_slice(&[0x31, 0x21, 0x11, 0x00]); // 顶行 BGR + padding
    let (width, height, rgba) = rgba_from_cf_dib(&dib24).unwrap();
    assert_eq!((width, height), (1, 2));
    assert_eq!(&rgba[0..4], &[0x11, 0x21, 0x31, 255], "顶行");
    assert_eq!(&rgba[4..8], &[0x10, 0x20, 0x30, 255], "底行");

    // ③ 负高度 = 顶向下：存储的第一行就是顶行
    let mut top_down = dib_header(2, -2, 32, 0);
    top_down.extend_from_slice(&px(10));
    top_down.extend_from_slice(&px(11));
    top_down.extend_from_slice(&px(20));
    top_down.extend_from_slice(&px(21));
    let (_, _, rgba) = rgba_from_cf_dib(&top_down).unwrap();
    assert_eq!(&rgba[0..4], &[12, 11, 10, 255]);
    assert_eq!(&rgba[12..16], &[23, 22, 21, 255]);

    // ④ 32bpp BI_BITFIELDS（40 字节头 + 12 字节掩码，无 alpha 掩码 → 不透明）
    let mut bitfields = dib_header(1, 1, 32, 3);
    bitfields.extend_from_slice(&0x00ff0000u32.to_le_bytes());
    bitfields.extend_from_slice(&0x0000ff00u32.to_le_bytes());
    bitfields.extend_from_slice(&0x000000ffu32.to_le_bytes());
    bitfields.extend_from_slice(&[0x01, 0x02, 0x03, 0x04]); // LE dword 0x04030201
    let (_, _, rgba) = rgba_from_cf_dib(&bitfields).unwrap();
    assert_eq!(rgba, vec![3, 2, 1, 255], "R=0x03 G=0x02 B=0x01，无 alpha 掩码按不透明");

    // ⑤ 坏头 / 数据不足一律拒绝，不 panic
    assert!(rgba_from_cf_dib(&[]).is_none());
    assert!(rgba_from_cf_dib(&dib_header(2, 2, 32, 0)).is_none(), "像素数据不足");
    assert!(rgba_from_cf_dib(&dib_header(0, 2, 32, 0)).is_none());
    assert!(rgba_from_cf_dib(&dib_header(2, 2, 16, 0)).is_none(), "16bpp 不支持");
    let mut rle = dib_header(2, 2, 32, 2); // BI_RLE8
    rle.extend_from_slice(&[0u8; 16]);
    assert!(rgba_from_cf_dib(&rle).is_none(), "RLE 不支持");
}

#[test]
fn local_limit_note_flags_oversized_parts() {
    let mut limits = ClipboardLimits::default();
    limits.text_mb = 1;
    limits.image_mb = 1;
    limits.files_mb = 1;
    let content = ClipboardContent {
        text: "x".repeat(2 * 1024 * 1024),
        image: Some(vec![0u8; 2 * 1024 * 1024]),
        files: vec![ClipboardFile {
            name: "a.bin".into(),
            path: None,
            bytes: Some(vec![0u8; 2 * 1024 * 1024]),
        }],
        ..Default::default()
    };
    let note = local_limit_note(&content, &limits);
    assert!(note.contains("文本超上限"), "{note}");
    assert!(note.contains("图片超上限"), "{note}");
    assert!(note.contains("部分文件超上限"), "{note}");
    let small = ClipboardContent { text: "hi".into(), ..Default::default() };
    assert_eq!(local_limit_note(&small, &limits), "", "没超限就没有备注");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn receiver_event_notes_image_dropped_over_limit() {
    let root = tempfile::tempdir().unwrap();
    let backend = Arc::new(TestBackend::default());
    let mut receiver = test_inner(root.path(), "receiver", Some(backend.clone()));
    {
        let inner = Arc::get_mut(&mut receiver).unwrap();
        inner.settings.limits.image_mb = 1;
    }
    let sender = test_inner(root.path(), "sender", None);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = receiver.clone();
    tokio::spawn(async move {
        lan_accept_loop_with_listener(task, listener).await;
    });
    let content = ClipboardContent {
        text: "文字照发".into(),
        image: Some(vec![9u8; 2 * 1024 * 1024]), // 2MB > 接收端 1MB 图片上限
        ..Default::default()
    };
    send_lan(&sender, addr, &content).await.unwrap();
    let received = wait_for_write(&backend);
    assert_eq!(received.text, "文字照发");
    assert!(received.image.is_none(), "超限图片应当被舍弃");
    // 被舍弃的图片必须在「收到」事件里点名——绝不静默（v0.9.1 之前只写「文本」）
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let event = receiver.last_event.lock().unwrap().clone();
        let detail = event
            .as_ref()
            .and_then(|value| value.get("detail"))
            .and_then(|detail| detail.as_str())
            .unwrap_or("")
            .to_string();
        if detail.contains("图片超上限") {
            break;
        }
        assert!(Instant::now() < deadline, "收到事件没提到被舍弃的图片：{event:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
    receiver.cancel.store(true, Ordering::SeqCst);
    receiver.notify.notify_waiters();
}

/// 脚本化的剪切板后端：第一次读是空（服务启动时剪切板没东西），之后每次读都有内容。
#[derive(Default)]
struct ScriptedClipboardBackend {
    calls: AtomicUsize,
}

impl HostBackend for ScriptedClipboardBackend {
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
    fn clipboard_read(&self) -> CoreResult<Option<ClipboardContent>> {
        let call = self.calls.fetch_add(1, Ordering::Relaxed);
        if call == 0 {
            Ok(None)
        } else {
            Ok(Some(ClipboardContent {
                text: "启用后的第一次复制".into(),
                ..Default::default()
            }))
        }
    }
}

#[test]
fn poller_primes_on_empty_clipboard_so_first_copy_sends() {
    // 服务启动时剪切板是空的：基线必须在这一次就建好（哪怕没内容可记），
    // 否则用户开启同步后的第一次复制会被当成「首次读取建基线」整次吞掉。
    let root = tempfile::tempdir().unwrap();
    let backend = Arc::new(ScriptedClipboardBackend::default());
    let inner = test_inner(root.path(), "local", Some(backend));
    spawn_native_poller(inner.clone());
    // 没有任何在线目标 → on_local → send_all 会记一条 "idle" 事件；
    // 事件出现即证明第一次真实复制走进了发送路径，而不是被基线吞掉。
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let event = inner.last_event.lock().unwrap().clone();
        if event.as_ref().and_then(|value| value.get("kind")).and_then(|kind| kind.as_str())
            == Some("idle")
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "空剪切板启动后的第一次复制被基线吞掉了：{event:?}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    inner.cancel.store(true, Ordering::SeqCst);
}
