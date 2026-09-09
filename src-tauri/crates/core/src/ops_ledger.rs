//! ledger domain: 记账的业务操作（GUI 与 CLI 共用同一条命令层）。
//!
//! 账本住在自己的 `ledger.json` 里（第五个领域文件），于是记一笔账既不抬高 data 域的
//! revision，也不和任务/日记抢同一把幂等台账。同步上它有独立的「账本」范围勾选。
//!
//! 金额一律**整数分**（i64）：浮点累加在统计里会 drift，而分是记账的最小单位。
//! 对外（CLI/Excel/JSON 输出）同时给 `amountCents` 与两位小数的 `amount` 字符串。

use serde_json::{json, Map, Value};

use crate::core::{
    apply_write_outcome, idem_summary, notify_host, param_str, require_confirmation, required_str,
    set_read_revision, ExecContext, Invocation,
};
use crate::envelope::Meta;
use crate::error::{CoreError, CoreResult};
use crate::ids::gen_id;
use crate::model::{
    AccountKind, LedgerAccount, LedgerCategory, LedgerEntry, LedgerFile, LedgerKind, LedgerSide,
};
use crate::repo::Domain;
use crate::time::{now_iso, parse_date, today_local};

pub fn ledger_dispatch(
    action: &str,
    inv: &Invocation,
    ctx: &ExecContext,
    meta: &mut Meta,
) -> CoreResult<Value> {
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
        "categories" => ledger_categories(inv, ctx, meta),
        "categoryAdd" => ledger_category_add(inv, ctx, meta),
        "categoryModify" => ledger_category_modify(inv, ctx, meta),
        "categoryRemove" => ledger_category_remove(inv, ctx, meta),
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
    let whole_cents: i128 = whole
        .trim_start_matches('0')
        .parse::<i128>()
        .unwrap_or(0)
        .checked_mul(100)?;
    // 两位小数截到分，第三位四舍五入
    let frac_digits: String = format!("{frac:0<3}");
    let tenths_hundredths: i128 = frac_digits[0..2].parse().ok()?;
    let third: i128 = frac_digits[2..3].parse().ok()?;
    let mut cents = whole_cents + tenths_hundredths;
    if third >= 5 {
        cents += 1;
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
    Ok(EntryDraft {
        kind,
        amount_cents,
        account_id,
        to_account_id,
        category_id,
        date,
        time,
        note,
    })
}

// ---------------------------------------------------------------------------
// entries
// ---------------------------------------------------------------------------

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
    let limit = params.get("limit").and_then(Value::as_u64);

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
    let total = entries.len() as u64;
    if let Some(limit) = limit {
        entries.truncate(limit as usize);
    }
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
    let (_, outcome) = ctx.repo.write_ledger(
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
        Some(raw) => AccountKind::parse(&raw).ok_or_else(|| {
            CoreError::validation(
                "LEDGER_ACCOUNT_KIND_INVALID",
                format!("无效账户类型 `{raw}`，支持 cash|debit|credit|investment|other"),
            )
        })?,
        None => AccountKind::default(),
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
                Some(raw) => Some(AccountKind::parse(&raw).ok_or_else(|| {
                    CoreError::validation(
                        "LEDGER_ACCOUNT_KIND_INVALID",
                        format!("无效账户类型 `{raw}`"),
                    )
                })?),
                None => None,
            };
            let initial = cents_param(params, "initial")?;
            let position = file
                .accounts
                .iter()
                .position(|item| item.id == id)
                .ok_or_else(|| account_not_found(&id))?;
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
// categories
// ---------------------------------------------------------------------------

fn ledger_categories(inv: &Invocation, ctx: &ExecContext, meta: &mut Meta) -> CoreResult<Value> {
    let file = ctx.repo.load_ledger()?;
    set_read_revision(meta, Domain::Ledger, file.meta.revision);
    let side = param_str(&inv.params, "side").and_then(|raw| LedgerSide::parse(&raw));
    let mut categories = file.categories.clone();
    categories.sort_by(|a, b| {
        a.side
            .as_str()
            .cmp(b.side.as_str())
            .then(a.order.partial_cmp(&b.order).unwrap_or(std::cmp::Ordering::Equal))
    });
    let items: Vec<Value> = categories
        .iter()
        .filter(|item| side.map(|want| item.side == want).unwrap_or(true))
        .map(category_view)
        .collect();
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
            None => "0000-01-01".to_string(),
        };
        let to = match to {
            Some(raw) => parse_date(&raw)?,
            None => "9999-12-31".to_string(),
        };
        let grain = if from.len() >= 7 && to.len() >= 7 && from[0..7] == to[0..7] {
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
    for entry in &entries {
        match entry.kind {
            LedgerKind::Income => income += entry.amount_cents,
            LedgerKind::Expense => expense += entry.amount_cents,
            LedgerKind::Transfer => transfer += entry.amount_cents,
        }
    }

    // 序列：按天或按月的收/支两条线
    let mut series: Vec<Value> = Vec::new();
    {
        let keys: Vec<String> = if range.grain == "day" {
            let (year, month) = {
                let year: i32 = range.from[0..4].parse().unwrap_or(1970);
                let month: u32 = range.from[5..7].parse().unwrap_or(1);
                (year, month)
            };
            if range.from == "0000-01-01" || range.to == "9999-12-31" {
                // 全量范围按出现的日期排序
                let mut keys: Vec<String> = entries
                    .iter()
                    .map(|entry| entry.date.clone())
                    .collect::<std::collections::BTreeSet<String>>()
                    .into_iter()
                    .collect();
                keys.sort();
                keys
            } else {
                let last = last_day_of_month(year, month);
                (1..=last).map(|day| format!("{year:04}-{month:02}-{day:02}")).collect()
            }
        } else if range.from != "0000-01-01" && range.to != "9999-12-31" {
            // 自定义跨月范围：逐月枚举
            let mut keys = Vec::new();
            let mut year: i32 = range.from[0..4].parse().unwrap_or(1970);
            let mut month: u32 = range.from[5..7].parse().unwrap_or(1);
            let end_year: i32 = range.to[0..4].parse().unwrap_or(9999);
            let end_month: u32 = range.to[5..7].parse().unwrap_or(12);
            while (year, month) <= (end_year, end_month) && keys.len() < 1200 {
                keys.push(format!("{year:04}-{month:02}"));
                month += 1;
                if month > 12 {
                    month = 1;
                    year += 1;
                }
            }
            keys
        } else {
            let mut keys: Vec<String> = entries
                .iter()
                .map(|entry| entry.date[0..7].to_string())
                .collect::<std::collections::BTreeSet<String>>()
                .into_iter()
                .collect();
            keys.sort();
            keys
        };
        for key in keys {
            let mut day_income = 0i64;
            let mut day_expense = 0i64;
            for entry in &entries {
                let entry_key = if range.grain == "day" {
                    entry.date.as_str()
                } else {
                    &entry.date[0..7]
                };
                if entry_key != key {
                    continue;
                }
                match entry.kind {
                    LedgerKind::Income => day_income += entry.amount_cents,
                    LedgerKind::Expense => day_expense += entry.amount_cents,
                    LedgerKind::Transfer => {}
                }
            }
            series.push(json!({
                "key": key,
                "incomeCents": day_income,
                "expenseCents": day_expense,
            }));
        }
    }

    // 分类占比：归到大类一级（子分类的金额并进父类），未分类单独一组
    let mut groups: Vec<Value> = Vec::new();
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
        let (group_id, group_name) = match entry.category_id.as_deref().and_then(|cid| find_category(&file, cid)) {
            Some(category) => match category.parent_id.as_deref().and_then(|pid| find_category(&file, pid)) {
                Some(parent) => (parent.id.clone(), parent.name.clone()),
                None => (category.id.clone(), category.name.clone()),
            },
            None => (String::new(), "未分类".to_string()),
        };
        let slot = groups.iter_mut().find(|item| {
            item["categoryId"].as_str() == Some(group_id.as_str()) && item["side"].as_str() == Some(side.as_str())
        });
        match slot {
            Some(item) => {
                item["count"] = json!(item["count"].as_u64().unwrap_or(0) + 1);
                let cents = item["cents"].as_i64().unwrap_or(0) + entry.amount_cents;
                item["cents"] = json!(cents);
            }
            None => groups.push(json!({
                "categoryId": group_id,
                "name": group_name,
                "side": side.as_str(),
                "count": 1,
                "cents": entry.amount_cents,
            })),
        }
    }
    for side in [LedgerSide::Expense, LedgerSide::Income] {
        let total: i64 = groups
            .iter()
            .filter(|item| item["side"].as_str() == Some(side.as_str()))
            .map(|item| item["cents"].as_i64().unwrap_or(0))
            .sum();
        for item in groups.iter_mut() {
            if item["side"].as_str() != Some(side.as_str()) {
                continue;
            }
            let cents = item["cents"].as_i64().unwrap_or(0);
            let percent = if total > 0 {
                (cents as f64 * 100.0 / total as f64 * 100.0).round() / 100.0
            } else {
                0.0
            };
            item["percent"] = json!(percent);
        }
    }
    groups.sort_by(|a, b| {
        b["cents"]
            .as_i64()
            .unwrap_or(0)
            .cmp(&a["cents"].as_i64().unwrap_or(0))
    });

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
        "categories": groups,
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
            if account.kind == AccountKind::Credit && balance < 0 {
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
                        kind: item.kind,
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
}
