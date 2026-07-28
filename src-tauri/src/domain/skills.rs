//! skills command: SKILL.md 编译期嵌入二进制，单文件分发无外挂依赖（§3.7）。

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::domain::error::{CoreError, CoreResult};

pub const SKILL_NAME: &str = "kxtodo";
pub const SKILL_VERSION: u32 = 1;

/// SKILL 内容在编译期嵌入二进制（与帮助信息一样），不依赖任何外部文件。
const SKILL_CONTENT: &str = include_str!("../../../skills/kxtodo/SKILL.md");

pub fn content(name: &str) -> CoreResult<&'static str> {
    if name != SKILL_NAME {
        return Err(CoreError::not_found(
            "SKILL_NOT_FOUND",
            format!("未知 SKILL `{name}`，当前版本仅发布 `{SKILL_NAME}`"),
        ));
    }
    Ok(SKILL_CONTENT)
}

pub fn persist_path(name: &str, requested: &Path, cwd: &Path) -> CoreResult<PathBuf> {
    content(name)?;
    let requested = crate::domain::ipc::normalize_absolute_path(requested, cwd);
    let is_skills = requested
        .file_name()
        .and_then(|part| part.to_str())
        .map(|part| {
            #[cfg(target_os = "windows")]
            {
                part.eq_ignore_ascii_case("skills")
            }
            #[cfg(not(target_os = "windows"))]
            {
                part == "skills"
            }
        })
        .unwrap_or(false);
    let root = if is_skills {
        requested
    } else {
        requested.join("skills")
    };
    Ok(root.join(name).join("SKILL.md"))
}

pub fn cmd_persist(name: &str, requested: &Path, cwd: &Path, dry_run: bool) -> CoreResult<Value> {
    let target = persist_path(name, requested, cwd)?;
    if !dry_run {
        crate::domain::repo::atomic_write(&target, content(name)?)?;
    }
    Ok(json!({
        "name": name,
        "path": target.display().to_string(),
        "source": "embedded",
        "dryRun": dry_run,
    }))
}

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
    let content = content(name)?;
    let meta = parse_frontmatter(content).ok();
    Ok(json!({
        "name": SKILL_NAME,
        "source": "embedded",
        "version": meta.and_then(|m| m.version),
        "content": content,
    }))
}

pub fn cmd_path() -> CoreResult<Value> {
    Ok(json!({
        "source": "embedded",
        "note": "SKILL 内容已编译期嵌入二进制；使用 skills persist 可按需落地外部副本",
    }))
}

/// Validate the embedded SKILL against the current CLI command tree (§3.7).
pub fn cmd_validate(commands: &[String], flags: &[String]) -> CoreResult<Value> {
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
    // Commands referenced anywhere in prose/code spans must exist in the
    // current CLI catalog. Only command domains are recognized, avoiding
    // accidental matches on ordinary prose words.
    let command_regex = regex::Regex::new(
        r"(?:kxtodo\s+)?(task|schedule|config|skills|doctor|notify|schema|version)(?:\s+([a-z][a-z-]*))?",
    )
    .expect("static command regex");
    let mut referenced = Vec::new();
    for capture in command_regex.captures_iter(SKILL_CONTENT) {
        let domain = &capture[1];
        let command = if matches!(domain, "task" | "schedule" | "config" | "skills") {
            capture
                .get(2)
                .map(|action| format!("{domain}.{}", action.as_str()))
                .unwrap_or_else(|| domain.to_string())
        } else {
            domain.to_string()
        };
        referenced.push(command);
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
    let flag_regex = regex::Regex::new(r"--([a-z][a-z0-9-]*)").expect("static flag regex");
    let mut referenced_flags: Vec<String> = flag_regex
        .captures_iter(SKILL_CONTENT)
        .map(|capture| capture[1].to_string())
        .collect();
    referenced_flags.sort();
    referenced_flags.dedup();
    let unknown_flags: Vec<String> = referenced_flags
        .iter()
        .filter(|flag| !flags.iter().any(|known| known == *flag))
        .cloned()
        .collect();
    if !unknown_flags.is_empty() {
        issues.push(json!({
            "severity": "error",
            "message": format!("SKILL 引用了当前 CLI 不支持的参数：--{}", unknown_flags.join(", --")),
            "flags": unknown_flags,
        }));
    }
    let valid = !issues
        .iter()
        .any(|issue| issue.get("severity").and_then(Value::as_str) == Some("error"));
    Ok(json!({
        "valid": valid,
        "source": "embedded",
        "referencedCommands": referenced,
        "referencedFlags": referenced_flags,
        "issues": issues,
    }))
}
