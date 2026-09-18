//! ledger domain: 记账的业务操作（GUI 与 CLI 共用同一条命令层）。
//!
//! 账本住在自己的 `ledger.json` 里（第五个领域文件），于是记一笔账既不抬高 data 域的
//! revision，也不和任务/日记抢同一把幂等台账。同步上它有独立的「账本」范围勾选。
//!
//! 金额一律**整数分**（i64）：浮点累加在统计里会 drift，而分是记账的最小单位。
//! 对外（CLI/Excel/JSON 输出）同时给 `amountCents` 与两位小数的 `amount` 字符串。

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use chrono::NaiveDate;
use serde_json::{json, Map, Value};

use crate::core::{
    apply_write_outcome, idem_summary, notify_host, param_str, require_confirmation, required_str,
    set_read_revision, unbounded_page_from, ExecContext, Invocation,
};
use crate::diary_archive::is_safe_image_name;
use crate::envelope::Meta;
use crate::error::{CoreError, CoreResult};
use crate::ids::gen_id;
use crate::model::{
    default_account_kind, LedgerAccount, LedgerAccountType, LedgerCategory, LedgerEntry,
    LedgerFile, LedgerKind, LedgerSide, ACCOUNT_KIND_CREDIT, LEDGER_IMAGE_NODE,
};
use crate::ops_task::paginate;
use crate::repo::Domain;
use crate::time::{now_iso, parse_date, today_local};

pub fn ledger_dispatch(
    action: &str,
    inv: &Invocation,
    ctx: &ExecContext,
    meta: &mut Meta,
) -> CoreResult<Value> {
    // 金融数据敏感（v0.7.2）：一切改账本的动作都必须先拿到用户的明确同意。
    // 门收在分发层，GUI/Android 桥恒带 controls.yes = true（GUI 操作即用户确认），不受影响。
    ledger_write_confirmation(action, inv)?;
    match action {
        "add" => ledger_add(inv, ctx, meta),
        "get" => ledger_get(inv, ctx, meta),
        "list" => ledger_list(inv, ctx, meta),
        "modify" => ledger_modify(inv, ctx, meta),
        "remove" => ledger_remove(inv, ctx, meta),
        "transfer" => ledger_transfer(inv, ctx, meta),
        "accounts" => ledger_accounts(inv, ctx, meta),
        "accountAdd" => ledger_account_add(inv, ctx, meta),
        "accountModify" => ledger_account_modify(inv, ctx, meta),
        "accountRemove" => ledger_account_remove(inv, ctx, meta),
        "accountTypes" => ledger_account_types(inv, ctx, meta),
        "accountTypeAdd" => ledger_account_type_add(inv, ctx, meta),
        "accountTypeModify" => ledger_account_type_modify(inv, ctx, meta),
        "accountTypeRemove" => ledger_account_type_remove(inv, ctx, meta),
        "categories" => ledger_categories(inv, ctx, meta),
        "categoryAdd" => ledger_category_add(inv, ctx, meta),
        "categoryModify" => ledger_category_modify(inv, ctx, meta),
        "categoryRemove" => ledger_category_remove(inv, ctx, meta),
        "iconList" => ledger_icon_list(inv, ctx, meta),
        "stats" => ledger_stats(inv, ctx, meta),
        "balance" => ledger_balance(inv, ctx, meta),
        "export" => ledger_export(inv, ctx, meta),
        "import" => ledger_import(inv, ctx, meta),
        other => Err(CoreError::validation(
            "UNKNOWN_ACTION",
            format!("未知 ledger 动作 `{other}`"),
        )),
    }
}

/// 记账写动作的确认门（v0.7.2）：金融数据敏感，Agent 改账本前必须先向用户说明这次
/// 增删改的内容并得到明确同意，同意后追加 --yes 再执行；只读动作永远不设门。
///
/// 为什么 remove / accountRemove / categoryRemove / import **不在这里设卡**：这四个处理器
/// 内部已各有一道更具体的确认门（它们先读账本，说得出要删的那笔金额、账户名、连带笔数、
/// 导入条数），保留信息量更大的那一道；两层门只会用两条不一样的文案各拦一次，反而混乱。
/// 其余写动作没有内部门，统一收在这条分发层的门上——一个动作只有一道门。
fn ledger_write_confirmation(action: &str, inv: &Invocation) -> CoreResult<()> {
    let label = match action {
        "add" => "在账本里记一笔",
        "transfer" => "在账户间转账",
        "modify" => "修改账本里的一笔",
        "accountAdd" => "新增资金账户",
        "accountModify" => "修改资金账户",
        "accountTypeAdd" => "新增账户类型",
        "accountTypeModify" => "修改账户类型",
        "accountTypeRemove" => "删除账户类型",
        "categoryAdd" => "新增记账分类",
        "categoryModify" => "修改记账分类",
        // 只读动作与内部已有确认门的动作（remove/accountRemove/categoryRemove/import）直接放行
        _ => return Ok(()),
    };
    const WHY: &str = "记账数据敏感，改动账本前必须先向用户说明这次增删改的内容并得到明确同意；用户同意后追加 --yes 再执行";
    let params = &inv.params;
    let text = |key: &str| -> String {
        match params.get(key) {
            Some(Value::String(raw)) => raw.trim().to_string(),
            Some(Value::Number(raw)) => raw.to_string(),
            _ => String::new(),
        }
    };
    // 关键字段顺手带上（金额/账户/日期/名称），说明更具体；缺了就空着，不值得为文案翻账本
    let mut bits: Vec<String> = Vec::new();
    for (key, name) in [
        ("amount", "金额"),
        ("account", "账户"),
        ("from", "转出"),
        ("to", "转入"),
        ("date", "日期"),
        ("name", "名称"),
    ] {
        let value = text(key);
        if !value.is_empty() {
            bits.push(format!("{name} {value}"));
        }
    }
    let detail = if bits.is_empty() {
        String::new()
    } else {
        format!("（{}）", bits.join("，"))
    };
    require_confirmation(
        &inv.controls,
        format!("将{label}{detail}。{WHY}"),
        json!({ "type": "ledger", "action": action }),
    )
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// 金额参数 → 整数分。接受字符串 "12.5"/"12.50"/"1,234.567" 或 JSON 数字；
/// 第三位小数四舍五入，绝不用 f64 累加。
fn cents_param(params: &Value, key: &str) -> CoreResult<Option<i64>> {
    let Some(raw) = params.get(key) else {
        return Ok(None);
    };
    let value: i64 = match raw {
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                int
            } else if let Some(float) = number.as_f64() {
                (float * 100.0).round() as i64
            } else {
                return Err(invalid_amount(key, &raw.to_string()));
            }
        }
        Value::String(text) => parse_cents(text).ok_or_else(|| invalid_amount(key, text))?,
        _ => return Err(invalid_amount(key, &raw.to_string())),
    };
    Ok(Some(value))
}

pub(crate) fn parse_cents(raw: &str) -> Option<i64> {
    let text = raw.trim().replace(',', "");
    if text.is_empty() {
        return None;
    }
    let (negative, digits_part) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(&text)),
    };
    let mut parts = digits_part.split('.');
    let whole = parts.next()?;
    let frac = parts.next().unwrap_or("0");
    if parts.next().is_some() {
        return None;
    }
    if whole.is_empty() && frac.is_empty() {
        return None;
    }
    if !whole.chars().all(|c| c.is_ascii_digit()) || !frac.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    // 全是 0（`"0"` / `"000"`）→ 0；否则解析失败就是真的溢出（i128 装不下 39 位以上），
    // **必须拒**：早先这里是 `.unwrap_or(0)`，于是「四十位数字的 --initial」被静默当成 0
    // ——`ledger add` 有 `amount_cents <= 0` 的写入门兜住（只是报错误导），而
    // `account-modify --initial/--balance` 里 0 是合法值，会真的把期初写成 0。
    let trimmed = whole.trim_start_matches('0');
    let whole_units: i128 = if trimmed.is_empty() {
        0
    } else {
        trimmed.parse::<i128>().ok()?
    };
    let whole_cents: i128 = whole_units.checked_mul(100)?;
    // 两位小数截到分，第三位四舍五入
    let frac_digits: String = format!("{frac:0<3}");
    let tenths_hundredths: i128 = frac_digits[0..2].parse().ok()?;
    let third: i128 = frac_digits[2..3].parse().ok()?;
    let mut cents = whole_cents.checked_add(tenths_hundredths)?;
    if third >= 5 {
        cents = cents.checked_add(1)?;
    }
    if negative {
        cents = -cents;
    }
    i64::try_from(cents).ok()
}

fn invalid_amount(key: &str, raw: &str) -> CoreError {
    CoreError::validation(
        "LEDGER_AMOUNT_INVALID",
        format!("无效金额 `{raw}`（--{key}），示例：12.5 或 0.01"),
    )
}

/// 分 → 两位小数字符串（输出与 Excel 用）。
pub fn cents_to_yuan(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let abs = cents.unsigned_abs();
    format!("{sign}{}.{:02}", abs / 100, abs % 100)
}

/// 本地 HH:MM:SS（记账时间缺省值；拿不到时区就退 UTC）。
fn local_time_hms() -> String {
    match crate::time::local_timezone() {
        Some(tz) => crate::time::now_in_tz(tz).format("%H:%M:%S").to_string(),
        None => now_iso()[11..19].to_string(),
    }
}

fn find_account<'a>(file: &'a LedgerFile, id: &str) -> Option<&'a LedgerAccount> {
    file.accounts.iter().find(|item| item.id == id)
}

fn find_category<'a>(file: &'a LedgerFile, id: &str) -> Option<&'a LedgerCategory> {
    file.categories.iter().find(|item| item.id == id)
}

fn not_found(id: &str) -> CoreError {
    CoreError::validation(
        "LEDGER_ENTRY_NOT_FOUND",
        format!("记账条目 `{id}` 不存在"),
    )
    .with_hint("先运行 kxtodo-cli ledger list 查看现有条目")
}

fn account_not_found(name: &str) -> CoreError {
    CoreError::validation(
        "LEDGER_ACCOUNT_NOT_FOUND",
        format!("资金账户 `{name}` 不存在"),
    )
    .with_hint("先运行 kxtodo-cli ledger accounts 查看现有账户")
}

/// 按名字解析账户（id 优先，名字兜底——CLI 与 Excel 导入都传名字）。
fn resolve_account<'a>(file: &'a LedgerFile, raw: &str) -> CoreResult<&'a LedgerAccount> {
    let trimmed = raw.trim();
    find_account(file, trimmed)
        .or_else(|| file.accounts.iter().find(|item| item.name == trimmed))
        .ok_or_else(|| account_not_found(trimmed))
}

fn resolve_category<'a>(
    file: &'a LedgerFile,
    raw: &str,
    side: LedgerSide,
) -> CoreResult<&'a LedgerCategory> {
    let trimmed = raw.trim();
    find_category(file, trimmed)
        .or_else(|| {
            file.categories
                .iter()
                .find(|item| item.name == trimmed && item.side == side)
        })
        .ok_or_else(|| {
            CoreError::validation(
                "LEDGER_CATEGORY_NOT_FOUND",
                format!("{}分类 `{trimmed}` 不存在", side_label(side)),
            )
            .with_hint("先运行 kxtodo-cli ledger categories 查看现有分类")
        })
}

fn side_label(side: LedgerSide) -> &'static str {
    match side {
        LedgerSide::Expense => "支出",
        LedgerSide::Income => "收入",
    }
}

fn kind_side(kind: LedgerKind) -> Option<LedgerSide> {
    match kind {
        LedgerKind::Expense => Some(LedgerSide::Expense),
        LedgerKind::Income => Some(LedgerSide::Income),
        LedgerKind::Transfer => None,
    }
}

/// 条目输出：id 之外把账户/分类名字一起带上，CLI 与 Agent 读起来不用二次查表。
fn entry_view(file: &LedgerFile, entry: &LedgerEntry) -> Value {
    let account = find_account(file, &entry.account_id);
    let to_account = entry
        .to_account_id
        .as_deref()
        .and_then(|id| find_account(file, id));
    let category = entry
        .category_id
        .as_deref()
        .and_then(|id| find_category(file, id));
    let parent = category
        .as_ref()
        .and_then(|item| item.parent_id.as_deref())
        .and_then(|id| find_category(file, id));
    let signed = match entry.kind {
        LedgerKind::Income => entry.amount_cents,
        _ => -entry.amount_cents,
    };
    json!({
        "id": entry.id,
        "kind": entry.kind.as_str(),
        "amountCents": entry.amount_cents,
        "amount": cents_to_yuan(entry.amount_cents),
        "signedCents": signed,
        "signed": cents_to_yuan(signed),
        "accountId": entry.account_id,
        "accountName": account.map(|item| item.name.clone()).unwrap_or_default(),
        "toAccountId": entry.to_account_id,
        "toAccountName": to_account.map(|item| item.name.clone()).unwrap_or_default(),
        "categoryId": entry.category_id,
        "categoryName": category.map(|item| item.name.clone()).unwrap_or_default(),
        "categoryParentName": parent.map(|item| item.name.clone()).unwrap_or_default(),
        "date": entry.date,
        "time": entry.time,
        "note": entry.note,
        "images": entry.images,
        "createdAt": entry.created_at,
        "updatedAt": entry.updated_at,
    })
}

fn account_view(file: &LedgerFile, account: &LedgerAccount) -> Value {
    json!({
        "id": account.id,
        "name": account.name,
        "kind": account.kind.as_str(),
        "icon": account.icon,
        "color": account.color,
        "initialCents": account.initial_cents,
        "initial": cents_to_yuan(account.initial_cents),
        "balanceCents": account_balance_cents(file, &account.id),
        "balance": cents_to_yuan(account_balance_cents(file, &account.id)),
        "note": account.note,
        "order": account.order,
    })
}

fn category_view(category: &LedgerCategory) -> Value {
    json!({
        "id": category.id,
        "name": category.name,
        "side": category.side.as_str(),
        // 层级写在输出里（0 = 大类，1 = 子分类）：列表是**平铺**的，
        // Agent 不必自己按 parentId 聚合才知道谁是父谁是子
        "depth": u8::from(category.parent_id.is_some()),
        "parentId": category.parent_id,
        "icon": category.icon,
        "color": category.color,
        "order": category.order,
    })
}

/// 日期闭区间过滤（from/to 均可缺省）。
fn in_range(date: &str, from: Option<&str>, to: Option<&str>) -> bool {
    if let Some(from) = from {
        if date < from {
            return false;
        }
    }
    if let Some(to) = to {
        if date > to {
            return false;
        }
    }
    true
}

/// 列表顺序：日期新→旧，同一天时间晚→早，再比创建时间。
fn sort_entries(entries: &mut [LedgerEntry]) {
    entries.sort_by(|a, b| {
        b.date
            .cmp(&a.date)
            .then(b.time.cmp(&a.time))
            .then(b.created_at.cmp(&a.created_at))
            .then(a.id.cmp(&b.id))
    });
}

/// 账户当前余额 = 期初 + 收入入账 − 支出出账 − 转账转出 + 转账转入。
fn account_balance_cents(file: &LedgerFile, account_id: &str) -> i64 {
    let mut cents = find_account(file, account_id)
        .map(|item| item.initial_cents)
        .unwrap_or(0);
    for entry in &file.entries {
        match entry.kind {
            LedgerKind::Income => {
                if entry.account_id == account_id {
                    cents += entry.amount_cents;
                }
            }
            LedgerKind::Expense => {
                if entry.account_id == account_id {
                    cents -= entry.amount_cents;
                }
            }
            LedgerKind::Transfer => {
                if entry.account_id == account_id {
                    cents -= entry.amount_cents;
                }
                if entry.to_account_id.as_deref() == Some(account_id) {
                    cents += entry.amount_cents;
                }
            }
        }
    }
    cents
}

/// 校验并组装一条新/改后的条目字段（add 与 modify 共用）。
struct EntryDraft {
    kind: LedgerKind,
    amount_cents: i64,
    account_id: String,
    to_account_id: Option<String>,
    category_id: Option<String>,
    date: String,
    time: String,
    note: String,
    images: Vec<String>,
}

fn validate_entry(file: &LedgerFile, draft: &EntryDraft) -> CoreResult<()> {
    if draft.amount_cents <= 0 {
        return Err(CoreError::validation(
            "LEDGER_AMOUNT_INVALID",
            "金额必须大于 0",
        ));
    }
    if find_account(file, &draft.account_id).is_none() {
        return Err(account_not_found(&draft.account_id));
    }
    if let Some(to) = &draft.to_account_id {
        if draft.kind != LedgerKind::Transfer {
            return Err(CoreError::validation(
                "LEDGER_TRANSFER_FIELD_MISUSE",
                "只有转账才需要 --to 账户",
            ));
        }
        if to == &draft.account_id {
            return Err(CoreError::validation(
                "LEDGER_TRANSFER_SAME_ACCOUNT",
                "转账的转出与转入不能是同一个账户",
            ));
        }
        if find_account(file, to).is_none() {
            return Err(account_not_found(to));
        }
    } else if draft.kind == LedgerKind::Transfer {
        return Err(CoreError::validation(
            "LEDGER_TRANSFER_MISSING_TO",
            "转账需要 --to 指定转入账户",
        ));
    }
    if let Some(category_id) = &draft.category_id {
        let category = find_category(file, category_id).ok_or_else(|| {
            CoreError::validation("LEDGER_CATEGORY_NOT_FOUND", "分类不存在")
        })?;
        if let Some(side) = kind_side(draft.kind) {
            if category.side != side {
                return Err(CoreError::validation(
                    "LEDGER_CATEGORY_SIDE_MISMATCH",
                    format!("{}不能选{}分类", side_label(side), side_label(category.side)),
                ));
            }
        }
    }
    Ok(())
}

fn draft_from_params(file: &LedgerFile, params: &Value, base: Option<&LedgerEntry>) -> CoreResult<EntryDraft> {
    let kind = match param_str(params, "kind") {
        Some(raw) => LedgerKind::parse(&raw).ok_or_else(|| {
            CoreError::validation(
                "LEDGER_KIND_INVALID",
                format!("无效类型 `{raw}`，支持 expense|income|transfer"),
            )
        })?,
        None => base.map(|item| item.kind).unwrap_or_default(),
    };
    let amount_cents = match cents_param(params, "amountCents")? {
        Some(value) => value,
        None => match cents_param(params, "amount")? {
            Some(value) => value,
            None => base
                .map(|item| item.amount_cents)
                .ok_or_else(|| invalid_amount("amount", ""))?,
        },
    };
    let account_id = match param_str(params, "accountId") {
        Some(raw) => resolve_account(file, &raw)?.id.clone(),
        None => match param_str(params, "account") {
            Some(raw) => resolve_account(file, &raw)?.id.clone(),
            None => base
                .map(|item| item.account_id.clone())
                .ok_or_else(|| CoreError::validation("MISSING_PARAM", "缺少必填参数 --account"))?,
        },
    };
    let to_account_id = if params.get("toAccountId").is_some() || params.get("to").is_some() {
        let raw = param_str(params, "toAccountId").or_else(|| param_str(params, "to"));
        match raw {
            Some(text) if !text.trim().is_empty() => Some(resolve_account(file, &text)?.id.clone()),
            _ => None,
        }
    } else {
        base.and_then(|item| item.to_account_id.clone())
    };
    let category_id = if params.get("categoryId").is_some() || params.get("category").is_some() {
        let raw = param_str(params, "categoryId").or_else(|| param_str(params, "category"));
        match raw {
            Some(text) if !text.trim().is_empty() => {
                let side = kind_side(kind).unwrap_or(LedgerSide::Expense);
                Some(resolve_category(file, &text, side)?.id.clone())
            }
            _ => None,
        }
    } else {
        base.and_then(|item| item.category_id.clone())
    };
    let date = match param_str(params, "date") {
        Some(raw) => parse_date(&raw)?,
        None => base
            .map(|item| item.date.clone())
            .unwrap_or_else(today_local),
    };
    let time = match param_str(params, "time") {
        Some(raw) => {
            let trimmed = raw.trim().to_string();
            if trimmed.is_empty() {
                String::new()
            } else if trimmed.len() == 5 && trimmed.contains(':') {
                format!("{trimmed}:00")
            } else {
                trimmed
            }
        }
        None => base
            .map(|item| item.time.clone())
            .unwrap_or_else(local_time_hms),
    };
    let note = param_str(params, "note")
        .map(|text| text.trim().to_string())
        .unwrap_or_else(|| base.map(|item| item.note.clone()).unwrap_or_default());
    let images = match params.get("images") {
        Some(Value::Array(list)) => {
            let mut names: Vec<String> = Vec::new();
            for item in list {
                let Some(raw) = item.as_str() else {
                    return Err(CoreError::validation(
                        "LEDGER_IMAGE_NAME_INVALID",
                        "images 必须是文件名字符串数组",
                    ));
                };
                let name = raw.trim().to_string();
                if name.is_empty() {
                    continue; // 空串跳过（modify 传空数组 = 清除全部附图）
                }
                if !is_safe_image_name(&name) {
                    return Err(CoreError::validation(
                        "LEDGER_IMAGE_NAME_INVALID",
                        format!("无效附图文件名 `{name}`"),
                    ));
                }
                if !names.contains(&name) {
                    names.push(name);
                }
            }
            names
        }
        Some(other) if !other.is_null() => {
            return Err(CoreError::validation(
                "LEDGER_IMAGE_NAME_INVALID",
                "images 必须是文件名字符串数组",
            ));
        }
        _ => base.map(|item| item.images.clone()).unwrap_or_default(),
    };
    Ok(EntryDraft {
        kind,
        amount_cents,
        account_id,
        to_account_id,
        category_id,
        date,
        time,
        note,
        images,
    })
}

// ---------------------------------------------------------------------------
// entries
// ---------------------------------------------------------------------------

/// 记账侧保存后的附图清理（v0.7.4）：对着**写入后**的账本扫 `img/data/ledger/`，删掉
/// 没有任何一笔账再引用的图片（只跟本地写，见 `image_gc`）。引用集合 = 各条目 `images`
/// 列表的并集（裸文件名），不是 markdown。清理失败一律吞掉——绝不让保存本身因为清理而失败。
fn sweep_ledger_images(ctx: &ExecContext, file: &LedgerFile) {
    let dir = ctx.repo.layout.entry_img_dir(LEDGER_IMAGE_NODE);
    let referenced: HashSet<String> = file
        .entries
        .iter()
        .flat_map(|entry| entry.images.iter().cloned())
        .collect();
    crate::image_gc::sweep_unreferenced_by_names(&dir, &referenced);
}

fn ledger_add(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let params = &inv.params;
    let file = ctx.repo.load_ledger()?;
    let draft = draft_from_params(&file, params, None)?;
    validate_entry(&file, &draft)?;
    let now = now_iso();
    let entry_id = gen_id("ledger");
    let view = {
        let draft = draft;
        let (file, outcome) = ctx.repo.write_ledger(
            inv.controls.if_revision,
            inv.controls.idempotency_key.as_deref(),
            &inv.command,
            |file| {
                let entry = LedgerEntry {
                    id: entry_id.clone(),
                    kind: draft.kind,
                    amount_cents: draft.amount_cents,
                    account_id: draft.account_id.clone(),
                    to_account_id: draft.to_account_id.clone(),
                    category_id: draft.category_id.clone(),
                    date: draft.date.clone(),
                    time: draft.time.clone(),
                    note: draft.note.clone(),
                    images: draft.images.clone(),
                    legacy_image: None,
                    created_at: now.clone(),
                    updated_at: None,
                    extra: Map::new(),
                };
                file.entries.push(entry.clone());
                sort_entries(&mut file.entries);
                Ok(idem_summary(&entry_view(file, &entry)))
            },
        )?;
        apply_write_outcome(meta, Domain::Ledger, &outcome);
        notify_host(ctx, Domain::Ledger, outcome.revision, vec![]);
        sweep_ledger_images(ctx, &file);
        if outcome.replayed {
            return Ok(outcome.replay_summary.clone().unwrap_or(Value::Null));
        }
        let entry = file
            .entries
            .iter()
            .find(|item| item.id == entry_id)
            .cloned()
            .ok_or_else(|| not_found(&entry_id))?;
        entry_view(&file, &entry)
    };
    Ok(view)
}

fn ledger_transfer(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let mut params = inv.params.clone();
    params["kind"] = json!("transfer");
    let wrapped = Invocation {
        command: inv.command.clone(),
        params,
        controls: inv.controls.clone(),
    };
    ledger_add(&wrapped, ctx, meta)
}

fn ledger_get(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let id = required_str(&inv.params, "id")?;
    let file = ctx.repo.load_ledger()?;
    set_read_revision(meta, Domain::Ledger, file.meta.revision);
    let entry = file.entries.iter().find(|item| item.id == id).ok_or_else(|| not_found(&id))?;
    Ok(entry_view(&file, entry))
}

fn ledger_list(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let params = &inv.params;
    let file = ctx.repo.load_ledger()?;
    set_read_revision(meta, Domain::Ledger, file.meta.revision);
    let date = param_str(params, "date");
    let from = param_str(params, "from");
    let to = param_str(params, "to");
    let kind = param_str(params, "kind").and_then(|raw| LedgerKind::parse(&raw));
    let account = param_str(params, "accountId").or_else(|| param_str(params, "account"));
    let account_id = match account {
        Some(raw) => Some(resolve_account(&file, &raw)?.id.clone()),
        None => None,
    };
    let category = param_str(params, "categoryId").or_else(|| param_str(params, "category"));
    let category_id = match category {
        Some(raw) => {
            let side = kind.and_then(kind_side).unwrap_or(LedgerSide::Expense);
            Some(resolve_category(&file, &raw, side)?.id.clone())
        }
        None => None,
    };
    let page = unbounded_page_from(params)?;

    let mut entries: Vec<LedgerEntry> = file
        .entries
        .iter()
        .filter(|entry| {
            if let Some(date) = &date {
                if &entry.date != date {
                    return false;
                }
            }
            if !in_range(&entry.date, from.as_deref(), to.as_deref()) {
                return false;
            }
            if let Some(kind) = kind {
                if entry.kind != kind {
                    return false;
                }
            }
            if let Some(account_id) = &account_id {
                if entry.account_id != *account_id
                    && entry.to_account_id.as_deref() != Some(account_id.as_str())
                {
                    return false;
                }
            }
            if let Some(category_id) = &category_id {
                if entry.category_id.as_deref() != Some(category_id.as_str()) {
                    return false;
                }
            }
            true
        })
        .cloned()
        .collect();
    sort_entries(&mut entries);
    // data.total 是「过滤后的全部」，与分页无关（不给 --limit 时它就等于 returned）
    let total = entries.len() as u64;
    let (entries, next_cursor, _) = paginate(entries, &page);
    // meta 与 data 双写同一份分页元信息：render 的人类可读输出（table/pretty）只认
    // meta.count，缺了它 ledger list 在这两种格式下就退化成逐行裸 JSON、没有「共 N 条」。
    meta.count = Some(total as usize);
    meta.next_cursor = next_cursor;
    let items: Vec<Value> = entries.iter().map(|entry| entry_view(&file, entry)).collect();
    Ok(json!({ "total": total, "returned": items.len(), "items": items }))
}

fn ledger_modify(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let params = &inv.params;
    let id = required_str(params, "id")?;
    let file = ctx.repo.load_ledger()?;
    let base = file
        .entries
        .iter()
        .find(|item| item.id == id)
        .ok_or_else(|| not_found(&id))?
        .clone();
    let draft = draft_from_params(&file, params, Some(&base))?;
    validate_entry(&file, &draft)?;
    let now = now_iso();
    let (file, outcome) = ctx.repo.write_ledger(
        inv.controls.if_revision,
        inv.controls.idempotency_key.as_deref(),
        &inv.command,
        |file| {
            let position = file
                .entries
                .iter()
                .position(|item| item.id == id)
                .ok_or_else(|| not_found(&id))?;
            {
                let entry = &mut file.entries[position];
                entry.kind = draft.kind;
                entry.amount_cents = draft.amount_cents;
                entry.account_id = draft.account_id.clone();
                entry.to_account_id = draft.to_account_id.clone();
                entry.category_id = draft.category_id.clone();
                entry.date = draft.date.clone();
                entry.time = draft.time.clone();
                entry.note = draft.note.clone();
                entry.images = draft.images.clone();
                entry.updated_at = Some(now.clone());
            }
            sort_entries(&mut file.entries);
            let entry = file
                .entries
                .iter()
                .find(|item| item.id == id)
                .ok_or_else(|| not_found(&id))?;
            Ok(idem_summary(&entry_view(file, entry)))
        },
    )?;
    apply_write_outcome(meta, Domain::Ledger, &outcome);
    notify_host(ctx, Domain::Ledger, outcome.revision, vec![id.clone()]);
    sweep_ledger_images(ctx, &file);
    if outcome.replayed {
        return Ok(outcome.replay_summary.clone().unwrap_or(Value::Null));
    }
    let entry = file
        .entries
        .iter()
        .find(|item| item.id == id)
        .ok_or_else(|| not_found(&id))?;
    Ok(entry_view(&file, entry))
}

fn ledger_remove(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let id = required_str(&inv.params, "id")?;
    let file = ctx.repo.load_ledger()?;
    let entry = file
        .entries
        .iter()
        .find(|item| item.id == id)
        .ok_or_else(|| not_found(&id))?;
    let plan = json!({
        "type": "ledger",
        "id": id,
        "date": entry.date,
        "amount": cents_to_yuan(entry.amount_cents),
        "kind": entry.kind.as_str(),
        "note": entry.note,
    });
    require_confirmation(
        &inv.controls,
        format!("将删除 {} 的一笔{}（{}）", entry.date, entry.kind.as_str(), cents_to_yuan(entry.amount_cents)),
        plan,
    )?;
    if inv.controls.dry_run {
        return Ok(json!({ "removed": 0, "dryRun": true }));
    }
    let now = now_iso();
    let (file, outcome) = ctx.repo.write_ledger(
        inv.controls.if_revision,
        inv.controls.idempotency_key.as_deref(),
        &inv.command,
        |file| {
            let before = file.entries.len();
            file.entries.retain(|item| item.id != id);
            // 删除必须显式传播：多端同步靠墓碑，而不是「这边少了就算删」
            file.meta.record_tombstone(&id, "ledger", &now);
            Ok(json!({ "removed": before - file.entries.len() }))
        },
    )?;
    apply_write_outcome(meta, Domain::Ledger, &outcome);
    notify_host(ctx, Domain::Ledger, outcome.revision, vec![id.clone()]);
    sweep_ledger_images(ctx, &file);
    Ok(json!({ "removed": 1, "revision": outcome.revision }))
}

// ---------------------------------------------------------------------------
// accounts
// ---------------------------------------------------------------------------

fn ledger_accounts(_inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let file = ctx.repo.load_ledger()?;
    set_read_revision(meta, Domain::Ledger, file.meta.revision);
    let mut accounts = file.accounts.clone();
    accounts.sort_by(|a, b| a.order.partial_cmp(&b.order).unwrap_or(std::cmp::Ordering::Equal));
    let items: Vec<Value> = accounts.iter().map(|item| account_view(&file, item)).collect();
    let net: i64 = accounts
        .iter()
        .map(|item| account_balance_cents(&file, &item.id))
        .sum();
    Ok(json!({ "total": items.len(), "items": items, "netCents": net, "net": cents_to_yuan(net) }))
}

fn ledger_account_add(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let params = &inv.params;
    let name = required_str(params, "name")?;
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(CoreError::validation("LEDGER_ACCOUNT_NAME_EMPTY", "账户名不能为空"));
    }
    let kind = match param_str(params, "kind") {
        Some(raw) => {
            let text = raw.trim().to_string();
            if text.is_empty() {
                return Err(CoreError::validation(
                    "LEDGER_ACCOUNT_KIND_INVALID",
                    "账户类型不能为空",
                ));
            }
            text
        }
        None => default_account_kind(),
    };
    let initial = cents_param(params, "initial")?.unwrap_or(0);
    let icon = param_str(params, "icon").unwrap_or_default();
    let color = param_str(params, "color").unwrap_or_default();
    let note = param_str(params, "note").unwrap_or_default();
    let now = now_iso();
    let account_id = gen_id("lacc");
    let (file, outcome) = ctx.repo.write_ledger(
        inv.controls.if_revision,
        inv.controls.idempotency_key.as_deref(),
        &inv.command,
        |file| {
            if file.accounts.iter().any(|item| item.name == name) {
                return Err(CoreError::validation(
                    "LEDGER_ACCOUNT_EXISTS",
                    format!("账户 `{name}` 已存在"),
                ));
            }
            let order = file
                .accounts
                .iter()
                .map(|item| item.order)
                .fold(0f64, f64::max)
                + 1.0;
            let account = LedgerAccount {
                id: account_id.clone(),
                name: name.clone(),
                icon: icon.trim().to_string(),
                color: color.trim().to_string(),
                kind,
                initial_cents: initial,
                note: note.trim().to_string(),
                order,
                created_at: now.clone(),
                updated_at: None,
                extra: Map::new(),
            };
            file.accounts.push(account.clone());
            Ok(idem_summary(&account_view(file, &account)))
        },
    )?;
    apply_write_outcome(meta, Domain::Ledger, &outcome);
    notify_host(ctx, Domain::Ledger, outcome.revision, vec![]);
    if outcome.replayed {
        return Ok(outcome.replay_summary.clone().unwrap_or(Value::Null));
    }
    let account = find_account(&file, &account_id)
        .ok_or_else(|| account_not_found(&name))?
        .clone();
    Ok(account_view(&file, &account))
}

fn ledger_account_modify(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let params = &inv.params;
    let id = required_str(params, "id")?;
    let now = now_iso();
    let (file, outcome) = ctx.repo.write_ledger(
        inv.controls.if_revision,
        inv.controls.idempotency_key.as_deref(),
        &inv.command,
        |file| {
            if file.accounts.iter().all(|item| item.id != id) {
                return Err(account_not_found(&id));
            }
            if let Some(raw) = param_str(params, "name") {
                let name = raw.trim().to_string();
                if name.is_empty() {
                    return Err(CoreError::validation("LEDGER_ACCOUNT_NAME_EMPTY", "账户名不能为空"));
                }
                if file.accounts.iter().any(|item| item.name == name && item.id != id) {
                    return Err(CoreError::validation(
                        "LEDGER_ACCOUNT_EXISTS",
                        format!("账户 `{name}` 已存在"),
                    ));
                }
            }
            let kind = match param_str(params, "kind") {
                Some(raw) => {
                    let text = raw.trim().to_string();
                    if text.is_empty() {
                        return Err(CoreError::validation(
                            "LEDGER_ACCOUNT_KIND_INVALID",
                            "账户类型不能为空",
                        ));
                    }
                    Some(text)
                }
                None => None,
            };
            let initial = cents_param(params, "initial")?;
            let balance = cents_param(params, "balance")?;
            if initial.is_some() && balance.is_some() {
                return Err(CoreError::validation(
                    "LEDGER_PARAM_CONFLICT",
                    "--initial（期初余额）与 --balance（当前余额）只能给一个",
                ));
            }
            let position = file
                .accounts
                .iter()
                .position(|item| item.id == id)
                .ok_or_else(|| account_not_found(&id))?;
            // 直设当前余额：期初 = 目标余额 − 流水推导和（口径与 ledger balance 完全一致），
            // 于是推导出来的余额恰好等于用户要的数。先算好再进可变借用。
            let balance_initial = match balance {
                Some(value) => {
                    let derived_flows = account_balance_cents(file, &id)
                        .saturating_sub(file.accounts[position].initial_cents);
                    Some(value.saturating_sub(derived_flows))
                }
                None => None,
            };
            {
                let account = &mut file.accounts[position];
                if let Some(raw) = param_str(params, "name") {
                    account.name = raw.trim().to_string();
                }
                if let Some(kind) = kind {
                    account.kind = kind;
                }
                if let Some(raw) = param_str(params, "icon") {
                    account.icon = raw.trim().to_string();
                }
                if let Some(raw) = param_str(params, "color") {
                    account.color = raw.trim().to_string();
                }
                if let Some(raw) = param_str(params, "note") {
                    account.note = raw.trim().to_string();
                }
                if let Some(value) = initial {
                    account.initial_cents = value;
                }
                if let Some(value) = balance_initial {
                    account.initial_cents = value;
                }
                if let Some(value) = params.get("order").and_then(Value::as_f64) {
                    account.order = value;
                }
                account.updated_at = Some(now.clone());
            }
            let account = &file.accounts[position];
            Ok(idem_summary(&account_view(file, account)))
        },
    )?;
    apply_write_outcome(meta, Domain::Ledger, &outcome);
    notify_host(ctx, Domain::Ledger, outcome.revision, vec![id.clone()]);
    if outcome.replayed {
        return Ok(outcome.replay_summary.clone().unwrap_or(Value::Null));
    }
    let account = find_account(&file, &id)
        .ok_or_else(|| account_not_found(&id))?
        .clone();
    Ok(account_view(&file, &account))
}

fn ledger_account_remove(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let id = required_str(&inv.params, "id")?;
    let file = ctx.repo.load_ledger()?;
    let account = find_account(&file, &id)
        .ok_or_else(|| account_not_found(&id))?
        .clone();
    let used = file
        .entries
        .iter()
        .filter(|entry| {
            entry.account_id == id || entry.to_account_id.as_deref() == Some(id.as_str())
        })
        .count();
    if used > 0 {
        return Err(CoreError::validation(
            "LEDGER_ACCOUNT_IN_USE",
            format!("账户「{}」名下还有 {used} 笔账，先删除或改到别的账户再删账户", account.name),
        ));
    }
    require_confirmation(
        &inv.controls,
        format!("将删除资金账户「{}」", account.name),
        json!({ "type": "ledgerAccount", "id": id, "name": account.name }),
    )?;
    if inv.controls.dry_run {
        return Ok(json!({ "removed": 0, "dryRun": true }));
    }
    let now = now_iso();
    let (_, outcome) = ctx.repo.write_ledger(
        inv.controls.if_revision,
        inv.controls.idempotency_key.as_deref(),
        &inv.command,
        |file| {
            file.accounts.retain(|item| item.id != id);
            file.meta.record_tombstone(&id, "ledgerAccount", &now);
            Ok(json!({ "removed": 1 }))
        },
    )?;
    apply_write_outcome(meta, Domain::Ledger, &outcome);
    notify_host(ctx, Domain::Ledger, outcome.revision, vec![id.clone()]);
    Ok(json!({ "removed": 1, "revision": outcome.revision }))
}

// ---------------------------------------------------------------------------
// account types（自定义账户类型，v0.7.5）
// ---------------------------------------------------------------------------

fn find_account_type<'a>(file: &'a LedgerFile, id: &str) -> Option<&'a LedgerAccountType> {
    file.account_types.iter().find(|item| item.id == id)
}

fn account_type_not_found(name: &str) -> CoreError {
    CoreError::validation(
        "LEDGER_ACCOUNT_TYPE_NOT_FOUND",
        format!("账户类型 `{name}` 不存在"),
    )
    .with_hint("先运行 kxtodo-cli ledger account-types 查看现有类型")
}

fn account_type_view(item: &LedgerAccountType) -> Value {
    json!({
        "id": item.id,
        "name": item.name,
        "icon": item.icon,
        "color": item.color,
        "createdAt": item.created_at,
        "updatedAt": item.updated_at,
    })
}

fn ledger_account_types(_inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let file = ctx.repo.load_ledger()?;
    set_read_revision(meta, Domain::Ledger, file.meta.revision);
    let items: Vec<Value> = file.account_types.iter().map(account_type_view).collect();
    Ok(json!({ "total": items.len(), "items": items }))
}

fn ledger_account_type_add(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let params = &inv.params;
    let name = required_str(params, "name")?;
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(CoreError::validation(
            "LEDGER_ACCOUNT_TYPE_NAME_EMPTY",
            "账户类型名不能为空",
        ));
    }
    let icon = param_str(params, "icon").unwrap_or_default();
    let color = param_str(params, "color").unwrap_or_default();
    let now = now_iso();
    let type_id = gen_id("latype");
    let (file, outcome) = ctx.repo.write_ledger(
        inv.controls.if_revision,
        inv.controls.idempotency_key.as_deref(),
        &inv.command,
        |file| {
            if file.account_types.iter().any(|item| item.name == name) {
                return Err(CoreError::validation(
                    "LEDGER_ACCOUNT_TYPE_EXISTS",
                    format!("账户类型 `{name}` 已存在"),
                ));
            }
            let item = LedgerAccountType {
                id: type_id.clone(),
                name: name.clone(),
                icon: icon.trim().to_string(),
                color: color.trim().to_string(),
                created_at: now.clone(),
                updated_at: None,
                extra: Map::new(),
            };
            file.account_types.push(item.clone());
            Ok(idem_summary(&account_type_view(&item)))
        },
    )?;
    apply_write_outcome(meta, Domain::Ledger, &outcome);
    notify_host(ctx, Domain::Ledger, outcome.revision, vec![]);
    if outcome.replayed {
        return Ok(outcome.replay_summary.clone().unwrap_or(Value::Null));
    }
    let item = find_account_type(&file, &type_id)
        .ok_or_else(|| account_type_not_found(&name))?
        .clone();
    Ok(account_type_view(&item))
}

fn ledger_account_type_modify(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let params = &inv.params;
    let id = required_str(params, "id")?;
    let now = now_iso();
    let (file, outcome) = ctx.repo.write_ledger(
        inv.controls.if_revision,
        inv.controls.idempotency_key.as_deref(),
        &inv.command,
        |file| {
            if find_account_type(file, &id).is_none() {
                return Err(account_type_not_found(&id));
            }
            if let Some(raw) = param_str(params, "name") {
                let name = raw.trim().to_string();
                if name.is_empty() {
                    return Err(CoreError::validation(
                        "LEDGER_ACCOUNT_TYPE_NAME_EMPTY",
                        "账户类型名不能为空",
                    ));
                }
                if file
                    .account_types
                    .iter()
                    .any(|item| item.name == name && item.id != id)
                {
                    return Err(CoreError::validation(
                        "LEDGER_ACCOUNT_TYPE_EXISTS",
                        format!("账户类型 `{name}` 已存在"),
                    ));
                }
            }
            let item = file
                .account_types
                .iter_mut()
                .find(|item| item.id == id)
                .ok_or_else(|| account_type_not_found(&id))?;
            if let Some(raw) = param_str(params, "name") {
                item.name = raw.trim().to_string();
            }
            if let Some(raw) = param_str(params, "icon") {
                item.icon = raw.trim().to_string();
            }
            if let Some(raw) = param_str(params, "color") {
                item.color = raw.trim().to_string();
            }
            item.updated_at = Some(now.clone());
            Ok(idem_summary(&account_type_view(item)))
        },
    )?;
    apply_write_outcome(meta, Domain::Ledger, &outcome);
    notify_host(ctx, Domain::Ledger, outcome.revision, vec![id.clone()]);
    if outcome.replayed {
        return Ok(outcome.replay_summary.clone().unwrap_or(Value::Null));
    }
    let item = find_account_type(&file, &id)
        .ok_or_else(|| account_type_not_found(&id))?
        .clone();
    Ok(account_type_view(&item))
}

fn ledger_account_type_remove(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let id = required_str(&inv.params, "id")?;
    // 名下有账户在用也允许删：账户的 kind 是自由字符串，类型删了字符串原样留着
    let (_, outcome) = ctx.repo.write_ledger(
        inv.controls.if_revision,
        inv.controls.idempotency_key.as_deref(),
        &inv.command,
        |file| {
            if find_account_type(file, &id).is_none() {
                return Err(account_type_not_found(&id));
            }
            file.account_types.retain(|item| item.id != id);
            file.meta.record_tombstone(&id, "ledgerAccountType", &now_iso());
            Ok(json!({ "removed": 1 }))
        },
    )?;
    apply_write_outcome(meta, Domain::Ledger, &outcome);
    notify_host(ctx, Domain::Ledger, outcome.revision, vec![id.clone()]);
    Ok(json!({ "removed": 1, "revision": outcome.revision }))
}

// ---------------------------------------------------------------------------
// categories
// ---------------------------------------------------------------------------

/// 图标目录（只读，不属于账本数据，也不设 domain revision）。
///
/// 给 CLI/Agent 一份「能选哪些图标」的权威清单：设计分类/账户时照着挑，而不是猜一个
/// lucide 里根本不存在的名字写进 --icon（前端画不出来，只会退化成省略号）。
/// `groups`/`icons` 是分类目录，`accountGroups`/`accountIcons` 是账户专用目录（v0.7.4）。
/// 目录本身是前端 `src/lib/ledgerIcons.ts` 的镜像，一致性由 tests/ledger_icons.rs 守着。
fn ledger_icon_list(_inv: &Invocation, _ctx: &ExecContext, _meta: &mut Meta) -> CoreResult<Value> {
    let groups: Vec<Value> = crate::ledger_icons::ICON_GROUPS
        .iter()
        .map(|group| json!({ "name": group.name, "icons": group.icons }))
        .collect();
    let icons = crate::ledger_icons::all_icons();
    let account_groups: Vec<Value> = crate::ledger_icons::ACCOUNT_ICON_GROUPS
        .iter()
        .map(|group| json!({ "name": group.name, "icons": group.icons }))
        .collect();
    let account_icons = crate::ledger_icons::all_account_icons();
    Ok(json!({
        "total": icons.len(),
        "groups": groups,
        "icons": icons,
        "accountTotal": account_icons.len(),
        "accountGroups": account_groups,
        "accountIcons": account_icons,
    }))
}

/// 分类列表：**平铺数组，但顺序是树的先序**（大类 → 紧跟它自己的子分类），
/// 同层按 `order`，先支出后收入。
///
/// v0.8.3 之前只按 `order` 排：子分类的 order 也从 1 起，于是「公交地铁」会插到它父亲
/// 「交通」前面，Agent 只能自己按 parentId 重新聚合——而 help 写的是「列出两级分类」。
/// 现在顺序本身就是层级，配合每项的 `depth` / `parentId` 不用再聚合。
fn ledger_categories(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let file = ctx.repo.load_ledger()?;
    set_read_revision(meta, Domain::Ledger, file.meta.revision);
    let side = param_str(&inv.params, "side").and_then(|raw| LedgerSide::parse(&raw));
    let mut categories: Vec<&LedgerCategory> = file
        .categories
        .iter()
        .filter(|item| side.map(|want| item.side == want).unwrap_or(true))
        .collect();
    categories.sort_by(|a, b| {
        a.side
            .as_str()
            .cmp(b.side.as_str())
            .then(a.order.partial_cmp(&b.order).unwrap_or(std::cmp::Ordering::Equal))
    });
    let mut items: Vec<Value> = Vec::with_capacity(categories.len());
    let mut listed = vec![false; categories.len()];
    for (index, parent) in categories.iter().enumerate() {
        if listed[index] || parent.parent_id.is_some() {
            continue;
        }
        listed[index] = true;
        items.push(category_view(parent));
        for (kid_index, kid) in categories.iter().enumerate() {
            if listed[kid_index] || kid.parent_id.as_deref() != Some(parent.id.as_str()) {
                continue;
            }
            listed[kid_index] = true;
            items.push(category_view(kid));
        }
    }
    // 父亲不在本次输出里的子分类（父被删 / 跨侧的异常数据）：照样列出来，条目一条都不能少
    for (index, item) in categories.iter().enumerate() {
        if !listed[index] {
            items.push(category_view(item));
        }
    }
    Ok(json!({ "total": items.len(), "items": items }))
}

fn ledger_category_add(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let params = &inv.params;
    let name = required_str(params, "name")?;
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(CoreError::validation("LEDGER_CATEGORY_NAME_EMPTY", "分类名不能为空"));
    }
    let side = match param_str(params, "side") {
        Some(raw) => LedgerSide::parse(&raw).ok_or_else(|| {
            CoreError::validation(
                "LEDGER_CATEGORY_SIDE_INVALID",
                format!("无效分类侧 `{raw}`，支持 expense|income"),
            )
        })?,
        None => LedgerSide::Expense,
    };
    let file = ctx.repo.load_ledger()?;
    let parent_id = match param_str(params, "parentId").or_else(|| param_str(params, "parent")) {
        Some(raw) if !raw.trim().is_empty() => {
            let parent = resolve_category(&file, &raw, side)?;
            if parent.parent_id.is_some() {
                return Err(CoreError::validation(
                    "LEDGER_CATEGORY_DEPTH",
                    "分类只有两级：父分类必须是大类",
                ));
            }
            Some(parent.id.clone())
        }
        _ => None,
    };
    let icon = param_str(params, "icon").unwrap_or_default();
    let color = param_str(params, "color").unwrap_or_default();
    let now = now_iso();
    let category_id = gen_id("lcat");
    let (file, outcome) = ctx.repo.write_ledger(
        inv.controls.if_revision,
        inv.controls.idempotency_key.as_deref(),
        &inv.command,
        |file| {
            let duplicated = file.categories.iter().any(|item| {
                item.name == name && item.side == side && item.parent_id == parent_id
            });
            if duplicated {
                return Err(CoreError::validation(
                    "LEDGER_CATEGORY_EXISTS",
                    format!("{}分类 `{name}` 已存在", side_label(side)),
                ));
            }
            let order = file
                .categories
                .iter()
                .filter(|item| item.side == side)
                .map(|item| item.order)
                .fold(0f64, f64::max)
                + 1.0;
            let category = LedgerCategory {
                id: category_id.clone(),
                name: name.clone(),
                side,
                parent_id: parent_id.clone(),
                icon: icon.trim().to_string(),
                color: color.trim().to_string(),
                order,
                created_at: now.clone(),
                updated_at: None,
                extra: Map::new(),
            };
            file.categories.push(category.clone());
            Ok(idem_summary(&category_view(&category)))
        },
    )?;
    apply_write_outcome(meta, Domain::Ledger, &outcome);
    notify_host(ctx, Domain::Ledger, outcome.revision, vec![]);
    if outcome.replayed {
        return Ok(outcome.replay_summary.clone().unwrap_or(Value::Null));
    }
    let category = find_category(&file, &category_id)
        .ok_or_else(|| CoreError::validation("LEDGER_CATEGORY_NOT_FOUND", "分类不存在"))?
        .clone();
    Ok(category_view(&category))
}

fn ledger_category_modify(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let params = &inv.params;
    let id = required_str(params, "id")?;
    let now = now_iso();
    let (file, outcome) = ctx.repo.write_ledger(
        inv.controls.if_revision,
        inv.controls.idempotency_key.as_deref(),
        &inv.command,
        |file| {
            let side = file
                .categories
                .iter()
                .find(|item| item.id == id)
                .map(|item| item.side)
                .ok_or_else(|| {
                    CoreError::validation("LEDGER_CATEGORY_NOT_FOUND", "分类不存在")
                })?;
            if let Some(raw) = param_str(params, "parentId") {
                let target = if raw.trim().is_empty() {
                    None
                } else {
                    let parent = resolve_category(file, &raw, side)?;
                    if parent.id == id {
                        return Err(CoreError::validation(
                            "LEDGER_CATEGORY_DEPTH",
                            "分类不能挂到自己名下",
                        ));
                    }
                    if parent.parent_id.is_some() {
                        return Err(CoreError::validation(
                            "LEDGER_CATEGORY_DEPTH",
                            "分类只有两级：父分类必须是大类",
                        ));
                    }
                    Some(parent.id.clone())
                };
                let category = file
                    .categories
                    .iter_mut()
                    .find(|item| item.id == id)
                    .ok_or_else(|| {
                        CoreError::validation("LEDGER_CATEGORY_NOT_FOUND", "分类不存在")
                    })?;
                category.parent_id = target;
            }
            let category = file
                .categories
                .iter_mut()
                .find(|item| item.id == id)
                .ok_or_else(|| {
                    CoreError::validation("LEDGER_CATEGORY_NOT_FOUND", "分类不存在")
                })?;
            if let Some(raw) = param_str(params, "name") {
                let name = raw.trim().to_string();
                if name.is_empty() {
                    return Err(CoreError::validation("LEDGER_CATEGORY_NAME_EMPTY", "分类名不能为空"));
                }
                category.name = name;
            }
            if let Some(raw) = param_str(params, "icon") {
                category.icon = raw.trim().to_string();
            }
            if let Some(raw) = param_str(params, "color") {
                category.color = raw.trim().to_string();
            }
            if let Some(value) = params.get("order").and_then(Value::as_f64) {
                category.order = value;
            }
            category.updated_at = Some(now.clone());
            Ok(idem_summary(&category_view(category)))
        },
    )?;
    apply_write_outcome(meta, Domain::Ledger, &outcome);
    notify_host(ctx, Domain::Ledger, outcome.revision, vec![id.clone()]);
    if outcome.replayed {
        return Ok(outcome.replay_summary.clone().unwrap_or(Value::Null));
    }
    let category = find_category(&file, &id)
        .ok_or_else(|| CoreError::validation("LEDGER_CATEGORY_NOT_FOUND", "分类不存在"))?
        .clone();
    Ok(category_view(&category))
}

fn ledger_category_remove(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let id = required_str(&inv.params, "id")?;
    let file = ctx.repo.load_ledger()?;
    let category = find_category(&file, &id)
        .ok_or_else(|| CoreError::validation("LEDGER_CATEGORY_NOT_FOUND", "分类不存在"))?
        .clone();
    // 删大类连带子分类一起删；名下条目不删，只是变成「未分类」
    let mut ids: Vec<String> = vec![id.clone()];
    if category.parent_id.is_none() {
        ids.extend(
            file.categories
                .iter()
                .filter(|item| item.parent_id.as_deref() == Some(id.as_str()))
                .map(|item| item.id.clone()),
        );
    }
    let touched = file
        .entries
        .iter()
        .filter(|entry| entry.category_id.as_deref().is_some_and(|cid| ids.contains(&cid.to_string())))
        .count();
    require_confirmation(
        &inv.controls,
        format!(
            "将删除{}分类「{}」{}，名下 {touched} 笔账会变为未分类",
            side_label(category.side),
            category.name,
            if ids.len() > 1 { format!("及其 {} 个子分类", ids.len() - 1) } else { String::new() }
        ),
        json!({ "type": "ledgerCategory", "ids": ids, "name": category.name }),
    )?;
    if inv.controls.dry_run {
        return Ok(json!({ "removed": 0, "dryRun": true }));
    }
    let now = now_iso();
    let removed = ids.len();
    let (_, outcome) = ctx.repo.write_ledger(
        inv.controls.if_revision,
        inv.controls.idempotency_key.as_deref(),
        &inv.command,
        |file| {
            file.categories.retain(|item| !ids.contains(&item.id));
            for entry in file.entries.iter_mut() {
                if entry.category_id.as_deref().is_some_and(|cid| ids.contains(&cid.to_string())) {
                    entry.category_id = None;
                }
            }
            for gone in &ids {
                file.meta.record_tombstone(gone, "ledgerCategory", &now);
            }
            Ok(json!({ "removed": removed }))
        },
    )?;
    apply_write_outcome(meta, Domain::Ledger, &outcome);
    notify_host(ctx, Domain::Ledger, outcome.revision, ids);
    Ok(json!({ "removed": removed, "revision": outcome.revision }))
}

// ---------------------------------------------------------------------------
// stats & balance
// ---------------------------------------------------------------------------

struct StatsRange {
    from: String,
    to: String,
    /// "day" = 序列按天，"month" = 序列按月
    grain: &'static str,
    label: String,
}

fn stats_range(params: &Value) -> CoreResult<StatsRange> {
    if let Some(raw) = param_str(params, "month") {
        let text = raw.trim().to_string();
        let (year, month) = text
            .split_once('-')
            .map(|(y, m)| (y.parse::<i32>().ok(), m.parse::<u32>().ok()))
            .unwrap_or((None, None));
        let (Some(year), Some(month)) = (year, month) else {
            return Err(CoreError::validation(
                "LEDGER_RANGE_INVALID",
                format!("无效月份 `{raw}`，示例：2026-09"),
            ));
        };
        if !(1..=12).contains(&month) {
            return Err(CoreError::validation(
                "LEDGER_RANGE_INVALID",
                format!("无效月份 `{raw}`"),
            ));
        }
        let last = last_day_of_month(year, month);
        Ok(StatsRange {
            from: format!("{year:04}-{month:02}-01"),
            to: format!("{year:04}-{month:02}-{last:02}"),
            grain: "day",
            label: text,
        })
    } else if let Some(raw) = param_str(params, "year") {
        let year: i32 = raw.trim().parse().map_err(|_| {
            CoreError::validation("LEDGER_RANGE_INVALID", format!("无效年份 `{raw}`"))
        })?;
        Ok(StatsRange {
            from: format!("{year:04}-01-01"),
            to: format!("{year:04}-12-31"),
            grain: "month",
            label: format!("{year}"),
        })
    } else {
        let from = param_str(params, "from");
        let to = param_str(params, "to");
        let from = match from {
            Some(raw) => parse_date(&raw)?,
            None => OPEN_FROM.to_string(),
        };
        let to = match to {
            Some(raw) => parse_date(&raw)?,
            None => OPEN_TO.to_string(),
        };
        // 跨度不超过两个月（含两端 ≤62 天）就按天，否则按月。
        // 早先的判据是「from 与 to 落在同一个自然月」，于是 09-28 ~ 10-03 这种
        // 跨月的周报区间会被摊成两个月桶，调用方拿不到自己指定范围内的日粒度。
        let grain = if span_days(&from, &to).is_some_and(|days| days <= DAY_GRAIN_MAX_DAYS) {
            "day"
        } else {
            "month"
        };
        let label = format!("{from} ~ {to}");
        Ok(StatsRange {
            from,
            to,
            grain,
            label,
        })
    }
}

fn last_day_of_month(year: i32, month: u32) -> u32 {
    let days = [31u32, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if month == 2 && (year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)) {
        29
    } else {
        days[(month - 1) as usize]
    }
}

/// 不给 --from / --to 时的哨兵边界（「全量范围」）。
const OPEN_FROM: &str = "0000-01-01";
const OPEN_TO: &str = "9999-12-31";
/// 日粒度的跨度上限（含两端）。**必须与前端 `src/lib/ledger.ts::bucketOf` 同一个数**：
/// 两边各自判一次，门槛不同的话同一段区间 GUI 画日桶、CLI 给月桶（v0.8.0 之前
/// 前端是 62、这里是「同一个自然月」，32~62 天的自定义区间两边结论相反）。
const DAY_GRAIN_MAX_DAYS: i64 = 62;
/// 序列横轴的硬顶：day 粒度只在 ≤62 天的区间上生效、month 粒度也轮不到一千个月，
/// 这个上限纯属防御，别让一个畸形区间生成上百万个空档。
const MAX_SERIES_KEYS: usize = 1200;

/// 闭区间 `[from, to]` 的天数（含两端）；任一端不是合法日期就 None。
fn span_days(from: &str, to: &str) -> Option<i64> {
    let start = NaiveDate::parse_from_str(from, "%Y-%m-%d").ok()?;
    let end = NaiveDate::parse_from_str(to, "%Y-%m-%d").ok()?;
    (end - start).num_days().checked_add(1)
}

/// 逐日枚举 `[from, to]`（含两端）。跨月、跨年、闰月都由 chrono 兜住；
/// `to` 早于 `from` 时返回空表（倒挂的区间里没有任何一天）。
fn enumerate_days(from: &str, to: &str) -> Vec<String> {
    let (Ok(start), Ok(end)) = (
        NaiveDate::parse_from_str(from, "%Y-%m-%d"),
        NaiveDate::parse_from_str(to, "%Y-%m-%d"),
    ) else {
        return Vec::new();
    };
    let mut keys = Vec::new();
    let mut cursor = start;
    while cursor <= end && keys.len() < MAX_SERIES_KEYS {
        keys.push(cursor.format("%Y-%m-%d").to_string());
        let Some(next) = cursor.succ_opt() else { break };
        cursor = next;
    }
    keys
}

/// 逐月枚举 `[from, to]` 覆盖到的 `YYYY-MM`（含两端）。
fn enumerate_months(from: &str, to: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let mut year: i32 = from[0..4].parse().unwrap_or(1970);
    let mut month: u32 = from[5..7].parse().unwrap_or(1);
    let end_year: i32 = to[0..4].parse().unwrap_or(9999);
    let end_month: u32 = to[5..7].parse().unwrap_or(12);
    while (year, month) <= (end_year, end_month) && keys.len() < MAX_SERIES_KEYS {
        keys.push(format!("{year:04}-{month:02}"));
        month += 1;
        if month > 12 {
            month = 1;
            year += 1;
        }
    }
    keys
}

/// 序列的横轴。
///
/// day 粒度**严格跟着 --from/--to 走**（逐日枚举，含两端）：早先是「from 所在月的
/// 1 号到月末」，于是 `stats --from 2026-09-14 --to 2026-09-21` 的 `range` 是对的、
/// `series` 却给出整月 30 天，做周报的调用方必须自己裁，很容易把整月当成一周用。
/// 开区间（没给 --from 或 --to）逐日/逐月枚举会是几百万个空档，按出现过的日期收表。
fn series_keys(range: &StatsRange, entries: &[&LedgerEntry]) -> Vec<String> {
    let open_ended = range.from == OPEN_FROM || range.to == OPEN_TO;
    match (range.grain, open_ended) {
        ("day", false) => enumerate_days(&range.from, &range.to),
        ("day", true) => distinct_sorted(entries.iter().map(|entry| entry.date.clone())),
        (_, false) => enumerate_months(&range.from, &range.to),
        (_, true) => distinct_sorted(entries.iter().map(|entry| entry.date[0..7].to_string())),
    }
}

fn distinct_sorted(values: impl Iterator<Item = String>) -> Vec<String> {
    values.collect::<BTreeSet<String>>().into_iter().collect()
}

/// `ledger stats` 的分类聚合中间态：聚合走结构体 + 槽位表，最后才组装成 JSON
/// （形状与字段顺序和早先在 `serde_json::Value` 上原地累加的实现逐字节一致）。
struct CategoryGroup {
    category_id: String,
    name: String,
    side: LedgerSide,
    count: u64,
    cents: i64,
    percent: f64,
}

fn ledger_stats(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let params = &inv.params;
    let file = ctx.repo.load_ledger()?;
    set_read_revision(meta, Domain::Ledger, file.meta.revision);
    let range = stats_range(params)?;
    let side_filter = param_str(params, "side").and_then(|raw| LedgerSide::parse(&raw));

    let entries: Vec<&LedgerEntry> = file
        .entries
        .iter()
        .filter(|entry| in_range(&entry.date, Some(&range.from), Some(&range.to)))
        .collect();

    let mut income: i64 = 0;
    let mut expense: i64 = 0;
    let mut transfer: i64 = 0;
    // 一次遍历同时算出总额与每个时间桶（day = 完整日期，month = YYYY-MM）的收/支：
    // 早先是「每个 key 都把全部 entries 重扫一遍」的 O(keys × entries)。
    let mut buckets: BTreeMap<&str, (i64, i64)> = BTreeMap::new();
    for entry in &entries {
        let bucket = if range.grain == "day" {
            entry.date.as_str()
        } else {
            &entry.date[0..7]
        };
        let slot = buckets.entry(bucket).or_insert((0, 0));
        match entry.kind {
            LedgerKind::Income => {
                income += entry.amount_cents;
                slot.0 += entry.amount_cents;
            }
            LedgerKind::Expense => {
                expense += entry.amount_cents;
                slot.1 += entry.amount_cents;
            }
            LedgerKind::Transfer => transfer += entry.amount_cents,
        }
    }

    // 序列：按天或按月的收/支两条线（横轴见 series_keys，空档补零）
    let series: Vec<Value> = series_keys(&range, &entries)
        .iter()
        .map(|key| {
            let (bucket_income, bucket_expense) =
                buckets.get(key.as_str()).copied().unwrap_or((0, 0));
            json!({
                "key": key,
                "incomeCents": bucket_income,
                "expenseCents": bucket_expense,
            })
        })
        .collect();

    // 分类占比：归到大类一级（子分类的金额并进父类），未分类单独一组。
    // 早先每笔都做两次线性 find_category，还在 Value 数组里逐笔比字符串找槽位；
    // 现在先建 id → 分类 的索引，聚合走结构体 + 槽位表，最后再组装成 JSON。
    let category_index: HashMap<&str, &LedgerCategory> = file
        .categories
        .iter()
        .map(|category| (category.id.as_str(), category))
        .collect();
    let mut groups: Vec<CategoryGroup> = Vec::new();
    let mut slots: HashMap<(String, bool), usize> = HashMap::new();
    for entry in &entries {
        if entry.kind == LedgerKind::Transfer {
            continue;
        }
        let side = match entry.kind {
            LedgerKind::Expense => LedgerSide::Expense,
            _ => LedgerSide::Income,
        };
        if side_filter.map(|want| want != side).unwrap_or(false) {
            continue;
        }
        let category = entry
            .category_id
            .as_deref()
            .and_then(|id| category_index.get(id).copied());
        let (group_id, group_name) = match category {
            Some(category) => {
                let parent = category
                    .parent_id
                    .as_deref()
                    .and_then(|id| category_index.get(id).copied());
                match parent {
                    Some(parent) => (parent.id.as_str(), parent.name.as_str()),
                    None => (category.id.as_str(), category.name.as_str()),
                }
            }
            None => ("", "未分类"),
        };
        let slot_key = (group_id.to_string(), side == LedgerSide::Expense);
        match slots.get(&slot_key) {
            Some(&index) => {
                groups[index].count += 1;
                groups[index].cents += entry.amount_cents;
            }
            None => {
                slots.insert(slot_key, groups.len());
                groups.push(CategoryGroup {
                    category_id: group_id.to_string(),
                    name: group_name.to_string(),
                    side,
                    count: 1,
                    cents: entry.amount_cents,
                    percent: 0.0,
                });
            }
        }
    }
    for side in [LedgerSide::Expense, LedgerSide::Income] {
        let total: i64 = groups
            .iter()
            .filter(|group| group.side == side)
            .map(|group| group.cents)
            .sum();
        for group in groups.iter_mut().filter(|group| group.side == side) {
            group.percent = if total > 0 {
                (group.cents as f64 * 100.0 / total as f64 * 100.0).round() / 100.0
            } else {
                0.0
            };
        }
    }
    // sort_by 是稳定排序：金额相同保持首次出现的顺序（与旧实现一致）
    groups.sort_by(|a, b| b.cents.cmp(&a.cents));
    let categories: Vec<Value> = groups
        .iter()
        .map(|group| {
            json!({
                "categoryId": group.category_id,
                "name": group.name,
                "side": group.side.as_str(),
                "count": group.count,
                "cents": group.cents,
                "percent": group.percent,
            })
        })
        .collect();

    Ok(json!({
        "range": { "from": range.from, "to": range.to, "grain": range.grain, "label": range.label },
        "count": entries.len(),
        "totals": {
            "incomeCents": income,
            "income": cents_to_yuan(income),
            "expenseCents": expense,
            "expense": cents_to_yuan(expense),
            "netCents": income - expense,
            "net": cents_to_yuan(income - expense),
            "transferCents": transfer,
            "transfer": cents_to_yuan(transfer),
        },
        "series": series,
        "categories": categories,
    }))
}

fn ledger_balance(_inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let file = ctx.repo.load_ledger()?;
    set_read_revision(meta, Domain::Ledger, file.meta.revision);
    let mut accounts = file.accounts.clone();
    accounts.sort_by(|a, b| a.order.partial_cmp(&b.order).unwrap_or(std::cmp::Ordering::Equal));
    let mut net = 0i64;
    let mut liabilities = 0i64;
    let items: Vec<Value> = accounts
        .iter()
        .map(|account| {
            let balance = account_balance_cents(&file, &account.id);
            net += balance;
            if account.kind == ACCOUNT_KIND_CREDIT && balance < 0 {
                liabilities += -balance;
            }
            account_view(&file, account)
        })
        .collect();
    Ok(json!({
        "items": items,
        "netCents": net,
        "net": cents_to_yuan(net),
        "liabilitiesCents": liabilities,
        "liabilities": cents_to_yuan(liabilities),
        "assetsCents": net + liabilities,
        "assets": cents_to_yuan(net + liabilities),
    }))
}

// ---------------------------------------------------------------------------
// export / import
// ---------------------------------------------------------------------------

fn ledger_export(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let params = &inv.params;
    let file = ctx.repo.load_ledger()?;
    set_read_revision(meta, Domain::Ledger, file.meta.revision);
    let from = param_str(params, "from");
    let to = param_str(params, "to");
    let entries: Vec<&LedgerEntry> = file
        .entries
        .iter()
        .filter(|entry| in_range(&entry.date, from.as_deref(), to.as_deref()))
        .collect();
    let mut sorted = entries.clone();
    sorted.sort_by(|a, b| {
        a.date
            .cmp(&b.date)
            .then(a.time.cmp(&b.time))
            .then(a.created_at.cmp(&b.created_at))
    });
    let bytes = crate::ledger_archive::build_zip(&file, &sorted)?;
    let name = crate::ledger_archive::archive_name(from.as_deref(), to.as_deref());
    if let Some(out) = param_str(params, "out") {
        let target = std::path::Path::new(&out);
        if let Some(parent) = target.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        std::fs::write(target, &bytes)
            .map_err(|error| CoreError::io(format!("无法写入 {}：{error}", target.display())))?;
        return Ok(json!({
            "path": target.display().to_string(),
            "name": name,
            "entries": sorted.len(),
            "bytes": bytes.len(),
        }));
    }
    use base64::Engine as _;
    let base64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(json!({
        "name": name,
        "entries": sorted.len(),
        "bytes": bytes.len(),
        "base64": base64,
    }))
}

fn ledger_import(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let params = &inv.params;
    let encoded = params
        .get("xlsxBase64")
        .and_then(Value::as_str)
        .ok_or_else(|| CoreError::validation("MISSING_PARAM", "缺少 xlsxBase64 导出包内容"))?;
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim().as_bytes())
        .map_err(|error| {
            CoreError::validation("LEDGER_IMPORT_INVALID", format!("导出包解码失败：{error}"))
        })?;
    let parsed = crate::ledger_archive::parse_bytes(&bytes)?;
    if parsed.entries.is_empty() {
        return Err(CoreError::validation(
            "LEDGER_IMPORT_EMPTY",
            "导出包里没有任何账目行",
        ));
    }

    // 事务前校验：一行坏数据不能把整笔导入做成半成品
    let mut skipped = 0u64;
    for item in &parsed.entries {
        if parse_date(&item.date).is_err() || item.amount_cents <= 0 {
            skipped += 1;
        }
    }
    require_confirmation(
        &inv.controls,
        format!(
            "将导入 {} 笔账（{} 个账户、{} 个分类；重复导入会产生重复账目）",
            parsed.entries.len(),
            parsed.accounts.len(),
            parsed.categories.len()
        ),
        json!({ "type": "ledgerImport", "entries": parsed.entries.len() }),
    )?;
    if inv.controls.dry_run {
        return Ok(json!({
            "imported": 0,
            "skipped": skipped,
            "dryRun": true,
            "entries": parsed.entries.len(),
        }));
    }

    let now = now_iso();
    let mut ids: Vec<String> = Vec::new();
    let mut summary_out = json!({});
    let (_, outcome) = ctx.repo.write_ledger(
        inv.controls.if_revision,
        inv.controls.idempotency_key.as_deref(),
        &inv.command,
        |file| {
            let mut accounts_created = 0u64;
            let mut categories_created = 0u64;
            // 账户：按导出包补齐（名字相同就复用，导入比 CLI 宽容，不丢数据）
            for item in &parsed.accounts {
                if !file.accounts.iter().any(|acc| acc.name == item.name) {
                    let order = file
                        .accounts
                        .iter()
                        .map(|acc| acc.order)
                        .fold(0f64, f64::max)
                        + 1.0;
                    file.accounts.push(LedgerAccount {
                        id: gen_id("lacc"),
                        name: item.name.clone(),
                        icon: item.icon.clone(),
                        color: item.color.clone(),
                        kind: item.kind.clone(),
                        initial_cents: item.initial_cents,
                        note: String::new(),
                        order,
                        created_at: now.clone(),
                        updated_at: None,
                        extra: Map::new(),
                    });
                    accounts_created += 1;
                }
            }
            // 分类：两级都按名字补齐（同侧、同名、同父 = 同一个）
            for item in &parsed.categories {
                let parent_id = match &item.parent {
                    Some(parent_name) => {
                        let existing = file
                            .categories
                            .iter()
                            .find(|cat| {
                                cat.name == *parent_name
                                    && cat.side == item.side
                                    && cat.parent_id.is_none()
                            })
                            .map(|cat| cat.id.clone());
                        match existing {
                            Some(id) => Some(id),
                            None => {
                                let order = file
                                    .categories
                                    .iter()
                                    .filter(|cat| cat.side == item.side)
                                    .map(|cat| cat.order)
                                    .fold(0f64, f64::max)
                                    + 1.0;
                                let id = gen_id("lcat");
                                file.categories.push(LedgerCategory {
                                    id: id.clone(),
                                    name: parent_name.clone(),
                                    side: item.side,
                                    parent_id: None,
                                    icon: String::new(),
                                    color: String::new(),
                                    order,
                                    created_at: now.clone(),
                                    updated_at: None,
                                    extra: Map::new(),
                                });
                                categories_created += 1;
                                Some(id)
                            }
                        }
                    }
                    None => None,
                };
                let duplicated = file.categories.iter().any(|cat| {
                    cat.name == item.name && cat.side == item.side && cat.parent_id == parent_id
                });
                if !duplicated {
                    let order = file
                        .categories
                        .iter()
                        .filter(|cat| cat.side == item.side)
                        .map(|cat| cat.order)
                        .fold(0f64, f64::max)
                        + 1.0;
                    file.categories.push(LedgerCategory {
                        id: gen_id("lcat"),
                        name: item.name.clone(),
                        side: item.side,
                        parent_id,
                        icon: item.icon.clone(),
                        color: item.color.clone(),
                        order,
                        created_at: now.clone(),
                        updated_at: None,
                        extra: Map::new(),
                    });
                    categories_created += 1;
                }
            }

            let mut imported = 0u64;
            for item in &parsed.entries {
                let Ok(date) = parse_date(&item.date) else {
                    continue;
                };
                if item.amount_cents <= 0 {
                    continue;
                }
                let Ok(account) = resolve_account(file, &item.account) else {
                    continue;
                };
                let account_id = account.id.clone();
                let to_account_id = match &item.to_account {
                    Some(raw) if !raw.trim().is_empty() => {
                        match resolve_account(file, raw) {
                            Ok(target) => Some(target.id.clone()),
                            Err(_) => continue,
                        }
                    }
                    _ => None,
                };
                let category_id = if item.category_parent.is_some() || item.category.is_some() {
                    let side = kind_side(item.kind).unwrap_or(LedgerSide::Expense);
                    crate::ledger_archive::find_category_by_names(
                        &file.categories,
                        side,
                        item.category_parent.as_deref(),
                        item.category.as_deref(),
                    )
                } else {
                    None
                };
                let id = gen_id("ledger");
                file.entries.push(LedgerEntry {
                    id: id.clone(),
                    kind: item.kind,
                    amount_cents: item.amount_cents,
                    account_id,
                    to_account_id,
                    category_id,
                    date,
                    time: item.time.clone(),
                    note: item.note.clone(),
                    images: Vec::new(),
                    legacy_image: None,
                    created_at: now.clone(),
                    updated_at: None,
                    extra: Map::new(),
                });
                ids.push(id);
                imported += 1;
            }
            sort_entries(&mut file.entries);
            summary_out = json!({
                "imported": imported,
                "skipped": skipped,
                "accountsCreated": accounts_created,
                "categoriesCreated": categories_created,
            });
            Ok(summary_out.clone())
        },
    )?;
    apply_write_outcome(meta, Domain::Ledger, &outcome);
    notify_host(ctx, Domain::Ledger, outcome.revision, vec![]);
    if outcome.replayed {
        return Ok(outcome.replay_summary.clone().unwrap_or(Value::Null));
    }
    let mut summary = summary_out;
    if let Some(object) = summary.as_object_mut() {
        object.insert("revision".to_string(), json!(outcome.revision));
        object.insert("ids".to_string(), json!(ids));
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cents_parsing_rounds_third_decimal() {
        assert_eq!(parse_cents("12.5"), Some(1250));
        assert_eq!(parse_cents("12.50"), Some(1250));
        assert_eq!(parse_cents("0.005"), Some(1));
        assert_eq!(parse_cents("0.004"), Some(0));
        assert_eq!(parse_cents("1,234.56"), Some(123456));
        assert_eq!(parse_cents("-3.2"), Some(-320));
        assert_eq!(parse_cents("abc"), None);
        assert_eq!(parse_cents("1.2.3"), None);
    }

    #[test]
    fn cents_parsing_rejects_overflow_instead_of_quietly_zeroing() {
        // 前导零与纯零仍然合法
        assert_eq!(parse_cents("0"), Some(0));
        assert_eq!(parse_cents("000"), Some(0));
        assert_eq!(parse_cents("0.5"), Some(50));
        assert_eq!(parse_cents("007"), Some(700));
        // i64 分装得下的最大一档
        assert_eq!(parse_cents("92233720368547758.07"), Some(i64::MAX));
        assert_eq!(parse_cents("-92233720368547758.08"), Some(i64::MIN));
        // 再大一档就拒（早先 i64::try_from 已经挡住这一类）
        assert_eq!(parse_cents("92233720368547758.08"), None);
        // **≥39 位整数部分**：i128 也装不下。早先 `.unwrap_or(0)` 会把它静默变成 0
        // （甚至只剩小数部分），`account-modify --initial` 里 0 是合法值 → 期初被写成 0。
        let huge = "9".repeat(45);
        assert_eq!(parse_cents(&huge), None);
        assert_eq!(parse_cents(&format!("{huge}.50")), None);
        assert_eq!(parse_cents(&format!("-{huge}")), None);
        // 37 位：i128 装得下但 ×100 溢出，checked_mul 挡住
        assert_eq!(parse_cents(&"9".repeat(37)), None);
    }

    #[test]
    fn yuan_formatting_keeps_two_decimals() {
        assert_eq!(cents_to_yuan(1250), "12.50");
        assert_eq!(cents_to_yuan(5), "0.05");
        assert_eq!(cents_to_yuan(-1250), "-12.50");
        assert_eq!(cents_to_yuan(0), "0.00");
    }

    #[test]
    fn last_day_handles_leap_years() {
        assert_eq!(last_day_of_month(2024, 2), 29);
        assert_eq!(last_day_of_month(2026, 2), 28);
        assert_eq!(last_day_of_month(2026, 12), 31);
    }

    #[test]
    fn span_days_counts_both_ends() {
        assert_eq!(span_days("2026-09-09", "2026-09-09"), Some(1));
        assert_eq!(span_days("2026-09-14", "2026-09-21"), Some(8));
        // 跨月的短区间：周报要的就是这几天的日粒度，不能被摊成两个月桶
        assert_eq!(span_days("2026-09-28", "2026-10-03"), Some(6));
        assert_eq!(span_days("2026-09-01", "2026-10-01"), Some(31));
        assert_eq!(span_days("2026-09-01", "2026-10-02"), Some(32));
        // 倒挂的区间是负数（grain 判定与 enumerate_days 都按「没有任何一天」处理）
        assert_eq!(span_days("2026-09-20", "2026-09-10"), Some(-9));
        assert_eq!(span_days("nope", "2026-09-10"), None);
    }

    #[test]
    fn grain_is_day_up_to_the_threshold_then_month() {
        let grain = |from: &str, to: &str| {
            stats_range(&json!({ "from": from, "to": to }))
                .expect("合法区间")
                .grain
        };
        // --month（30/31 天）与周报（跨月 6 天）都按天
        assert_eq!(grain("2026-09-01", "2026-09-30"), "day");
        assert_eq!(grain("2026-09-28", "2026-10-03"), "day");
        // 门槛含两端：62 天按天、63 天按月
        assert_eq!(grain("2026-01-01", "2026-03-03"), "day");
        assert_eq!(span_days("2026-01-01", "2026-03-03"), Some(DAY_GRAIN_MAX_DAYS));
        assert_eq!(grain("2026-01-01", "2026-03-04"), "month");
        // --year 按月
        assert_eq!(grain("2026-01-01", "2026-12-31"), "month");
    }

    /// 日粒度门槛**必须与前端 `src/lib/ledger.ts::bucketOf` 同一个数**：两边各判一次，
    /// 漂移了就是同一段区间 GUI 画日桶、CLI 给月桶。与 `tests/ledger_icons.rs` 同一路数
    /// （include_str! 前端源码直接比对），改一边就会被这条挡住。
    #[test]
    fn day_grain_threshold_matches_frontend() {
        const TS: &str = include_str!("../../../../src/lib/ledger.ts");
        let needle = "days <= ";
        let at = TS.find(needle).expect("前端 bucketOf 的门槛判定不见了");
        let digits: String = TS[at + needle.len()..]
            .chars()
            .take_while(|ch| ch.is_ascii_digit())
            .collect();
        assert_eq!(
            digits.parse::<i64>().expect("前端门槛不是数字"),
            DAY_GRAIN_MAX_DAYS,
            "core 与前端的日粒度门槛不一致：改一边必须改另一边"
        );
    }

    #[test]
    fn enumerate_days_crosses_month_and_year_boundaries() {
        assert_eq!(
            enumerate_days("2026-09-28", "2026-10-03"),
            vec![
                "2026-09-28", "2026-09-29", "2026-09-30",
                "2026-10-01", "2026-10-02", "2026-10-03",
            ]
        );
        assert_eq!(
            enumerate_days("2026-12-30", "2027-01-02"),
            vec!["2026-12-30", "2026-12-31", "2027-01-01", "2027-01-02"]
        );
        // 闰月：2028-02 有 29 天
        assert_eq!(enumerate_days("2028-02-01", "2028-02-29").len(), 29);
        assert_eq!(enumerate_days("2026-02-01", "2026-02-28").len(), 28);
        // 单天与倒挂
        assert_eq!(enumerate_days("2026-09-09", "2026-09-09"), vec!["2026-09-09"]);
        assert!(enumerate_days("2026-09-20", "2026-09-10").is_empty());
        assert!(enumerate_days("nope", "2026-09-10").is_empty());
        // 硬顶：畸形区间不会生成上百万个空档
        assert_eq!(enumerate_days("0000-01-01", "9999-12-31").len(), MAX_SERIES_KEYS);
    }

    #[test]
    fn enumerate_months_covers_the_range() {
        assert_eq!(
            enumerate_months("2026-08-01", "2026-10-31"),
            vec!["2026-08", "2026-09", "2026-10"]
        );
        assert_eq!(enumerate_months("2026-01-01", "2026-12-31").len(), 12);
        assert_eq!(
            enumerate_months("2025-11-15", "2026-02-03"),
            vec!["2025-11", "2025-12", "2026-01", "2026-02"]
        );
    }
}
