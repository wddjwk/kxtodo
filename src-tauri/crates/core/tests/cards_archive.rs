//! 一般卡片 Markdown 压缩包（cards_archive）往返测试：命名、插图随包、引用归一、
//! 网络图片不碰、护栏。

use std::io::{Cursor, Read, Write};

use kxtodo_core::cards_archive;
use kxtodo_core::model::Item;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

fn item(id: &str, markdown: &str, created_at: &str) -> Item {
    serde_json::from_value(serde_json::json!({
        "id": id,
        "nodeId": "n1",
        "order": 1.0,
        "markdown": markdown,
        "createdAt": created_at
    }))
    .expect("item json")
}

fn zip_names(bytes: &[u8]) -> Vec<String> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut names = Vec::new();
    for index in 0..archive.len() {
        let file = archive.by_index(index).expect("entry");
        names.push(file.name().to_string());
    }
    names
}

#[test]
fn round_trip_keeps_images_and_normalizes_refs() {
    let a = item(
        "t1",
        "第一张卡片正文内容足够长\n\n![](pic.png)",
        "2026-09-01T10:00:00+08:00",
    );
    let b = item("t2", "第二张", "2026-09-01T11:00:00+08:00");
    let refs = [&a, &b];
    let zip = cards_archive::build_zip(&refs, &|name| {
        (name == "pic.png").then(|| vec![1, 2, 3])
    })
    .expect("build");

    // 命名 = 日期 + 正文前 10 个字符；插图进 images/
    let names = zip_names(&zip);
    assert!(
        names.iter().any(|n| n == "20260901_第一张卡片正文内容足.md"),
        "names: {names:?}"
    );
    assert!(names.iter().any(|n| n == "images/pic.png"), "names: {names:?}");

    let parsed = cards_archive::parse_zip(&zip).expect("parse");
    assert_eq!(parsed.cards.len(), 2);
    // 导出改写成包内相对路径，导入归一回裸文件名（包里有这张图）
    assert!(parsed.cards[0].contains("![](pic.png)"), "{}", parsed.cards[0]);
    assert_eq!(parsed.images.get("pic.png").map(Vec::len), Some(3));
}

#[test]
fn remote_images_are_left_alone() {
    let a = item("t1", "正文\n![](https://example.com/x.png)", "2026-09-02T10:00:00+08:00");
    let refs = [&a];
    let zip = cards_archive::build_zip(&refs, &|_| Some(vec![9])).expect("build");
    // 网络图片不进包
    assert!(!zip_names(&zip).iter().any(|n| n.starts_with("images/")));
    let parsed = cards_archive::parse_zip(&zip).expect("parse");
    assert!(parsed.cards[0].contains("https://example.com/x.png"));
    assert!(parsed.images.is_empty());
}

#[test]
fn colliding_names_get_index_suffix() {
    let a = item("t1", "同样的正文内容一二三四", "2026-09-03T10:00:00+08:00");
    let b = item("t2", "同样的正文内容一二三四", "2026-09-03T11:00:00+08:00");
    let refs = [&a, &b];
    let zip = cards_archive::build_zip(&refs, &|_| None).expect("build");
    let names = zip_names(&zip);
    assert!(names.contains(&"20260903_同样的正文内容一二三.md".to_string()), "{names:?}");
    assert!(names.contains(&"20260903_同样的正文内容一二三_2.md".to_string()), "{names:?}");
}

#[test]
fn garbage_is_rejected() {
    let error = cards_archive::parse_zip(b"not a zip at all").unwrap_err();
    assert_eq!(error.code, "CARDS_IMPORT_INVALID");
    let error = cards_archive::parse_zip(&[]).unwrap_err();
    assert_eq!(error.code, "CARDS_IMPORT_INVALID");
}

#[test]
fn image_bytes_survive_verbatim() {
    let payload: Vec<u8> = (0..=255u8).cycle().take(4096).collect();
    let moved = payload.clone();
    let a = item("t1", "![](big.bin.png)", "2026-09-04T10:00:00+08:00");
    let refs = [&a];
    let zip = cards_archive::build_zip(&refs, &|name| {
        (name == "big.bin.png").then(|| moved.clone())
    })
    .expect("build");
    let mut archive = ZipArchive::new(Cursor::new(&zip)).expect("zip");
    let mut file = archive.by_name("images/big.bin.png").expect("image entry");
    let mut raw = Vec::new();
    file.read_to_end(&mut raw).expect("read");
    assert_eq!(raw, payload);
}

#[test]
fn parse_returns_only_referenced_images() {
    // 手工搭一个包：两张 md（一张引用 used.png，一张不带图）+ 两张图（另一张没人引用）。
    // 没人引用的图不该进解析结果——否则跟着落盘就是条目插图目录里的孤儿图（v0.7.2）。
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer.start_file("20260905_带图.md", options).expect("start md");
    writer
        .write_all("第一张卡片\n\n![](images/used.png)".as_bytes())
        .expect("write md");
    writer.start_file("20260905_无图.md", options).expect("start md2");
    writer
        .write_all("第二张卡片，没有图".as_bytes())
        .expect("write md2");
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    writer.start_file("images/used.png", stored).expect("start img");
    writer.write_all(&[1, 2, 3]).expect("write img");
    writer.start_file("images/orphan.png", stored).expect("start orphan");
    writer.write_all(&[4, 5, 6]).expect("write orphan");
    let zip = writer.finish().expect("finish").into_inner();

    let parsed = cards_archive::parse_zip(&zip).expect("parse");
    assert_eq!(parsed.cards.len(), 2, "一个 md 一张卡片");
    assert!(
        parsed.cards[0].contains("![](used.png)"),
        "引用归一回裸文件名：{}",
        parsed.cards[0]
    );
    assert_eq!(parsed.images.len(), 1, "只带被引用的图");
    assert_eq!(parsed.images.get("used.png").map(Vec::len), Some(3));
    assert!(!parsed.images.contains_key("orphan.png"), "没人引用的图不进结果");
}
