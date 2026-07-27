//! skills command: SKILL.md 编译期嵌入二进制，单文件分发无外挂依赖（§3.7）。

use serde_json::{json, Value};

use crate::domain::error::{CoreError, CoreResult};

pub const SKILL_NAME: &str = "kxtodo";
pub const SKILL_VERSION: u32 = 1;

/// SKILL 内容在编译期嵌入二进制（与帮助信息一样），不依赖任何外部文件。
const SKILL_CONTENT: &str = include_str!("../../../skills/kxtodo/SKILL.md");

#[derive(Debug, Clone)]
pub struct SkillMeta {
    pub name: String,
    pub version: Option<u32>,
    pub cli_help: Option<String>,
}

pub fn parse_frontmatter(content: &str) -> CoreResult<SkillMeta> {
    let mut lines = content.lines();
    if lines.next().map(str::trim) != Some("---") {
        return Err(CoreError::validation(
            "SKILL_FRONTMATTER_MISSING",
            "SKILL.md 缺少 YAML frontmatter（--- 开头）",
        ));
    }
    let mut name = String::new();
    let mut version = None;
    let mut cli_help = None;
    for line in lines {
        let line = line.trim_end();
        if line.trim() == "---" {
            break;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim().trim_matches('"').trim_matches('\'');
        match key {
            "name" => name = value.to_string(),
            "version" => version = value.parse::<u32>().ok(),
            "cliHelp" => cli_help = Some(value.to_string()),
            _ => {}
        }
    }
    if name.is_empty() {
        return Err(CoreError::validation(
            "SKILL_FRONTMATTER_INVALID",
            "SKILL.md frontmatter 缺少 name",
        ));
    }
    Ok(SkillMeta {
        name,
        version,
        cli_help,
    })
}

pub fn cmd_list() -> CoreResult<Value> {
    let meta = parse_frontmatter(SKILL_CONTENT).ok();
    Ok(json!({
        "skills": [{
            "name": meta.as_ref().map(|m| m.name.clone()).unwrap_or_else(|| SKILL_NAME.to_string()),
            "version": meta.and_then(|m| m.version),
            "source": "embedded",
            "available": true,
        }]
    }))
}

pub fn cmd_read(name: &str) -> CoreResult<Value> {
    if name != SKILL_NAME {
        return Err(CoreError::not_found(
            "SKILL_NOT_FOUND",
            format!("未知 SKILL `{name}`，当前版本仅发布 `{SKILL_NAME}`"),
        ));
    }
    let meta = parse_frontmatter(SKILL_CONTENT).ok();
    Ok(json!({
        "name": SKILL_NAME,
        "source": "embedded",
        "version": meta.and_then(|m| m.version),
        "content": SKILL_CONTENT,
    }))
}

pub fn cmd_path() -> CoreResult<Value> {
    Ok(json!({
        "source": "embedded",
        "note": "SKILL 内容已编译期嵌入二进制，无外部文件路径",
    }))
}

/// Validate the embedded SKILL against the current CLI command tree (§3.7).
pub fn cmd_validate(commands: &[String]) -> CoreResult<Value> {
    let mut issues = Vec::new();
    let meta = match parse_frontmatter(SKILL_CONTENT) {
        Ok(meta) => meta,
        Err(error) => {
            issues.push(json!({ "severity": "error", "message": error.message }));
            return Ok(json!({ "valid": false, "source": "embedded", "issues": issues }));
        }
    };
    if meta.name != SKILL_NAME {
        issues.push(json!({
            "severity": "error",
            "message": format!("frontmatter name 应为 {SKILL_NAME}，实际为 {}", meta.name),
        }));
    }
    match meta.version {
        Some(version) if version <= SKILL_VERSION => {}
        Some(version) => issues.push(json!({
            "severity": "error",
            "message": format!("SKILL 版本 {version} 高于 CLI 支持的 {SKILL_VERSION}"),
        })),
        None => issues.push(json!({
            "severity": "error",
            "message": "frontmatter 缺少 version",
        })),
    }
    if meta.cli_help.is_none() {
        issues.push(json!({
            "severity": "warning",
            "message": "frontmatter 缺少 cliHelp（建议指向 kxtodo --help）",
        }));
    }
    // Commands referenced in the SKILL must exist in the current CLI catalog.
    let mut referenced = Vec::new();
    for line in SKILL_CONTENT.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("kxtodo ") {
            continue;
        }
        let tokens: Vec<&str> = trimmed
            .trim_start_matches("kxtodo ")
            .split_whitespace()
            .take_while(|token| {
                !token.starts_with('-')
                    && !token.starts_with('"')
                    && !token.starts_with('\'')
                    && !token.contains('=')
            })
            .collect();
        let mut dotted = String::new();
        for token in tokens {
            if token.chars().all(|ch| ch.is_ascii_lowercase() || ch == '-') {
                if !dotted.is_empty() {
                    dotted.push('.');
                }
                dotted.push_str(token);
            } else {
                break;
            }
        }
        if !dotted.is_empty() {
            referenced.push(dotted);
        }
    }
    referenced.sort();
    referenced.dedup();
    let mut unknown = Vec::new();
    for command in &referenced {
        if !commands
            .iter()
            .any(|known| known == command || command.starts_with(&format!("{known}.")))
        {
            unknown.push(command.clone());
        }
    }
    if !unknown.is_empty() {
        issues.push(json!({
            "severity": "error",
            "message": format!("SKILL 引用了当前 CLI 不支持的命令：{}", unknown.join(", ")),
            "commands": unknown,
        }));
    }
    let valid = !issues
        .iter()
        .any(|issue| issue.get("severity").and_then(Value::as_str) == Some("error"));
    Ok(json!({
        "valid": valid,
        "source": "embedded",
        "referencedCommands": referenced,
        "issues": issues,
    }))
}
