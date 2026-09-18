//! 文件传输助手端到端：两台「设备」（各带自己的 runtime）凭同一句口令，经**本地 mock
//! pkarr 目录**互相发现、loopback 直连，跑一次含子目录的多文件传输（v0.8.4 起是
//! 「双端各自 online + 挑设备发」的模型）。
//!
//! 完全离线可跑：目录用本文件里的 mock pkarr relay（回环 HTTP），relay 设成 `disabled`
//! （两个端点在同机，目录里带的回环直连地址足够建连），不碰任何公共服务——
//! 与 `p2p_e2e` 同一套路。

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener as StdTcpListener, TcpStream as StdTcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;
use kxtodo_core::repo::Layout;
use kxtodo_core::transfer::{self, TransferItem, TransferNet, TransferPayload};

const CODE: &str = "kxtodo-transfer-e2e-code";

// ---------------------------------------------------------------------------
// mock pkarr relay：PUT /<z32> 存 body，GET /<z32> 回 200+body 或 404
// ---------------------------------------------------------------------------

fn start_mock_pkarr() -> String {
    let listener = StdTcpListener::bind("127.0.0.1:0").expect("bind mock pkarr relay");
    let url = format!("http://{}", listener.local_addr().unwrap());
    let store: Arc<Mutex<HashMap<String, Vec<u8>>>> = Arc::new(Mutex::new(HashMap::new()));
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let store = store.clone();
            std::thread::spawn(move || serve_conn(stream, store));
        }
    });
    url
}

fn serve_conn(stream: StdTcpStream, store: Arc<Mutex<HashMap<String, Vec<u8>>>>) {
    let mut writer = stream.try_clone().expect("clone stream");
    let mut reader = BufReader::new(stream);
    loop {
        let mut request_line = String::new();
        if reader.read_line(&mut request_line).unwrap_or(0) == 0 {
            return;
        }
        let mut content_length = 0usize;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                return;
            }
            let trimmed = line.trim_end();
            if trimmed.is_empty() {
                break;
            }
            if let Some((key, value)) = trimmed.split_once(':') {
                if key.eq_ignore_ascii_case("content-length") {
                    content_length = value.trim().parse().unwrap_or(0);
                }
            }
        }
        let mut parts = request_line.split_whitespace();
        let method = parts.next().unwrap_or_default().to_string();
        let path = parts.next().unwrap_or_default().to_string();
        let mut body = vec![0u8; content_length];
        if content_length > 0 && reader.read_exact(&mut body).is_err() {
            return;
        }
        let key = path.trim_start_matches('/').to_string();
        let (status, reason, payload) = match method.as_str() {
            "PUT" => {
                store.lock().expect("store").insert(key, body);
                (204, "No Content", Vec::new())
            }
            "GET" => match store.lock().expect("store").get(&key) {
                Some(value) => (200, "OK", value.clone()),
                None => (404, "Not Found", Vec::new()),
            },
            _ => (405, "Method Not Allowed", Vec::new()),
        };
        let head = format!(
            "HTTP/1.1 {status} {reason}\r\ncontent-length: {}\r\nconnection: keep-alive\r\n\r\n",
            payload.len()
        );
        if writer.write_all(head.as_bytes()).is_err() {
            return;
        }
        if !payload.is_empty() && writer.write_all(&payload).is_err() {
            return;
        }
        if writer.flush().is_err() {
            return;
        }
    }
}

// ---------------------------------------------------------------------------
// 事件记录
// ---------------------------------------------------------------------------

#[derive(Default, Clone)]
struct Recorder(Arc<Mutex<Vec<Value>>>);

impl Recorder {
    fn sink(&self) -> transfer::TransferSink {
        let events = self.0.clone();
        Arc::new(move |payload: Value| events.lock().expect("events").push(payload))
    }
    fn events(&self) -> Vec<Value> {
        self.0.lock().expect("events").clone()
    }
    fn kinds(&self) -> Vec<String> {
        self.events()
            .iter()
            .filter_map(|event| event["kind"].as_str().map(str::to_string))
            .collect()
    }
    fn wait_for(&self, session_id: &str, kind: &str, timeout: Duration) -> Option<Value> {
        let deadline = Instant::now() + timeout;
        loop {
            let found = self.events().into_iter().find(|event| {
                event["sessionId"].as_str() == Some(session_id)
                    && event["kind"].as_str() == Some(kind)
            });
            if found.is_some() {
                return found;
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

fn net(directory_url: &str) -> TransferNet {
    TransferNet {
        // 同机直连就够：relay 关掉，传输不依赖任何公共服务
        relay: Some("disabled".to_string()),
        directory_url: directory_url.to_string(),
    }
}

// ---------------------------------------------------------------------------
// 用例
// ---------------------------------------------------------------------------

#[test]
fn code_must_be_at_least_eight_chars() {
    assert!(transfer::validate_code("short").is_err());
    assert!(transfer::validate_code("  12345678  ").is_ok());
    let error = transfer::validate_code("七位口令七位口").err().unwrap();
    assert_eq!(error.code, "TRANSFER_CODE_TOO_SHORT");
}

#[test]
fn send_rejects_missing_files_and_bad_paths() {
    let dir = tempfile::tempdir().unwrap();
    let offline_root = tempfile::tempdir().unwrap();
    let offline_layout = Layout::new(offline_root.path().to_path_buf());
    // 不存在的文件
    let error = transfer::send(
        &offline_layout,
        "target",
        TransferPayload::Files {
            root: None,
            items: vec![TransferItem {
                rel: dir.path().join("nope.txt").to_string_lossy().to_string(),
                size: 1,
            }],
        },
    )
    .err()
    .unwrap();
    assert_eq!(error.code, "TRANSFER_FILE_MISSING");
    // 越界路径
    let error = transfer::send(
        &offline_layout,
        "target",
        TransferPayload::Files {
            root: Some(dir.path().to_string_lossy().to_string()),
            items: vec![TransferItem {
                rel: "../escape.txt".to_string(),
                size: 1,
            }],
        },
    )
    .err()
    .unwrap();
    assert_eq!(error.code, "TRANSFER_PATH_UNSAFE");
    // 空清单：校验先于在线检查（没上线也报「没东西可发」）
    let error = transfer::send(
        &offline_layout,
        "target",
        TransferPayload::Files {
            root: None,
            items: vec![],
        },
    )
    .err()
    .unwrap();
    assert_eq!(error.code, "TRANSFER_NOTHING_TO_SEND");
    // 空文本
    let error = transfer::send(
        &offline_layout,
        "target",
        TransferPayload::Text {
            text: "   ".to_string(),
        },
    )
    .err()
    .unwrap();
    assert_eq!(error.code, "TRANSFER_EMPTY_TEXT");
    // 清单合法但没上线 → TRANSFER_OFFLINE
    let error = transfer::send(
        &offline_layout,
        "target",
        TransferPayload::Text {
            text: "你好".to_string(),
        },
    )
    .err()
    .unwrap();
    assert_eq!(error.code, "TRANSFER_OFFLINE");
}

#[test]
fn go_offline_clears_the_online_slot() {
    let recorder = Recorder::default();
    let root = tempfile::tempdir().unwrap();
    let save = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("runtime")).unwrap();
    let layout = Layout::new(root.path().to_path_buf());
    // 目录服务不可达：online 会在后台报错，但槽位先建出来了
    let result = transfer::go_online(
        recorder.sink(),
        &layout,
        CODE,
        save.path(),
        "测试设备",
        false,
        net("http://127.0.0.1:1"),
    );
    assert!(result.is_ok(), "上线应能先建会话");
    let status = transfer::status(&layout);
    assert_eq!(status["online"], Value::Bool(true));
    assert!(!status["deviceId"].as_str().unwrap_or_default().is_empty());
    transfer::go_offline(&layout).unwrap();
    assert_eq!(transfer::status(&layout)["online"], Value::Bool(false));
}

#[test]
fn folder_transfer_roundtrip_with_progress() {
    let directory_url = start_mock_pkarr();

    // 发送侧：一个含子目录的小文件夹
    let source = tempfile::tempdir().unwrap();
    let hello = "你好，传输助手！".repeat(2000);
    std::fs::write(source.path().join("a.txt"), hello.as_bytes()).unwrap();
    std::fs::create_dir_all(source.path().join("sub/deep")).unwrap();
    std::fs::write(source.path().join("sub/deep/b.bin"), vec![7u8; 300_000]).unwrap();
    let items = vec![
        TransferItem {
            rel: "a.txt".to_string(),
            size: hello.as_bytes().len() as u64,
        },
        TransferItem {
            rel: "sub/deep/b.bin".to_string(),
            size: 300_000,
        },
    ];

    // 接收侧：自己的 runtime（身份 / 口令都落在那儿），上线待命
    let save = tempfile::tempdir().unwrap();
    let receiver = Recorder::default();
    let receiver_root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(receiver_root.path().join("runtime")).unwrap();
    let receiver_layout = Layout::new(receiver_root.path().to_path_buf());
    let receive_info = transfer::go_online(
        receiver.sink(),
        &receiver_layout,
        CODE,
        save.path(),
        "接收机",
        true, // 自动接收：这一版默认关，测试里直接放行
        net(&directory_url),
    )
    .expect("接收侧上线");
    let receiver_id = receive_info["deviceId"].as_str().unwrap().to_string();

    // 发送侧：也上线，等看到接收机
    let sender = Recorder::default();
    let sender_root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(sender_root.path().join("runtime")).unwrap();
    let sender_layout = Layout::new(sender_root.path().to_path_buf());
    transfer::go_online(
        sender.sink(),
        &sender_layout,
        CODE,
        save.path(),
        "发送机",
        false,
        net(&directory_url),
    )
    .expect("发送侧上线");
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        let devices = transfer::devices(&sender_layout);
        if devices["devices"]
            .as_array()
            .map(|list| list.iter().any(|item| item["id"] == Value::String(receiver_id.clone())))
            .unwrap_or(false)
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    assert!(
        transfer::devices(&sender_layout)["devices"]
            .as_array()
            .map(|list| list.iter().any(|item| item["id"] == Value::String(receiver_id.clone())))
            .unwrap_or(false),
        "房间列表里应该看得到接收机"
    );

    let send_id = transfer::send(
        &sender_layout,
        &receiver_id,
        TransferPayload::Files {
            root: Some(source.path().to_string_lossy().to_string()),
            items: items.clone(),
        },
    )
    .expect("发送应能建会话");

    let done = sender.wait_for(&send_id, "done", Duration::from_secs(60));
    assert!(
        done.is_some(),
        "发送侧没等到 done：{:?}",
        sender
            .events()
            .iter()
            .map(|event| format!("{}/{}", event["kind"], event["message"]))
            .collect::<Vec<_>>()
    );
    // 接收侧的子会话：等任意一条 done（id 由 core 生成，这里按 kind 找）
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut received: Option<Value> = None;
    while Instant::now() < deadline {
        received = receiver
            .events()
            .into_iter()
            .find(|event| event["kind"] == "done" && event["role"] == "receive");
        if received.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let summary = received.unwrap_or_else(|| {
        panic!(
            "接收侧没等到 done：{:?}",
            receiver
                .events()
                .iter()
                .map(|event| format!("{}/{}", event["kind"], event["message"]))
                .collect::<Vec<_>>()
        )
    });
    assert_eq!(summary["files"], 2);
    // 接收确认卡：自动接收开着也会推一条 request（界面拿它做提示）
    assert!(receiver.kinds().iter().any(|kind| kind == "request"));

    // 落盘内容与目录结构
    let landed = std::fs::read_to_string(save.path().join("a.txt")).unwrap();
    assert_eq!(landed, hello);
    let blob = std::fs::read(save.path().join("sub/deep/b.bin")).unwrap();
    assert_eq!(blob.len(), 300_000);
    assert!(blob.iter().all(|byte| *byte == 7));

    // 每个文件都有进度与完成事件，且进度单调到顶
    let receive_session = summary["sessionId"].as_str().unwrap().to_string();
    for (index, rel) in [(0, "a.txt"), (1, "sub/deep/b.bin")] {
        let progress: Vec<u64> = receiver
            .events()
            .iter()
            .filter(|event| {
                event["sessionId"].as_str() == Some(receive_session.as_str())
                    && event["kind"] == "progress"
                    && event["index"].as_u64() == Some(index)
            })
            .map(|event| event["sent"].as_u64().unwrap())
            .collect();
        assert!(
            progress.windows(2).all(|pair| pair[0] <= pair[1]),
            "{rel} 的进度不单调"
        );
        assert_eq!(
            progress.last().copied().unwrap_or(0),
            items[index as usize].size,
            "{rel} 的进度没到顶"
        );
        assert!(receiver
            .events()
            .iter()
            .any(|event| event["kind"] == "fileDone" && event["index"].as_u64() == Some(index)));
    }
    // 发送侧也看到了 connected 与对方的 id
    assert!(sender.kinds().contains(&"connected".to_string()));

    // 历史：两边各记了一条
    let sender_log = transfer::history(&sender_layout);
    assert!(
        sender_log["entries"]
            .as_array()
            .map(|list| !list.is_empty())
            .unwrap_or(false),
        "发送侧应留下一条历史"
    );
    let receiver_log = transfer::history(&receiver_layout);
    assert!(
        receiver_log["entries"]
            .as_array()
            .map(|list| !list.is_empty())
            .unwrap_or(false),
        "接收侧应留下一条历史"
    );
    assert_eq!(
        receiver_log["entries"][0]["direction"], "receive",
        "接收侧那条历史的 direction 应为 receive"
    );

    transfer::go_offline(&receiver_layout).unwrap();
    transfer::go_offline(&sender_layout).unwrap();
}

#[test]
fn text_message_roundtrip() {
    let directory_url = start_mock_pkarr();
    let save = tempfile::tempdir().unwrap();
    let receiver = Recorder::default();
    let receiver_root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(receiver_root.path().join("runtime")).unwrap();
    let receiver_layout = Layout::new(receiver_root.path().to_path_buf());
    let info = transfer::go_online(
        receiver.sink(),
        &receiver_layout,
        CODE,
        save.path(),
        "接收机",
        true,
        net(&directory_url),
    )
    .unwrap();
    let receiver_id = info["deviceId"].as_str().unwrap().to_string();

    let sender = Recorder::default();
    let sender_root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(sender_root.path().join("runtime")).unwrap();
    let sender_layout = Layout::new(sender_root.path().to_path_buf());
    transfer::go_online(
        sender.sink(),
        &sender_layout,
        CODE,
        save.path(),
        "发送机",
        false,
        net(&directory_url),
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        let seen = transfer::devices(&sender_layout)["devices"]
            .as_array()
            .map(|list| list.iter().any(|item| item["id"] == Value::String(receiver_id.clone())))
            .unwrap_or(false);
        if seen {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }

    let send_id = transfer::send(
        &sender_layout,
        &receiver_id,
        TransferPayload::Text {
            text: "一段测试文本".to_string(),
        },
    )
    .unwrap();
    assert!(sender.wait_for(&send_id, "done", Duration::from_secs(30)).is_some());
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut got: Option<Value> = None;
    while Instant::now() < deadline {
        got = receiver
            .events()
            .into_iter()
            .find(|event| event["kind"] == "text" && event["received"] == Value::Bool(true));
        if got.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let event = got.unwrap_or_else(|| {
        panic!(
            "接收侧应收到 text 事件：{:?}",
            receiver
                .events()
                .iter()
                .map(|event| format!("{}/{}/{}", event["kind"], event["received"], event["message"]))
                .collect::<Vec<_>>()
        )
    });
    assert_eq!(event["text"], "一段测试文本");
    // 文本不进保存位置
    assert_eq!(
        std::fs::read_dir(save.path()).unwrap().count(),
        0,
        "文本消息不该在保存目录里落任何文件"
    );
    transfer::go_offline(&receiver_layout).unwrap();
    transfer::go_offline(&sender_layout).unwrap();
}

#[test]
fn rendezvous_zone_is_the_code_itself() {
    // 同一句口令在任意设备派生出同一把 rendezvous 密钥（pkarr 的 zone 就是它的公钥），
    // 不同口令的 zone 互不可见——这就是「口令即房间」的全部安全边界。
    let a1 = transfer::room_secret(CODE);
    let a2 = transfer::room_secret(CODE);
    let b = transfer::room_secret("another-code-1");
    assert_eq!(a1.public(), a2.public());
    assert_ne!(a1.public(), b.public());
    // 首尾空白不算口令的一部分：trim 发生在 validate_code，之后派生的房间才稳定
    assert_eq!(
        transfer::room_secret(&transfer::validate_code("  kxtodo-x  ").unwrap()).public(),
        transfer::room_secret("kxtodo-x").public()
    );
}
