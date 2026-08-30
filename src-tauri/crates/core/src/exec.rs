//! Process execution for script/executable actions (§3.5, §4.5).
//! argv-based spawn, no shell string interpolation.

use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::error::{CoreError, CoreResult};
use crate::model::{Action, Probe, Runtimes, ScriptLanguage, Source};

/// The single implementation of legacy v8 argument splitting (also used by migration).
pub fn split_legacy_arguments(raw: &str) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    for ch in raw.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        match ch {
            '\\' if !in_single => escaped = true,
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            ch if ch.is_whitespace() && !in_single && !in_double => {
                if !current.is_empty() {
                    args.push(current.clone());
                    current.clear();
                }
            }
            _ => current.push(ch),
        }
    }
    if escaped {
        current.push('\\');
    }
    if in_single || in_double {
        return Err("参数包含未闭合的引号".to_string());
    }
    if !current.is_empty() {
        args.push(current);
    }
    Ok(args)
}

#[derive(Debug, Clone, Default)]
pub struct ExecOutput {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
    pub cancelled: bool,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

#[cfg(target_os = "windows")]
fn executable_candidates(name: &str) -> Vec<String> {
    if Path::new(name).extension().is_some() {
        return vec![name.to_string()];
    }
    let mut candidates = vec![name.to_string()];
    let path_ext = std::env::var("PATHEXT").unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".to_string());
    for ext in path_ext.split(';').filter(|value| !value.trim().is_empty()) {
        candidates.push(format!("{name}{}", ext.to_ascii_lowercase()));
    }
    candidates
}

#[cfg(not(target_os = "windows"))]
fn executable_candidates(name: &str) -> Vec<String> {
    vec![name.to_string()]
}

fn env_executable(env_names: &[&str]) -> Option<String> {
    for name in env_names {
        let Ok(raw) = std::env::var(name) else {
            continue;
        };
        let value = raw.trim().to_string();
        if value.is_empty() {
            continue;
        }
        if Path::new(&value).is_file() {
            return Some(value);
        }
    }
    None
}

pub fn find_executable(names: &[&str], env_names: &[&str]) -> String {
    if let Some(value) = env_executable(env_names) {
        return value;
    }
    let Some(paths) = std::env::var_os("PATH") else {
        return String::new();
    };
    for dir in std::env::split_paths(&paths) {
        for name in names {
            for candidate in executable_candidates(name) {
                let path = dir.join(candidate);
                if path.is_file() {
                    return path.to_string_lossy().to_string();
                }
            }
        }
    }
    String::new()
}

pub fn detect_runtimes() -> Runtimes {
    Runtimes {
        python: find_executable(
            &["python", "python3", "py"],
            &["PYTHON", "PYTHON_EXECUTABLE"],
        ),
        node: find_executable(&["node"], &["NODE", "NODE_EXECUTABLE"]),
        pwsh: find_executable(
            &["pwsh", "powershell"],
            &["PWSH", "POWERSHELL", "POWERSHELL_EXECUTABLE"],
        ),
        bash: find_executable(&["bash"], &["BASH", "BASH_EXECUTABLE"]),
        make: find_executable(&["make", "mingw32-make"], &["MAKE", "MAKE_EXECUTABLE"]),
        extra: Default::default(),
    }
}

pub fn runtime_path(runtimes: &Runtimes, key: &str) -> String {
    let configured = match key {
        "python" => runtimes.python.as_str(),
        "node" => runtimes.node.as_str(),
        "pwsh" => runtimes.pwsh.as_str(),
        "bash" => runtimes.bash.as_str(),
        "make" => runtimes.make.as_str(),
        _ => "",
    };
    if !configured.trim().is_empty() {
        return configured.trim().to_string();
    }
    let detected = detect_runtimes();
    match key {
        "python" => detected.python,
        "node" => detected.node,
        "pwsh" => detected.pwsh,
        "bash" => detected.bash,
        "make" => detected.make,
        _ => String::new(),
    }
}

pub struct ExecSpec {
    pub program: String,
    pub args: Vec<String>,
    pub working_directory: Option<String>,
    pub timeout_ms: Option<u64>,
    pub temp_file: Option<PathBuf>,
}

fn temp_makefile(code: &str) -> CoreResult<PathBuf> {
    let dir = std::env::temp_dir().join("kxtodo-make");
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!(
        "Makefile-{}-{}.mk",
        std::process::id(),
        uuid_suffix()
    ));
    fs::write(&path, code)?;
    Ok(path)
}

fn uuid_suffix() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or(0)
}

/// Build the argv spec for a script action (or probe) against configured runtimes.
pub fn build_script_spec(
    language: ScriptLanguage,
    source: &Source,
    args: &[String],
    interpreter: Option<&str>,
    working_directory: Option<&str>,
    timeout_ms: Option<u64>,
    runtimes: &Runtimes,
) -> CoreResult<ExecSpec> {
    let interpreter = interpreter
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| runtime_path(runtimes, language.runtime_key()));
    if interpreter.is_empty() {
        return Err(CoreError::execution(
            "RUNTIME_NOT_FOUND",
            format!(
                "未找到 {} 的解释器，请运行 kxtodo-cli schedule runtime detect 或 runtime set",
                language.as_str()
            ),
        ));
    }
    let mut argv: Vec<String> = Vec::new();
    let mut temp_file = None;
    match source {
        Source::File { path } => {
            if path.trim().is_empty() {
                return Err(CoreError::validation(
                    "SCRIPT_PATH_REQUIRED",
                    "脚本文件路径为空",
                ));
            }
            match language {
                ScriptLanguage::Powershell => {
                    argv.extend([
                        "-NoProfile".to_string(),
                        "-ExecutionPolicy".to_string(),
                        "Bypass".to_string(),
                        "-File".to_string(),
                        path.clone(),
                    ]);
                }
                ScriptLanguage::Makefile => {
                    argv.extend(["-f".to_string(), path.clone()]);
                }
                _ => argv.push(path.clone()),
            }
        }
        Source::Inline { code } => {
            if code.trim().is_empty() {
                return Err(CoreError::validation(
                    "SCRIPT_CODE_REQUIRED",
                    "inline 脚本内容为空",
                ));
            }
            match language {
                ScriptLanguage::Python => argv.extend(["-c".to_string(), code.clone()]),
                ScriptLanguage::Javascript => argv.extend(["-e".to_string(), code.clone()]),
                ScriptLanguage::Powershell => argv.extend([
                    "-NoProfile".to_string(),
                    "-ExecutionPolicy".to_string(),
                    "Bypass".to_string(),
                    "-Command".to_string(),
                    code.clone(),
                ]),
                ScriptLanguage::Bash => argv.extend(["-lc".to_string(), code.clone()]),
                ScriptLanguage::Makefile => {
                    let path = temp_makefile(code)?;
                    argv.extend(["-f".to_string(), path.to_string_lossy().to_string()]);
                    temp_file = Some(path);
                }
            }
        }
    }
    argv.extend(args.iter().cloned());
    Ok(ExecSpec {
        program: interpreter,
        args: argv,
        working_directory: working_directory.map(str::to_string),
        timeout_ms,
        temp_file,
    })
}

pub fn build_action_spec(action: &Action, runtimes: &Runtimes) -> CoreResult<Option<ExecSpec>> {
    match action {
        Action::Notification { .. } => Ok(None),
        Action::Script {
            language,
            source,
            args,
            interpreter,
            working_directory,
            timeout,
            ..
        } => Ok(Some(build_script_spec(
            *language,
            source,
            args,
            interpreter.as_deref(),
            working_directory.as_deref(),
            match timeout {
                Some(raw) => Some(crate::time::parse_duration_ms(raw)?),
                None => None,
            },
            runtimes,
        )?)),
        Action::Executable {
            program,
            args,
            working_directory,
            timeout,
            ..
        } => {
            if program.trim().is_empty() {
                return Err(CoreError::validation(
                    "PROGRAM_REQUIRED",
                    "可执行程序路径为空",
                ));
            }
            Ok(Some(ExecSpec {
                program: program.trim().to_string(),
                args: args.clone(),
                working_directory: working_directory.clone(),
                timeout_ms: match timeout {
                    Some(raw) => Some(crate::time::parse_duration_ms(raw)?),
                    None => None,
                },
                temp_file: None,
            }))
        }
    }
}

pub fn build_probe_spec(probe: &Probe, runtimes: &Runtimes) -> CoreResult<ExecSpec> {
    match probe {
        Probe::Script {
            language,
            source,
            args,
            interpreter,
            working_directory,
            timeout,
        } => build_script_spec(
            *language,
            source,
            args,
            interpreter.as_deref(),
            working_directory.as_deref(),
            match timeout {
                Some(raw) => Some(crate::time::parse_duration_ms(raw)?),
                None => None,
            },
            runtimes,
        ),
        Probe::Executable {
            program,
            args,
            working_directory,
            timeout,
        } => {
            if program.trim().is_empty() {
                return Err(CoreError::validation(
                    "PROGRAM_REQUIRED",
                    "probe 可执行程序路径为空",
                ));
            }
            Ok(ExecSpec {
                program: program.trim().to_string(),
                args: args.clone(),
                working_directory: working_directory.clone(),
                timeout_ms: match timeout {
                    Some(raw) => Some(crate::time::parse_duration_ms(raw)?),
                    None => None,
                },
                temp_file: None,
            })
        }
    }
}

/// Registry of running children so `stop` can kill them (§3.5.5).
#[derive(Default)]
pub struct ProcessRegistry {
    children: Mutex<HashMap<String, Arc<ChildHandle>>>,
}

struct ChildHandle {
    child: Mutex<Child>,
    cancelled: Arc<AtomicBool>,
}

impl ProcessRegistry {
    pub fn stop(&self, task_id: &str) -> bool {
        let handle = self
            .children
            .lock()
            .map(|children| children.get(task_id).cloned())
            .unwrap_or(None);
        if let Some(handle) = handle {
            handle.cancelled.store(true, Ordering::SeqCst);
            if let Ok(mut child) = handle.child.lock() {
                let _ = child.kill();
            }
            true
        } else {
            false
        }
    }

    pub fn is_cancelled(&self, task_id: &str) -> bool {
        self.children
            .lock()
            .map(|children| {
                children
                    .get(task_id)
                    .map(|handle| handle.cancelled.load(Ordering::SeqCst))
                    .unwrap_or(false)
            })
            .unwrap_or(false)
    }

    pub fn stop_all(&self) -> Vec<String> {
        let handles: Vec<(String, Arc<ChildHandle>)> = self
            .children
            .lock()
            .map(|children| {
                children
                    .iter()
                    .map(|(id, handle)| (id.clone(), handle.clone()))
                    .collect()
            })
            .unwrap_or_default();
        for (_, handle) in &handles {
            handle.cancelled.store(true, Ordering::SeqCst);
            if let Ok(mut child) = handle.child.lock() {
                let _ = child.kill();
            }
        }
        handles.into_iter().map(|(id, _)| id).collect()
    }

    pub fn running_ids(&self) -> Vec<String> {
        self.children
            .lock()
            .map(|children| children.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Run to completion with timeout, cancellation and output capture.
    pub fn run(&self, task_id: &str, spec: ExecSpec) -> CoreResult<ExecOutput> {
        self.run_with_cancel(task_id, spec, Arc::new(AtomicBool::new(false)))
    }

    pub fn run_with_cancel(
        &self,
        task_id: &str,
        spec: ExecSpec,
        cancelled: Arc<AtomicBool>,
    ) -> CoreResult<ExecOutput> {
        if cancelled.load(Ordering::SeqCst) {
            return Err(CoreError::execution("RUN_CANCELLED", "任务在启动前已取消"));
        }
        if self
            .children
            .lock()
            .map_err(|error| CoreError::internal(error.to_string()))?
            .contains_key(task_id)
        {
            return Err(CoreError::conflict(
                "PROCESS_ALREADY_RUNNING",
                format!("任务进程 {task_id} 已在运行"),
            ));
        }
        let mut command = Command::new(&spec.program);
        command.args(&spec.args);
        if let Some(dir) = &spec.working_directory {
            let path = PathBuf::from(dir);
            if !path.is_dir() {
                return Err(CoreError::execution(
                    "WORKDIR_NOT_FOUND",
                    format!("工作目录不存在：{dir}"),
                ));
            }
            command.current_dir(path);
        }
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        let mut child = command.spawn().map_err(|error| {
            CoreError::execution(
                "SPAWN_FAILED",
                format!("无法启动 {}：{error}", spec.program),
            )
        })?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let stdout_thread = stdout.map(|reader| thread::spawn(move || read_pipe(reader)));
        let stderr_thread = stderr.map(|reader| thread::spawn(move || read_pipe(reader)));

        let handle = Arc::new(ChildHandle {
            child: Mutex::new(child),
            cancelled,
        });
        self.children
            .lock()
            .map_err(|error| CoreError::internal(error.to_string()))?
            .insert(task_id.to_string(), handle.clone());
        // Close the stop-before-registration race: stop may set the shared
        // run-slot token after the pre-spawn check but before registry insert.
        if handle.cancelled.load(Ordering::SeqCst) {
            if let Ok(mut child) = handle.child.lock() {
                let _ = child.kill();
            }
        }

        let deadline = spec
            .timeout_ms
            .map(|ms| Instant::now() + Duration::from_millis(ms));
        let mut timed_out = false;
        let status = loop {
            {
                let mut guard = handle
                    .child
                    .lock()
                    .map_err(|error| CoreError::internal(error.to_string()))?;
                match guard.try_wait() {
                    Ok(Some(status)) => break Ok(status),
                    Ok(None) => {}
                    Err(error) => {
                        break Err(CoreError::execution("WAIT_FAILED", error.to_string()))
                    }
                }
            }
            if let Some(deadline) = deadline {
                if Instant::now() >= deadline && !timed_out {
                    timed_out = true;
                    if let Ok(mut guard) = handle.child.lock() {
                        let _ = guard.kill();
                    }
                    // Next poll iterations reap the killed child.
                }
            }
            thread::sleep(Duration::from_millis(60));
        };

        if let Ok(mut children) = self.children.lock() {
            children.remove(task_id);
        }
        if let Some(path) = &spec.temp_file {
            let _ = fs::remove_file(path);
        }

        let status = status?;
        let stdout = stdout_thread
            .and_then(|handle| handle.join().ok())
            .unwrap_or_default();
        let stderr = stderr_thread
            .and_then(|handle| handle.join().ok())
            .unwrap_or_default();

        Ok(ExecOutput {
            exit_code: status.code(),
            stdout: stdout.text,
            stderr: stderr.text,
            timed_out,
            cancelled: handle.cancelled.load(Ordering::SeqCst),
            stdout_truncated: stdout.truncated,
            stderr_truncated: stderr.truncated,
        })
    }
}

#[derive(Default)]
struct PipeCapture {
    text: String,
    truncated: bool,
}

fn read_pipe<R: Read>(mut reader: R) -> PipeCapture {
    let limit = crate::repo::SCHEDULE_OUTPUT_MAX_BYTES;
    let mut retained = Vec::with_capacity(limit);
    let mut chunk = [0u8; 8192];
    let mut truncated = false;
    loop {
        let read = match reader.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        let remaining = limit.saturating_sub(retained.len());
        let keep = remaining.min(read);
        retained.extend_from_slice(&chunk[..keep]);
        if keep < read {
            truncated = true;
        }
    }
    let text = match std::str::from_utf8(&retained) {
        Ok(text) => text.to_string(),
        Err(_) => {
            let (decoded, _, _) = encoding_rs::GBK.decode(&retained);
            decoded.into_owned()
        }
    };
    let (text, expanded) = crate::history::truncate(&text, limit);
    PipeCapture {
        text,
        truncated: truncated || expanded,
    }
}

#[cfg(test)]
mod tests {
    use super::read_pipe;

    #[test]
    fn pipe_capture_is_bounded_and_drains_input() {
        let bytes = vec![b'x'; crate::repo::SCHEDULE_OUTPUT_MAX_BYTES * 8];
        let capture = read_pipe(std::io::Cursor::new(bytes));
        assert!(capture.truncated);
        assert!(capture.text.len() <= crate::repo::SCHEDULE_OUTPUT_MAX_BYTES);
    }

    #[test]
    fn pipe_capture_preserves_small_output() {
        let capture = read_pipe(std::io::Cursor::new("你好\n".as_bytes()));
        assert!(!capture.truncated);
        assert_eq!(capture.text, "你好\n");
    }
}
