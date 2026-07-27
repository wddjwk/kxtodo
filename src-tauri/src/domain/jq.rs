//! Built-in jq-compatible subset for `--jq` (§3.2).
//! Supported syntax is listed by `kxtodo schema jq`.

use serde_json::{json, Value};

use crate::domain::error::{CoreError, CoreResult};

pub const JQ_SUBSET_DOC: &str = r#"{
  "syntax": [
    ".                 恒等",
    ".a.b.c           字段路径",
    ".\"key\"         带特殊字符的键",
    ".a[0]            数组下标（支持负数）",
    ".a[] 或 .[]      展开数组（结果为多个值，序列化为数组）",
    "expr | expr       管道",
    "length            数组/字符串长度或对象键数",
    "keys              对象的键数组",
    "first / last      数组首/尾元素",
    "map(expr)         对数组每个元素应用 expr",
    "select(.a == 值)  条件过滤（== / !=，值支持字符串、数字、true/false/null）"
  ],
  "examples": [
    ".data.items",
    ".data.items | length",
    ".data.items[] | .id",
    ".data.items | map(.id)",
    ".data.items[] | select(.completed == false) | .markdown"
  ]
}"#;

#[derive(Debug, Clone)]
enum Stage {
    Access(Vec<Accessor>),
    Length,
    Keys,
    First,
    Last,
    Map(Vec<Stage>),
    Select(PathCondition),
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

pub fn apply(expr: &str, input: &Value) -> CoreResult<Value> {
    let stages = parse_pipeline(expr)?;
    let results = eval_stages(&stages, vec![input.clone()])?;
    // Multiple values (from iteration) are serialized as an array; a single
    // value is returned as-is.
    if results.len() == 1 {
        Ok(results.into_iter().next().unwrap_or(Value::Null))
    } else {
        Ok(Value::Array(results))
    }
}

fn parse_pipeline(expr: &str) -> CoreResult<Vec<Stage>> {
    let parts = split_top_level(expr, '|')?;
    parts.iter().map(|part| parse_stage(part.trim())).collect()
}

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
            '(' => {
                depth += 1;
                current.push(ch);
            }
            ')' => {
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
    if let Some(inner) = expr.strip_prefix("select(").and_then(|v| v.strip_suffix(')')) {
        return Ok(Stage::Select(parse_condition(inner)?));
    }
    if expr.starts_with('.') {
        return Ok(Stage::Access(parse_accessors(expr)?));
    }
    Err(bad_syntax(
        expr,
        "不支持的表达式；支持范围见 kxtodo schema jq",
    ))
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
                while index < chars.len()
                    && (chars[index].is_alphanumeric() || chars[index] == '_' || chars[index] == '-')
                {
                    key.push(chars[index]);
                    index += 1;
                }
                if key.is_empty() {
                    if index >= chars.len() {
                        break;
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
                while index < chars.len() && (chars[index].is_ascii_digit() || chars[index] == '-') {
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
        return Ok(Value::String(inner.replace("\\\"", "\"").replace("\\'", "'")));
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
        .with_hint("支持范围见 kxtodo schema jq")
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
