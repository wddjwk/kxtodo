//! Built-in jq-compatible subset for `--jq` (§3.2).
//! Supported syntax is listed by `kxtodo-cli schema jq`.

use serde_json::{json, Map, Value};

use crate::error::{CoreError, CoreResult};

pub const JQ_SUBSET_DOC: &str = r#"{
  "syntax": [
    ".                 恒等",
    ".a.b.c           字段路径",
    ".\"key\"         带特殊字符的键",
    ".a[0]            数组下标（支持负数）",
    ".a[] 或 .[]      展开数组（结果为多个值，序列化为数组）",
    "expr | expr      管道",
    ".a, .b           逗号多输出（优先级高于 |；结果为多个值，序列化为数组）",
    "{a, b}           对象构造（简写 = 同名字段路径，键取路径最后一段）",
    "{\"名字\": .a.b}  对象构造的显式键名（键可以是裸标识符或引号字符串）",
    "length           数组/字符串长度或对象键数",
    "keys             对象的键数组",
    "first / last     数组首/尾元素",
    "map(expr)        对数组每个元素应用 expr（expr 可以是 {…}、管道或任何受支持的段）",
    "select(.a == 值) 条件过滤（== / !=，值支持字符串、数字、true/false/null）"
  ],
  "examples": [
    ".data.items",
    ".data.items | length",
    ".data.items[] | .id",
    ".data.items | map(.id)",
    ".data.items[] | select(.completed == false) | .markdown",
    ".data.items | map({id, markdown})",
    ".data.items[] | {id, done: .completed}",
    ".data.total, .meta.count"
  ]
}"#;

/// 解析失败时**内联**给出的支持清单：调用方（Agent）不必再跑一次 `schema jq`
/// 才知道能写什么——那条往返在自动化里就是白烧一轮。
const SUPPORT_SUMMARY: &str = "支持：`.` 恒等｜`.a.b` 字段路径｜`.\"key\"` 特殊字符键｜`.a[0]` 下标（可负）｜`.a[]` 展开数组｜`a | b` 管道｜`.a, .b` 逗号多输出｜`{a, b}` 与 `{\"名字\": .a.b}` 对象构造｜length｜keys｜first｜last｜map(expr)｜select(.a == 值 / .a != 值)；完整清单与示例见 kxtodo-cli schema jq";

#[derive(Debug, Clone)]
enum Stage {
    Access(Vec<Accessor>),
    Length,
    Keys,
    First,
    Last,
    Map(Vec<Stage>),
    Select(PathCondition),
    /// `{a, b}` / `{"名字": .a.b}`：逐字段对当前输入求值再组装成对象。
    /// 某个字段产出多个值时按 jq 语义做笛卡尔积（一个组合一个对象）。
    Object(Vec<(String, Vec<Stage>)>),
}

#[derive(Debug, Clone)]
enum Accessor {
    Field(String),
    Index(i64),
    Iterate,
}

#[derive(Debug, Clone)]
struct PathCondition {
    path: Vec<Accessor>,
    negate: bool,
    literal: Value,
}

/// 一条管道：`a | b | c`（`map(...)` 的内容与对象字段的值都是这种）。
type Pipeline = Vec<Stage>;
/// 顶层程序按 `|` 切出的段；段内再按 `,` 切分支。`,` 的优先级高于 `|`，
/// 所以段里每个逗号分支恰好是一个 Stage。
type Segment = Vec<Stage>;

#[derive(Debug, Clone)]
pub struct CompiledJq {
    segments: Vec<Segment>,
}

pub fn compile(expr: &str) -> CoreResult<CompiledJq> {
    Ok(CompiledJq {
        segments: parse_program(expr)?,
    })
}

pub fn evaluate(program: &CompiledJq, input: &Value) -> CoreResult<Value> {
    let results = eval_program(&program.segments, input)?;
    // Multiple values (from iteration or `,`) are serialized as an array; a
    // single value is returned as-is.
    if results.len() == 1 {
        Ok(results.into_iter().next().unwrap_or(Value::Null))
    } else {
        Ok(Value::Array(results))
    }
}

pub fn apply(expr: &str, input: &Value) -> CoreResult<Value> {
    evaluate(&compile(expr)?, input)
}

/// `,` 的优先级高于 `|`：先按 `|` 切段，段内再按 `,` 切分支。于是
/// `.a | .b, .c` = `.a | (.b, .c)`、`.a, .b | .c` = `(.a, .b) | .c`，与 jq 一致。
fn parse_program(expr: &str) -> CoreResult<Vec<Segment>> {
    let mut segments: Vec<Segment> = Vec::new();
    for part in split_top_level(expr, '|')? {
        let mut stages = Vec::new();
        for branch in split_top_level(&part, ',')? {
            stages.push(parse_stage(branch.trim())?);
        }
        segments.push(stages);
    }
    Ok(segments)
}

fn parse_pipeline(expr: &str) -> CoreResult<Pipeline> {
    split_top_level(expr, '|')?
        .iter()
        .map(|part| parse_stage(part.trim()))
        .collect()
}

/// 按 `delimiter` 切分，但跳过字符串字面量与 `()` `{}` `[]` 里的内容。
fn split_top_level(expr: &str, delimiter: char) -> CoreResult<Vec<String>> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut string_ch = '"';
    let mut escaped = false;
    for ch in expr.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        if in_string {
            current.push(ch);
            if ch == '\\' {
                escaped = true;
            } else if ch == string_ch {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' | '\'' => {
                in_string = true;
                string_ch = ch;
                current.push(ch);
            }
            '(' | '{' | '[' => {
                depth += 1;
                current.push(ch);
            }
            ')' | '}' | ']' => {
                depth -= 1;
                if depth < 0 {
                    return Err(bad_syntax(expr, "括号不配对"));
                }
                current.push(ch);
            }
            c if c == delimiter && depth == 0 => {
                parts.push(current.trim().to_string());
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    if depth != 0 || in_string {
        return Err(bad_syntax(expr, "括号或字符串未闭合"));
    }
    if !current.trim().is_empty() || parts.is_empty() {
        parts.push(current.trim().to_string());
    }
    Ok(parts)
}

fn parse_stage(expr: &str) -> CoreResult<Stage> {
    if expr.is_empty() {
        return Err(bad_syntax(expr, "空表达式"));
    }
    if expr == "length" {
        return Ok(Stage::Length);
    }
    if expr == "keys" {
        return Ok(Stage::Keys);
    }
    if expr == "first" {
        return Ok(Stage::First);
    }
    if expr == "last" {
        return Ok(Stage::Last);
    }
    if let Some(inner) = expr.strip_prefix("map(").and_then(|v| v.strip_suffix(')')) {
        return Ok(Stage::Map(parse_pipeline(inner)?));
    }
    if let Some(inner) = expr
        .strip_prefix("select(")
        .and_then(|v| v.strip_suffix(')'))
    {
        return Ok(Stage::Select(parse_condition(inner)?));
    }
    if expr.starts_with('{') {
        return Ok(Stage::Object(parse_object(expr)?));
    }
    if expr.starts_with('.') {
        return Ok(Stage::Access(parse_accessors(expr)?));
    }
    Err(bad_syntax(expr, "不支持的表达式"))
}

/// `{a, b}` / `{"名字": .a.b}` → (键, 求值管道) 列表；`{}` 是空对象。
fn parse_object(expr: &str) -> CoreResult<Vec<(String, Pipeline)>> {
    let Some(inner) = expr
        .strip_prefix('{')
        .and_then(|rest| rest.strip_suffix('}'))
    else {
        return Err(bad_syntax(expr, "对象构造要以 { 开头、} 结尾"));
    };
    if inner.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut fields = Vec::new();
    for raw in split_top_level(inner, ',')? {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err(bad_syntax(expr, "对象构造里有空字段"));
        }
        fields.push(parse_object_field(expr, raw)?);
    }
    Ok(fields)
}

fn parse_object_field(whole: &str, raw: &str) -> CoreResult<(String, Pipeline)> {
    // 显式键名：`键: 表达式`
    let parts = split_top_level(raw, ':')?;
    if parts.len() > 1 {
        let key = parse_object_key(whole, parts[0].trim())?;
        let value = parse_pipeline(&parts[1..].join(":"))?;
        return Ok((key, value));
    }
    // 简写：`{a}` = `{"a": .a}`；`{.a.b}` 的键取路径最后一段（= `{"b": .a.b}`）
    let path = if raw.starts_with('.') {
        raw.to_string()
    } else {
        format!(".{raw}")
    };
    let value = parse_pipeline(&path)?;
    let key = match value.as_slice() {
        [Stage::Access(accessors)] => match accessors.last() {
            Some(Accessor::Field(name)) => name.clone(),
            _ => return Err(bad_syntax(whole, &format!("对象简写 `{raw}` 的键必须是字段名"))),
        },
        _ => return Err(bad_syntax(whole, &format!("对象简写 `{raw}` 只能是字段路径"))),
    };
    Ok((key, value))
}

fn parse_object_key(whole: &str, raw: &str) -> CoreResult<String> {
    if let Some(inner) = raw.strip_prefix('"').and_then(|key| key.strip_suffix('"')) {
        return Ok(inner.replace("\\\"", "\""));
    }
    if let Some(inner) = raw
        .strip_prefix('\'')
        .and_then(|key| key.strip_suffix('\''))
    {
        return Ok(inner.replace("\\'", "'"));
    }
    if !raw.is_empty() && raw.chars().all(is_bare_key_char) {
        return Ok(raw.to_string());
    }
    Err(bad_syntax(whole, &format!("无效的对象键 `{raw}`")))
}

fn is_bare_key_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_' || ch == '-'
}

fn parse_accessors(expr: &str) -> CoreResult<Vec<Accessor>> {
    let mut accessors = Vec::new();
    let chars: Vec<char> = expr.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        match chars[index] {
            '.' => {
                index += 1;
                // ."key" form
                if index < chars.len() && (chars[index] == '"' || chars[index] == '\'') {
                    let quote = chars[index];
                    index += 1;
                    let mut key = String::new();
                    while index < chars.len() && chars[index] != quote {
                        if chars[index] == '\\' && index + 1 < chars.len() {
                            index += 1;
                        }
                        key.push(chars[index]);
                        index += 1;
                    }
                    if index >= chars.len() {
                        return Err(bad_syntax(expr, "字符串未闭合"));
                    }
                    index += 1;
                    accessors.push(Accessor::Field(key));
                    continue;
                }
                let mut key = String::new();
                while index < chars.len() && is_bare_key_char(chars[index]) {
                    key.push(chars[index]);
                    index += 1;
                }
                if key.is_empty() {
                    if index >= chars.len() {
                        break;
                    }
                    // `.[]` / `.[0]`：`.` 后面直接跟下标，字段名留给空。JQ_SUBSET_DOC
                    // 一直写着支持 `.[]`，早先这里会报「`.` 后必须是字段名」。
                    if chars[index] == '[' {
                        continue;
                    }
                    return Err(bad_syntax(expr, "`.` 后必须是字段名"));
                }
                accessors.push(Accessor::Field(key));
            }
            '[' => {
                index += 1;
                if index < chars.len() && chars[index] == ']' {
                    accessors.push(Accessor::Iterate);
                    index += 1;
                    continue;
                }
                if index < chars.len() && (chars[index] == '"' || chars[index] == '\'') {
                    let quote = chars[index];
                    index += 1;
                    let mut key = String::new();
                    while index < chars.len() && chars[index] != quote {
                        if chars[index] == '\\' && index + 1 < chars.len() {
                            index += 1;
                        }
                        key.push(chars[index]);
                        index += 1;
                    }
                    index += 1; // closing quote
                    if index >= chars.len() || chars[index] != ']' {
                        return Err(bad_syntax(expr, "缺少 ]"));
                    }
                    index += 1;
                    accessors.push(Accessor::Field(key));
                    continue;
                }
                let mut number = String::new();
                while index < chars.len() && (chars[index].is_ascii_digit() || chars[index] == '-')
                {
                    number.push(chars[index]);
                    index += 1;
                }
                if index >= chars.len() || chars[index] != ']' {
                    return Err(bad_syntax(expr, "缺少 ]"));
                }
                index += 1;
                let parsed: i64 = number
                    .parse()
                    .map_err(|_| bad_syntax(expr, "数组下标无效"))?;
                accessors.push(Accessor::Index(parsed));
            }
            other => {
                return Err(bad_syntax(expr, &format!("无法解析 `{other}`")));
            }
        }
    }
    Ok(accessors)
}

fn parse_condition(expr: &str) -> CoreResult<PathCondition> {
    for op in ["!=", "=="] {
        if let Some(pos) = expr.find(op) {
            let path_raw = expr[..pos].trim();
            let literal_raw = expr[pos + 2..].trim();
            if !path_raw.starts_with('.') {
                return Err(bad_syntax(expr, "select 条件左侧必须是字段路径"));
            }
            let literal = parse_literal(literal_raw)?;
            return Ok(PathCondition {
                path: parse_accessors(path_raw)?,
                negate: op == "!=",
                literal,
            });
        }
    }
    Err(bad_syntax(expr, "select 条件需要 == 或 !="))
}

fn parse_literal(raw: &str) -> CoreResult<Value> {
    if raw == "true" {
        return Ok(Value::Bool(true));
    }
    if raw == "false" {
        return Ok(Value::Bool(false));
    }
    if raw == "null" {
        return Ok(Value::Null);
    }
    if (raw.starts_with('"') && raw.ends_with('"'))
        || (raw.starts_with('\'') && raw.ends_with('\''))
    {
        let inner = &raw[1..raw.len() - 1];
        return Ok(Value::String(
            inner.replace("\\\"", "\"").replace("\\'", "'"),
        ));
    }
    if let Ok(number) = raw.parse::<i64>() {
        return Ok(json!(number));
    }
    if let Ok(number) = raw.parse::<f64>() {
        return Ok(json!(number));
    }
    Err(bad_syntax(raw, "不支持的字面值"))
}

fn bad_syntax(expr: &str, reason: &str) -> CoreError {
    CoreError::validation("JQ_SYNTAX", format!("jq 表达式 `{expr}` 无效：{reason}"))
        .with_hint(SUPPORT_SUMMARY)
}

fn eval_program(segments: &[Segment], input: &Value) -> CoreResult<Vec<Value>> {
    let mut values = vec![input.clone()];
    for segment in segments {
        let mut next: Vec<Value> = Vec::new();
        for value in std::mem::take(&mut values) {
            if segment.len() == 1 {
                // 单分支（最常见）直接交出所有权，省一次 clone
                eval_stage(&segment[0], value, &mut next)?;
            } else {
                // 逗号多输出：每个分支各求一次值，结果按写下的顺序串起来
                for stage in segment {
                    eval_stage(stage, value.clone(), &mut next)?;
                }
            }
        }
        values = next;
    }
    Ok(values)
}

fn eval_stages(stages: &[Stage], mut values: Vec<Value>) -> CoreResult<Vec<Value>> {
    for stage in stages {
        let mut next = Vec::new();
        for value in values {
            eval_stage(stage, value, &mut next)?;
        }
        values = next;
    }
    Ok(values)
}

fn eval_stage(stage: &Stage, value: Value, out: &mut Vec<Value>) -> CoreResult<()> {
    match stage {
        Stage::Access(accessors) => eval_access(accessors, value, out),
        Stage::Length => {
            let len = match &value {
                Value::Array(items) => Some(items.len()),
                Value::String(text) => Some(text.chars().count()),
                Value::Object(map) => Some(map.len()),
                Value::Null => Some(0),
                _ => None,
            };
            match len {
                Some(len) => {
                    out.push(json!(len));
                    Ok(())
                }
                None => Err(CoreError::validation(
                    "JQ_TYPE",
                    "length 只能用于数组/字符串/对象/null",
                )),
            }
        }
        Stage::Keys => match &value {
            Value::Object(map) => {
                out.push(json!(map.keys().collect::<Vec<_>>()));
                Ok(())
            }
            _ => Err(CoreError::validation("JQ_TYPE", "keys 只能用于对象")),
        },
        Stage::First => match &value {
            Value::Array(items) => {
                out.push(items.first().cloned().unwrap_or(Value::Null));
                Ok(())
            }
            _ => Err(CoreError::validation("JQ_TYPE", "first 只能用于数组")),
        },
        Stage::Last => match &value {
            Value::Array(items) => {
                out.push(items.last().cloned().unwrap_or(Value::Null));
                Ok(())
            }
            _ => Err(CoreError::validation("JQ_TYPE", "last 只能用于数组")),
        },
        Stage::Map(stages) => match value {
            Value::Array(items) => {
                let mapped = eval_stages(stages, items)?;
                out.push(Value::Array(mapped));
                Ok(())
            }
            _ => Err(CoreError::validation("JQ_TYPE", "map 只能用于数组")),
        },
        Stage::Select(condition) => {
            let mut probe = Vec::new();
            eval_access(&condition.path, value.clone(), &mut probe)?;
            let matched = probe.iter().any(|candidate| {
                if values_equal(candidate, &condition.literal) {
                    !condition.negate
                } else {
                    condition.negate
                }
            });
            if matched {
                out.push(value);
            }
            Ok(())
        }
        Stage::Object(fields) => {
            // 逐字段对**当前输入**求值；某个字段产出多个值时按 jq 语义做笛卡尔积。
            let mut built: Vec<Map<String, Value>> = vec![Map::new()];
            for (key, pipeline) in fields {
                let mut next: Vec<Map<String, Value>> = Vec::new();
                for partial in &built {
                    for item in eval_stages(pipeline, vec![value.clone()])? {
                        let mut object = partial.clone();
                        object.insert(key.clone(), item);
                        next.push(object);
                    }
                }
                built = next;
            }
            out.extend(built.into_iter().map(Value::Object));
            Ok(())
        }
    }
}

fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        _ => a == b,
    }
}

fn eval_access(accessors: &[Accessor], value: Value, out: &mut Vec<Value>) -> CoreResult<()> {
    let Some((first, rest)) = accessors.split_first() else {
        out.push(value);
        return Ok(());
    };
    match first {
        Accessor::Field(key) => match value {
            Value::Object(map) => {
                let next = map.get(key).cloned().unwrap_or(Value::Null);
                eval_access(rest, next, out)
            }
            Value::Null => eval_access(rest, Value::Null, out),
            _ => Err(CoreError::validation(
                "JQ_TYPE",
                format!("无法在非对象值上取字段 `{key}`"),
            )),
        },
        Accessor::Index(index) => match value {
            Value::Array(items) => {
                let len = items.len() as i64;
                let resolved = if *index < 0 { len + index } else { *index };
                let next = if resolved >= 0 && resolved < len {
                    items[resolved as usize].clone()
                } else {
                    Value::Null
                };
                eval_access(rest, next, out)
            }
            _ => Err(CoreError::validation("JQ_TYPE", "下标只能用于数组")),
        },
        Accessor::Iterate => match value {
            Value::Array(items) => {
                for item in items {
                    eval_access(rest, item, out)?;
                }
                Ok(())
            }
            _ => Err(CoreError::validation("JQ_TYPE", "[] 只能用于数组")),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Value {
        json!({
            "ok": true,
            "command": "task.list",
            "data": {
                "total": 3,
                "items": [
                    { "id": "a", "completed": false, "markdown": "任务A", "entry": { "name": "收集箱" } },
                    { "id": "b", "completed": true, "markdown": "任务B", "entry": { "name": "收集箱" } },
                    { "id": "c", "completed": false, "markdown": "任务C", "entry": { "name": "工作" } }
                ]
            },
            "meta": { "count": 3 }
        })
    }

    #[test]
    fn object_shorthand_picks_named_fields() {
        let input = fixture();
        assert_eq!(
            apply(".data.items[0] | {id, markdown}", &input).unwrap(),
            json!({ "id": "a", "markdown": "任务A" })
        );
        // 键的先后就是写下的先后（serde_json 开了 preserve_order）
        assert_eq!(
            apply(".data.items[0] | {markdown, id}", &input)
                .unwrap()
                .to_string(),
            "{\"markdown\":\"任务A\",\"id\":\"a\"}"
        );
        // 简写可以是完整路径，键取路径最后一段
        assert_eq!(
            apply(".data.items[0] | {.entry.name}", &input).unwrap(),
            json!({ "name": "收集箱" })
        );
    }

    #[test]
    fn object_construction_over_an_array() {
        let input = fixture();
        assert_eq!(
            apply(".data.items | map({id, done: .completed})", &input).unwrap(),
            json!([
                { "id": "a", "done": false },
                { "id": "b", "done": true },
                { "id": "c", "done": false }
            ])
        );
        assert_eq!(
            apply(".data.items[] | {id, entry: .entry.name}", &input).unwrap(),
            json!([
                { "id": "a", "entry": "收集箱" },
                { "id": "b", "entry": "收集箱" },
                { "id": "c", "entry": "工作" }
            ])
        );
    }

    #[test]
    fn object_keys_accept_quotes_and_missing_fields_are_null() {
        let input = fixture();
        assert_eq!(
            apply(".data.items[0] | {\"任务\": .markdown, 'id': .id}", &input).unwrap(),
            json!({ "任务": "任务A", "id": "a" })
        );
        // 不存在的字段是 null（与 jq 一致），不报错
        assert_eq!(
            apply(".data.items[0] | {nope}", &input).unwrap(),
            json!({ "nope": null })
        );
        assert_eq!(apply(".data.items[0] | {}", &input).unwrap(), json!({}));
    }

    #[test]
    fn comma_produces_multiple_values_as_an_array() {
        let input = fixture();
        assert_eq!(
            apply(".data.total, .meta.count", &input).unwrap(),
            json!([3, 3])
        );
        // `,` 的优先级高于 `|`：`.a | .b, .c` = `.a | (.b, .c)`
        assert_eq!(
            apply(".data | .total, .items[0].id", &input).unwrap(),
            json!([3, "a"])
        );
        // 逗号与展开叠加：每个 item 产出两个值，顺序是交错的
        assert_eq!(
            apply(".data.items[] | .id, .markdown", &input).unwrap(),
            json!(["a", "任务A", "b", "任务B", "c", "任务C"])
        );
        // 单个值仍然不套数组
        assert_eq!(apply(".data.total", &input).unwrap(), json!(3));
    }

    #[test]
    fn comma_and_object_reject_broken_syntax() {
        let input = fixture();
        assert_eq!(apply("{id, ", &input).unwrap_err().code, "JQ_SYNTAX");
        assert_eq!(apply("{.a[0]}", &input).unwrap_err().code, "JQ_SYNTAX");
        assert_eq!(apply("{,id}", &input).unwrap_err().code, "JQ_SYNTAX");
        assert_eq!(apply("{id:}", &input).unwrap_err().code, "JQ_SYNTAX");
        assert_eq!(apply(".data.total,", &input).unwrap(), json!(3), "尾随逗号宽容处理");
    }

    #[test]
    fn syntax_error_hint_inlines_the_supported_subset() {
        let error = apply(".data(", &fixture()).unwrap_err();
        let hint = error.hint.expect("jq 语法错误必须带 hint");
        for token in ["{a, b}", ".a, .b", "map(", "select(", "schema jq"] {
            assert!(hint.contains(token), "hint 里应内联 `{token}`：{hint}");
        }
        // 与 `schema jq` 的支持清单同源：新语法两边都要出现
        let doc = crate::schema::jq_schema().to_string();
        for token in ["{a, b}", ".a, .b"] {
            assert!(doc.contains(token), "JQ_SUBSET_DOC 里应有 `{token}`：{doc}");
        }
    }

    #[test]
    fn existing_subset_still_behaves() {
        let input = fixture();
        assert_eq!(apply(".data.items | length", &input).unwrap(), json!(3));
        assert_eq!(apply(".data.items[-1].id", &input).unwrap(), json!("c"));
        // `.[]` 与 `.a[]` 同义（文档一直这么写，parse_accessors 早先会拒）
        assert_eq!(
            apply(".data.items | .[].id", &input).unwrap(),
            json!(["a", "b", "c"])
        );
        assert_eq!(apply(".data.items | .[0].id", &input).unwrap(), json!("a"));
        assert_eq!(
            apply(".data.items[] | select(.completed == false) | .id", &input).unwrap(),
            json!(["a", "c"])
        );
        assert_eq!(apply(".data.items | keys", &input).unwrap_err().code, "JQ_TYPE");
        assert!(apply("..", &input).is_err(), "递归下降不在子集内");
        assert!(apply(".data.items | sort_by(.id)", &input).is_err());
    }
}
