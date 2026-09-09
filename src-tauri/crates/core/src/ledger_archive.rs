//! 记账的 Excel 导出/导入：zip 包内含一张 `kxtodo-ledger.xlsx`，四张表
//! （说明 / 账户 / 分类 / 账目）。
//!
//! 选 Excel 而不是 csv/md 的理由：账目天然是**多张关联表**（账户、分类、流水），
//! 单表 csv 表达不了；而 Excel 是用户唯一会自己打开核对的格式。导入只认这套表头，
//! 但缺列容忍（缺「转入账户」列就导不了转账，其余照进）。
//!
//! 写用 rust_xlsxwriter、读用 calamine，两者都是纯 Rust，Android 交叉编译零负担。

use std::io::{Cursor, Read, Write};

use calamine::{open_workbook_from_rs, Data, Range, Reader, Xlsx};
use rust_xlsxwriter::{Format, Workbook};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::error::{CoreError, CoreResult};
use crate::model::{AccountKind, LedgerCategory, LedgerEntry, LedgerFile, LedgerKind, LedgerSide};
use crate::ops_ledger::cents_to_yuan;

/// 导入护栏：与日记压缩包同一套口径。
const MAX_ARCHIVE_BYTES: usize = 200 * 1024 * 1024;
const MAX_FILES: usize = 20_000;
const MAX_FILE_BYTES: u64 = 20 * 1024 * 1024;
/// 账目行上限（一天记一百笔、记十年也才三万多行）
const MAX_ENTRY_ROWS: usize = 200_000;

/// 包内 xlsx 的固定名字（zip 里就这一个文件）
const XLSX_NAME: &str = "kxtodo-ledger.xlsx";
/// 说明表第一行的格式标记，导入靠它认「这是我们设计好的表格」
pub const FORMAT_MARKER: &str = "KXTODO_LEDGER_V1";

const SHEET_README: &str = "说明";
const SHEET_ACCOUNTS: &str = "账户";
const SHEET_CATEGORIES: &str = "分类";
const SHEET_ENTRIES: &str = "账目";

// ---------------------------------------------------------------------------
// 导出
// ---------------------------------------------------------------------------

pub fn archive_name(from: Option<&str>, to: Option<&str>) -> String {
    let mut name = format!("kxtodo-ledger-{}", crate::core::APP_VERSION);
    match (from, to) {
        (Some(from), Some(to)) => name.push_str(&format!("-{from}-{to}")),
        (Some(from), None) => name.push_str(&format!("-from-{from}")),
        (None, Some(to)) => name.push_str(&format!("-to-{to}")),
        (None, None) => {}
    }
    format!("{name}.zip")
}

/// 导出包 = zip( kxtodo-ledger.xlsx )。zip 是为了和日记/卡片导出的「一个包」手感一致，
/// 也给将来塞附件留位置；导入同时接受裸 xlsx。
pub fn build_zip(file: &LedgerFile, entries: &[&LedgerEntry]) -> CoreResult<Vec<u8>> {
    let xlsx = build_xlsx(file, entries)?;
    // xlsx 本身已是 zip，二次 Deflate 只烧 CPU 不换体积
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .start_file(XLSX_NAME, options)
        .map_err(|error| CoreError::internal(format!("写入压缩包失败：{error}")))?;
    writer
        .write_all(&xlsx)
        .map_err(|error| CoreError::internal(format!("写入压缩包失败：{error}")))?;
    let cursor = writer
        .finish()
        .map_err(|error| CoreError::internal(format!("收尾压缩包失败：{error}")))?;
    Ok(cursor.into_inner())
}

fn build_xlsx(file: &LedgerFile, entries: &[&LedgerEntry]) -> CoreResult<Vec<u8>> {
    let mut workbook = Workbook::new();
    let head_format = Format::new().set_bold();
    let money_format = Format::new().set_num_format("0.00");
    let io = |error: rust_xlsxwriter::XlsxError| CoreError::internal(format!("生成 Excel 失败：{error}"));

    // --- 说明 ---
    let sheet = workbook.add_worksheet();
    sheet.set_name(SHEET_README).map_err(io)?;
    let mut income = 0i64;
    let mut expense = 0i64;
    for entry in entries {
        match entry.kind {
            LedgerKind::Income => income += entry.amount_cents,
            LedgerKind::Expense => expense += entry.amount_cents,
            LedgerKind::Transfer => {}
        }
    }
    let readme: Vec<String> = vec![
        FORMAT_MARKER.to_string(),
        String::new(),
        "本文件由 KXToDo 记账导出，可改可核，改完可直接导入回 KXToDo（只认这套表头）。".to_string(),
        format!("账目笔数：{}", entries.len()),
        format!("收入合计：{} 元", cents_to_yuan(income)),
        format!("支出合计：{} 元", cents_to_yuan(expense)),
        format!("结余：{} 元", cents_to_yuan(income - expense)),
        String::new(),
        format!("「{SHEET_ACCOUNTS}」表：名称 | 类型 | 图标 | 颜色 | 期初余额 | 备注"),
        format!("「{SHEET_CATEGORIES}」表：收支 | 大类 | 分类 | 图标 | 颜色（大类行的「分类」留空）"),
        format!("「{SHEET_ENTRIES}」表：日期 | 时间 | 类型 | 账户 | 转入账户 | 大类 | 分类 | 金额 | 备注"),
        "金额为带符号的元：支出为负、收入为正；转账只填转出账户与转入账户，不计入收支合计。".to_string(),
        "日期请保持 YYYY-MM-DD 文本；导入时账户/分类按名字匹配，缺的会自动创建。".to_string(),
    ];
    for (row, line) in readme.iter().enumerate() {
        sheet.write_string(row as u32, 0, line).map_err(io)?;
    }
    sheet.set_column_width(0, 96).map_err(io)?;

    // --- 账户 ---
    let sheet = workbook.add_worksheet();
    sheet.set_name(SHEET_ACCOUNTS).map_err(io)?;
    write_header(sheet, &head_format, &["名称", "类型", "图标", "颜色", "期初余额", "备注"])?;
    let mut accounts = file.accounts.clone();
    accounts.sort_by(|a, b| a.order.partial_cmp(&b.order).unwrap_or(std::cmp::Ordering::Equal));
    for (index, account) in accounts.iter().enumerate() {
        let row = index as u32 + 1;
        sheet.write_string(row, 0, &account.name).map_err(io)?;
        sheet.write_string(row, 1, account.kind.as_str()).map_err(io)?;
        sheet.write_string(row, 2, &account.icon).map_err(io)?;
        sheet.write_string(row, 3, &account.color).map_err(io)?;
        sheet
            .write_number_with_format(row, 4, account.initial_cents as f64 / 100.0, &money_format)
            .map_err(io)?;
        sheet.write_string(row, 5, &account.note).map_err(io)?;
    }

    // --- 分类 ---
    let sheet = workbook.add_worksheet();
    sheet.set_name(SHEET_CATEGORIES).map_err(io)?;
    write_header(sheet, &head_format, &["收支", "大类", "分类", "图标", "颜色"])?;
    let mut categories = file.categories.clone();
    categories.sort_by(|a, b| {
        a.side
            .as_str()
            .cmp(b.side.as_str())
            .then(a.order.partial_cmp(&b.order).unwrap_or(std::cmp::Ordering::Equal))
    });
    let parent_name = |id: &str| -> String {
        categories
            .iter()
            .find(|item| item.id == id)
            .map(|item| item.name.clone())
            .unwrap_or_default()
    };
    for (index, category) in categories.iter().enumerate() {
        let row = index as u32 + 1;
        let (major, minor) = match &category.parent_id {
            Some(parent) => (parent_name(parent), category.name.clone()),
            None => (category.name.clone(), String::new()),
        };
        sheet.write_string(row, 0, side_label(category.side)).map_err(io)?;
        sheet.write_string(row, 1, &major).map_err(io)?;
        sheet.write_string(row, 2, &minor).map_err(io)?;
        sheet.write_string(row, 3, &category.icon).map_err(io)?;
        sheet.write_string(row, 4, &category.color).map_err(io)?;
    }

    // --- 账目 ---
    let sheet = workbook.add_worksheet();
    sheet.set_name(SHEET_ENTRIES).map_err(io)?;
    write_header(
        sheet,
        &head_format,
        &["日期", "时间", "类型", "账户", "转入账户", "大类", "分类", "金额", "备注"],
    )?;
    let widths = [12f32, 10f32, 8f32, 18f32, 18f32, 12f32, 12f32, 12f32, 40f32];
    for (col, width) in widths.iter().enumerate() {
        sheet.set_column_width(col as u16, *width).map_err(io)?;
    }
    for (index, entry) in entries.iter().enumerate() {
        let row = index as u32 + 1;
        let account = file
            .accounts
            .iter()
            .find(|item| item.id == entry.account_id)
            .map(|item| item.name.clone())
            .unwrap_or_default();
        let to_account = entry
            .to_account_id
            .as_deref()
            .and_then(|id| file.accounts.iter().find(|item| item.id == id))
            .map(|item| item.name.clone())
            .unwrap_or_default();
        let (major, minor) = match entry
            .category_id
            .as_deref()
            .and_then(|id| file.categories.iter().find(|item| item.id == id))
        {
            Some(category) => match &category.parent_id {
                Some(parent) => (parent_name(parent), category.name.clone()),
                None => (category.name.clone(), String::new()),
            },
            None => (String::new(), String::new()),
        };
        let signed = match entry.kind {
            LedgerKind::Income => entry.amount_cents,
            _ => -entry.amount_cents,
        };
        sheet.write_string(row, 0, &entry.date).map_err(io)?;
        sheet.write_string(row, 1, &entry.time).map_err(io)?;
        sheet.write_string(row, 2, kind_label(entry.kind)).map_err(io)?;
        sheet.write_string(row, 3, &account).map_err(io)?;
        sheet.write_string(row, 4, &to_account).map_err(io)?;
        sheet.write_string(row, 5, &major).map_err(io)?;
        sheet.write_string(row, 6, &minor).map_err(io)?;
        sheet
            .write_number_with_format(row, 7, signed as f64 / 100.0, &money_format)
            .map_err(io)?;
        sheet.write_string(row, 8, &entry.note).map_err(io)?;
    }

    workbook.save_to_buffer().map_err(io)
}

fn write_header(
    sheet: &mut rust_xlsxwriter::Worksheet,
    format: &Format,
    headers: &[&str],
) -> CoreResult<()> {
    for (col, head) in headers.iter().enumerate() {
        sheet
            .write_string_with_format(0, col as u16, *head, format)
            .map_err(|error| CoreError::internal(format!("写表头失败：{error}")))?;
    }
    Ok(())
}

fn side_label(side: LedgerSide) -> &'static str {
    match side {
        LedgerSide::Expense => "支出",
        LedgerSide::Income => "收入",
    }
}

fn kind_label(kind: LedgerKind) -> &'static str {
    match kind {
        LedgerKind::Expense => "支出",
        LedgerKind::Income => "收入",
        LedgerKind::Transfer => "转账",
    }
}

// ---------------------------------------------------------------------------
// 导入
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct ImportAccount {
    pub name: String,
    pub kind: AccountKind,
    pub icon: String,
    pub color: String,
    pub initial_cents: i64,
}

#[derive(Debug, Clone, Default)]
pub struct ImportCategory {
    pub side: LedgerSide,
    /// 大类名；None = 自己就是大类
    pub parent: Option<String>,
    pub name: String,
    pub icon: String,
    pub color: String,
}

#[derive(Debug, Clone, Default)]
pub struct ImportEntry {
    pub kind: LedgerKind,
    pub amount_cents: i64,
    pub account: String,
    pub to_account: Option<String>,
    /// 大类名（导入时先确保它存在）
    pub category_parent: Option<String>,
    /// 子分类名；None = 记在大类上
    pub category: Option<String>,
    pub date: String,
    pub time: String,
    pub note: String,
}

#[derive(Debug, Clone, Default)]
pub struct ParsedLedger {
    pub accounts: Vec<ImportAccount>,
    pub categories: Vec<ImportCategory>,
    pub entries: Vec<ImportEntry>,
}

/// 接受 zip 包（取包内第一张 xlsx）或裸 xlsx。
pub fn parse_bytes(bytes: &[u8]) -> CoreResult<ParsedLedger> {
    if bytes.is_empty() {
        return Err(CoreError::validation("LEDGER_IMPORT_INVALID", "导出包是空的"));
    }
    if bytes.len() > MAX_ARCHIVE_BYTES {
        return Err(CoreError::validation(
            "LEDGER_IMPORT_TOO_LARGE",
            format!("导出包超过 {}MB 上限", MAX_ARCHIVE_BYTES / 1024 / 1024),
        ));
    }
    let xlsx = if bytes.starts_with(b"PK") {
        extract_xlsx(bytes)?
    } else {
        bytes.to_vec()
    };
    parse_xlsx(&xlsx)
}

fn extract_xlsx(bytes: &[u8]) -> CoreResult<Vec<u8>> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|error| {
        CoreError::validation("LEDGER_IMPORT_INVALID", format!("不是有效的 zip 压缩包：{error}"))
    })?;
    if archive.len() > MAX_FILES {
        return Err(CoreError::validation(
            "LEDGER_IMPORT_TOO_LARGE",
            format!("压缩包内文件数超过 {MAX_FILES} 上限"),
        ));
    }
    // 优先固定名，其次包内第一张 xlsx
    let mut fallback: Option<usize> = None;
    for index in 0..archive.len() {
        let Ok(file) = archive.by_index(index) else { continue };
        if file.is_dir() {
            continue;
        }
        let name = file.name().to_string();
        let is_xlsx = name.to_ascii_lowercase().ends_with(".xlsx");
        let is_target = name == XLSX_NAME;
        drop(file);
        if !is_xlsx {
            continue;
        }
        if is_target {
            let file = archive.by_index(index).map_err(|error| {
                CoreError::validation("LEDGER_IMPORT_INVALID", format!("读压缩包失败：{error}"))
            })?;
            return read_entry(file);
        }
        if fallback.is_none() {
            fallback = Some(index);
        }
    }
    let Some(index) = fallback else {
        return Err(CoreError::validation(
            "LEDGER_IMPORT_INVALID",
            "压缩包里没有 xlsx 表格（不是 KXToDo 的记账导出包）",
        ));
    };
    let file = archive.by_index(index).map_err(|error| {
        CoreError::validation("LEDGER_IMPORT_INVALID", format!("读压缩包失败：{error}"))
    })?;
    read_entry(file)
}

fn read_entry<R: Read>(mut file: R) -> CoreResult<Vec<u8>> {
    let mut raw = Vec::new();
    file.read_to_end(&mut raw).map_err(|error| {
        CoreError::validation("LEDGER_IMPORT_INVALID", format!("读压缩包失败：{error}"))
    })?;
    if raw.len() as u64 > MAX_FILE_BYTES {
        return Err(CoreError::validation(
            "LEDGER_IMPORT_TOO_LARGE",
            "包内表格超过 20MB 上限",
        ));
    }
    Ok(raw)
}

fn cell_text(cell: &Data) -> String {
    match cell {
        Data::String(text) => text.trim().to_string(),
        Data::Int(value) => value.to_string(),
        Data::Float(value) => {
            if (value - value.round()).abs() < f64::EPSILON {
                value.round().to_string()
            } else {
                format!("{value:.2}")
            }
        }
        Data::Bool(value) => value.to_string(),
        Data::DateTimeIso(text) => text.trim().to_string(),
        _ => String::new(),
    }
}

fn cell_cents(cell: &Data) -> Option<i64> {
    match cell {
        Data::Int(value) => Some(value.saturating_mul(100)),
        Data::Float(value) => Some((value * 100.0).round() as i64),
        Data::String(text) => crate::ops_ledger::parse_cents(text),
        _ => None,
    }
}

struct Columns {
    header: usize,
    index: Vec<(String, usize)>,
}

/// 在前 10 行里找包含全部必需列名的表头行，返回行号与 列名→列号；可选列缺了就跳过。
fn header_columns(range: &Range<Data>, wanted: &[&str], required: &[&str]) -> Option<Columns> {
    for (row_index, row) in range.rows().take(10).enumerate() {
        let texts: Vec<String> = row.iter().map(cell_text).collect();
        let has_required = required.iter().all(|name| texts.iter().any(|text| text == *name));
        if !has_required {
            continue;
        }
        let index: Vec<(String, usize)> = wanted
            .iter()
            .filter_map(|name| {
                texts
                    .iter()
                    .position(|text| text == *name)
                    .map(|col| (name.to_string(), col))
            })
            .collect();
        return Some(Columns {
            header: row_index,
            index,
        });
    }
    None
}

fn get_text(row: &[Data], index: &[(String, usize)], name: &str) -> String {
    index
        .iter()
        .find(|(key, _)| key == name)
        .and_then(|(_, col)| row.get(*col))
        .map(cell_text)
        .unwrap_or_default()
}

fn get_cents(row: &[Data], index: &[(String, usize)], name: &str) -> i64 {
    index
        .iter()
        .find(|(key, _)| key == name)
        .and_then(|(_, col)| row.get(*col))
        .and_then(cell_cents)
        .unwrap_or(0)
}

fn parse_xlsx(bytes: &[u8]) -> CoreResult<ParsedLedger> {
    let mut workbook: Xlsx<_> =
        open_workbook_from_rs(Cursor::new(bytes.to_vec())).map_err(|error| {
            CoreError::validation("LEDGER_IMPORT_INVALID", format!("不是有效的 Excel 表格：{error}"))
        })?;
    let names = workbook.sheet_names();
    let mut out = ParsedLedger::default();

    // --- 账户 ---
    if names.iter().any(|name| name == SHEET_ACCOUNTS) {
        if let Ok(range) = workbook.worksheet_range(SHEET_ACCOUNTS) {
            if let Some(columns) = header_columns(
                &range,
                &["名称", "类型", "图标", "颜色", "期初余额", "备注"],
                &["名称"],
            ) {
                for row in range.rows().skip(columns.header + 1) {
                    let name = get_text(row, &columns.index, "名称");
                    if name.is_empty() {
                        continue;
                    }
                    out.accounts.push(ImportAccount {
                        name,
                        kind: AccountKind::parse(&get_text(row, &columns.index, "类型"))
                            .unwrap_or_default(),
                        icon: get_text(row, &columns.index, "图标"),
                        color: get_text(row, &columns.index, "颜色"),
                        initial_cents: get_cents(row, &columns.index, "期初余额"),
                    });
                }
            }
        }
    }

    // --- 分类 ---
    if names.iter().any(|name| name == SHEET_CATEGORIES) {
        if let Ok(range) = workbook.worksheet_range(SHEET_CATEGORIES) {
            if let Some(columns) =
                header_columns(&range, &["收支", "大类", "分类", "图标", "颜色"], &["收支", "大类"])
            {
                for row in range.rows().skip(columns.header + 1) {
                    let Some(side) = LedgerSide::parse(&get_text(row, &columns.index, "收支")) else {
                        continue;
                    };
                    let major = get_text(row, &columns.index, "大类");
                    let minor = get_text(row, &columns.index, "分类");
                    let icon = get_text(row, &columns.index, "图标");
                    let color = get_text(row, &columns.index, "颜色");
                    if minor.is_empty() {
                        if major.is_empty() {
                            continue;
                        }
                        out.categories.push(ImportCategory {
                            side,
                            parent: None,
                            name: major,
                            icon,
                            color,
                        });
                    } else {
                        out.categories.push(ImportCategory {
                            side,
                            parent: Some(major),
                            name: minor,
                            icon,
                            color,
                        });
                    }
                }
            }
        }
    }

    // --- 账目 ---
    let entries_sheet = if names.iter().any(|name| name == SHEET_ENTRIES) {
        Some(SHEET_ENTRIES.to_string())
    } else {
        // 容忍：有人把表另存成单张表，找第一张带「日期」+「金额」表头的
        let mut found = None;
        for name in &names {
            let Ok(range) = workbook.worksheet_range(name) else { continue };
            let hit = range.rows().take(10).any(|row| {
                let texts: Vec<String> = row.iter().map(cell_text).collect();
                texts.iter().any(|text| text == "日期") && texts.iter().any(|text| text == "金额")
            });
            if hit {
                found = Some(name.clone());
                break;
            }
        }
        found
    };
    let Some(sheet_name) = entries_sheet else {
        return Err(CoreError::validation(
            "LEDGER_IMPORT_INVALID",
            "找不到「账目」表（不是 KXToDo 的记账导出包）",
        ));
    };
    let range = workbook.worksheet_range(&sheet_name).map_err(|error| {
        CoreError::validation("LEDGER_IMPORT_INVALID", format!("读「账目」表失败：{error}"))
    })?;
    let columns = header_columns(
        &range,
        &["日期", "时间", "类型", "账户", "转入账户", "大类", "分类", "金额", "备注"],
        &["日期", "账户", "金额"],
    )
    .ok_or_else(|| {
        CoreError::validation(
            "LEDGER_IMPORT_INVALID",
            "「账目」表缺少表头行（日期/账户/金额）",
        )
    })?;
    for row in range.rows().skip(columns.header + 1) {
        if out.entries.len() >= MAX_ENTRY_ROWS {
            break;
        }
        let date = normalize_date_text(&get_text(row, &columns.index, "日期"));
        if date.is_empty() {
            continue;
        }
        let account = get_text(row, &columns.index, "账户");
        if account.is_empty() {
            continue;
        }
        let amount = get_cents(row, &columns.index, "金额");
        if amount == 0 {
            continue;
        }
        let kind = match LedgerKind::parse(&get_text(row, &columns.index, "类型")) {
            Some(kind) => kind,
            // 类型列缺省时按金额符号猜：负 = 支出，正 = 收入
            None => {
                if amount < 0 {
                    LedgerKind::Expense
                } else {
                    LedgerKind::Income
                }
            }
        };
        let to_account = get_text(row, &columns.index, "转入账户");
        let major = get_text(row, &columns.index, "大类");
        let minor = get_text(row, &columns.index, "分类");
        out.entries.push(ImportEntry {
            kind,
            amount_cents: amount.abs(),
            account,
            to_account: if to_account.is_empty() { None } else { Some(to_account) },
            category_parent: if major.is_empty() { None } else { Some(major) },
            category: if minor.is_empty() { None } else { Some(minor) },
            date,
            time: get_text(row, &columns.index, "时间"),
            note: get_text(row, &columns.index, "备注"),
        });
    }
    Ok(out)
}

/// 容忍 2026/07/10、2026-7-10、20260710 三种写法，归一成 YYYY-MM-DD；非法返回空串。
fn normalize_date_text(raw: &str) -> String {
    let text = raw.trim().replace('/', "-").replace('.', "-");
    let digits: String = text.chars().filter(|c| c.is_ascii_digit()).collect();
    let parts: Vec<&str> = text.split('-').collect();
    if parts.len() == 3 && !parts.iter().any(|part| part.is_empty()) {
        let (Ok(year), Ok(month), Ok(day)) = (
            parts[0].parse::<i32>(),
            parts[1].parse::<u32>(),
            parts[2].parse::<u32>(),
        ) else {
            return String::new();
        };
        if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return String::new();
        }
        return format!("{year:04}-{month:02}-{day:02}");
    }
    if digits.len() == 8 {
        let (Ok(year), Ok(month), Ok(day)) = (
            digits[0..4].parse::<i32>(),
            digits[4..6].parse::<u32>(),
            digits[6..8].parse::<u32>(),
        ) else {
            return String::new();
        };
        if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return String::new();
        }
        return format!("{year:04}-{month:02}-{day:02}");
    }
    String::new()
}

/// 导入合并时按两级名字定位分类：先找大类（side + 顶层 + 名字），再在它名下找子分类；
/// 只给大类名时就返回大类本身。
pub fn find_category_by_names(
    categories: &[LedgerCategory],
    side: LedgerSide,
    parent: Option<&str>,
    name: Option<&str>,
) -> Option<String> {
    let Some(parent_name) = parent else {
        let name = name?;
        return categories
            .iter()
            .find(|item| item.name == name && item.side == side)
            .map(|item| item.id.clone());
    };
    let parent_id = categories
        .iter()
        .find(|item| item.name == parent_name && item.side == side && item.parent_id.is_none())
        .map(|item| item.id.clone())?;
    match name {
        Some(child) if !child.is_empty() => categories
            .iter()
            .find(|item| {
                item.name == child && item.side == side && item.parent_id.as_deref() == Some(parent_id.as_str())
            })
            .map(|item| item.id.clone()),
        _ => Some(parent_id),
    }
}
