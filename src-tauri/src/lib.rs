use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    env,
    fs,
    path::PathBuf,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};
use tauri::{AppHandle, Manager};

#[cfg(desktop)]
use std::process::Command;
#[cfg(desktop)]
use tauri::{
    image::Image,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    State,
};
#[cfg(desktop)]
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
#[cfg(desktop)]
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

const DEFAULT_UI_SCALE: f64 = 0.75;
const IMG_DIR: &str = "img";
const AVATAR_DIR: &str = "avator";
const BACKGROUND_DIR: &str = "background";
const ENTRY_IMAGE_DIR: &str = "data";

// Fields are only read by desktop-only tray / window-close handling.
#[cfg_attr(not(desktop), allow(dead_code))]
struct LifecycleState {
    close_to_tray: AtomicBool,
    quitting: AtomicBool,
}

impl Default for LifecycleState {
    fn default() -> Self {
        Self {
            close_to_tray: AtomicBool::new(true),
            quitting: AtomicBool::new(false),
        }
    }
}

fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    #[cfg(desktop)]
    {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                return Ok(dir.join("todo-note-data"));
            }
        }
    }

    app.path().app_data_dir().map_err(|error| error.to_string())
}

fn ensure_data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = data_dir(app)?;
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(dir)
}

fn move_dir_contents(src: PathBuf, dest: PathBuf) -> Result<(), String> {
    if !src.exists() {
        return Ok(());
    }
    fs::create_dir_all(&dest).map_err(|error| error.to_string())?;
    for entry in fs::read_dir(&src).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        if src_path.is_dir() {
            move_dir_contents(src_path.clone(), dest_path)?;
            let _ = fs::remove_dir(&src_path);
        } else if !dest_path.exists() {
            fs::rename(&src_path, &dest_path)
                .or_else(|_| fs::copy(&src_path, &dest_path).map(|_| ()))
                .map_err(|error| error.to_string())?;
            let _ = fs::remove_file(&src_path);
        }
    }
    let _ = fs::remove_dir(&src);
    Ok(())
}

fn ensure_storage_layout(app: &AppHandle) -> Result<PathBuf, String> {
    let root = ensure_data_dir(app)?;
    fs::create_dir_all(root.join(IMG_DIR).join(AVATAR_DIR)).map_err(|error| error.to_string())?;
    fs::create_dir_all(root.join(IMG_DIR).join(BACKGROUND_DIR))
        .map_err(|error| error.to_string())?;
    fs::create_dir_all(root.join(IMG_DIR).join(ENTRY_IMAGE_DIR))
        .map_err(|error| error.to_string())?;

    move_dir_contents(root.join("images"), root.join(IMG_DIR).join(BACKGROUND_DIR))?;
    move_dir_contents(root.join("avatar"), root.join(IMG_DIR).join(AVATAR_DIR))?;
    move_dir_contents(
        root.join(IMG_DIR).join("avatar"),
        root.join(IMG_DIR).join(AVATAR_DIR),
    )?;

    let legacy_img = root.join(IMG_DIR);
    if legacy_img.is_dir() {
        for entry in fs::read_dir(&legacy_img).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let name = entry.file_name().to_string_lossy().to_string();
            if matches!(
                name.as_str(),
                AVATAR_DIR | BACKGROUND_DIR | ENTRY_IMAGE_DIR | "avatar"
            ) {
                continue;
            }
            let path = entry.path();
            if path.is_dir() {
                move_dir_contents(path, root.join(IMG_DIR).join(ENTRY_IMAGE_DIR).join(name))?;
            }
        }
    }

    Ok(root)
}

fn images_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = ensure_storage_layout(app)?
        .join(IMG_DIR)
        .join(BACKGROUND_DIR);
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(dir)
}

fn avatar_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = ensure_storage_layout(app)?.join(IMG_DIR).join(AVATAR_DIR);
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(dir)
}

fn md_images_dir(app: &AppHandle, node_id: &str) -> Result<PathBuf, String> {
    if node_id.is_empty()
        || node_id.contains('/')
        || node_id.contains('\\')
        || node_id.contains("..")
    {
        return Err("Invalid node id".to_string());
    }
    let dir = ensure_storage_layout(app)?
        .join(IMG_DIR)
        .join(ENTRY_IMAGE_DIR)
        .join(node_id);
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(dir)
}

fn data_file(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(data_dir(app)?.join("data.json"))
}

fn settings_file(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(data_dir(app)?.join("settings.json"))
}

fn scheduler_file(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(data_dir(app)?.join("tasks.json"))
}

fn ensure_parent(path: &PathBuf) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn read_json(path: PathBuf) -> Result<Value, String> {
    if !path.exists() {
        return Ok(json!(null));
    }

    let raw = fs::read_to_string(path).map_err(|error| error.to_string())?;
    serde_json::from_str(&raw).map_err(|error| error.to_string())
}

fn write_json(path: PathBuf, value: Value) -> Result<(), String> {
    ensure_parent(&path)?;
    let raw = serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?;
    fs::write(path, raw).map_err(|error| error.to_string())
}

fn normalize_ui_scale(scale: Option<f64>) -> f64 {
    let raw = scale.unwrap_or(DEFAULT_UI_SCALE);
    if (raw - 0.62).abs() < 0.001
        || (raw - 0.72).abs() < 0.001
        || (raw - 0.86).abs() < 0.001
        || (raw - 0.92).abs() < 0.001
    {
        return DEFAULT_UI_SCALE;
    }
    raw.clamp(0.5, 1.5)
}

#[tauri::command]
fn load_state(app: AppHandle) -> Result<Value, String> {
    read_json(data_file(&app)?)
}

#[tauri::command]
fn save_state(app: AppHandle, state: Value) -> Result<(), String> {
    write_json(data_file(&app)?, state)
}

#[tauri::command]
fn load_settings(app: AppHandle) -> Result<Value, String> {
    read_json(settings_file(&app)?)
}

#[tauri::command]
fn save_settings(app: AppHandle, settings: Value) -> Result<(), String> {
    write_json(settings_file(&app)?, settings)
}

#[tauri::command]
fn load_scheduler(app: AppHandle) -> Result<Value, String> {
    read_json(scheduler_file(&app)?)
}

#[tauri::command]
fn save_scheduler(app: AppHandle, scheduler: Value) -> Result<(), String> {
    write_json(scheduler_file(&app)?, scheduler)
}

#[cfg_attr(not(desktop), allow(dead_code))]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScheduledActionCommand {
    #[serde(default, rename = "type")]
    action_type: String,
    #[serde(default)]
    script_mode: String,
    #[serde(default)]
    language: String,
    #[serde(default)]
    interpreter: String,
    #[serde(default)]
    file_path: String,
    #[serde(default)]
    code: String,
    #[serde(default)]
    executable_path: String,
    #[serde(default)]
    arguments: String,
    #[serde(default)]
    working_directory: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScheduledActionOutput {
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
}

#[cfg(target_os = "windows")]
fn executable_candidates(name: &str) -> Vec<String> {
    if std::path::Path::new(name).extension().is_some() {
        return vec![name.to_string()];
    }
    let mut candidates = vec![name.to_string()];
    let path_ext = env::var("PATHEXT").unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".to_string());
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
        let Ok(raw) = env::var(name) else {
            continue;
        };
        let value = raw.trim().to_string();
        if value.is_empty() {
            continue;
        }
        let path = PathBuf::from(&value);
        if path.is_file() {
            return Some(value);
        }
    }
    None
}

fn find_executable(names: &[&str], env_names: &[&str]) -> String {
    if let Some(value) = env_executable(env_names) {
        return value;
    }

    let Some(paths) = env::var_os("PATH") else {
        return String::new();
    };
    for dir in env::split_paths(&paths) {
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

fn default_executor_paths() -> HashMap<String, String> {
    let mut paths = HashMap::new();
    paths.insert(
        "python".to_string(),
        find_executable(&["python", "python3", "py"], &["PYTHON", "PYTHON_EXECUTABLE"]),
    );
    paths.insert(
        "node".to_string(),
        find_executable(&["node"], &["NODE", "NODE_EXECUTABLE"]),
    );
    paths.insert(
        "pwsh".to_string(),
        find_executable(
            &["pwsh", "powershell"],
            &["PWSH", "POWERSHELL", "POWERSHELL_EXECUTABLE"],
        ),
    );
    paths.insert(
        "bash".to_string(),
        find_executable(&["bash"], &["BASH", "BASH_EXECUTABLE"]),
    );
    paths.insert(
        "make".to_string(),
        find_executable(&["make", "mingw32-make"], &["MAKE", "MAKE_EXECUTABLE"]),
    );
    paths
}

#[tauri::command]
fn resolve_executor_paths() -> HashMap<String, String> {
    default_executor_paths()
}

#[cfg(desktop)]
fn split_arguments(raw: &str) -> Result<Vec<String>, String> {
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
        return Err("Arguments contain an unclosed quote".to_string());
    }
    if !current.is_empty() {
        args.push(current);
    }
    Ok(args)
}

#[cfg(desktop)]
fn runtime_key_for_language(language: &str) -> Option<&'static str> {
    match language {
        "python" => Some("python"),
        "javascript" => Some("node"),
        "powershell" => Some("pwsh"),
        "bash" => Some("bash"),
        "makefile" => Some("make"),
        _ => None,
    }
}

#[cfg(desktop)]
fn configured_interpreter(
    action: &ScheduledActionCommand,
    runtimes: &HashMap<String, String>,
) -> String {
    let custom = action.interpreter.trim();
    if !custom.is_empty() {
        return custom.to_string();
    }
    let Some(key) = runtime_key_for_language(action.language.as_str()) else {
        return String::new();
    };
    let configured = runtimes.get(key).map(|value| value.trim()).unwrap_or("");
    if !configured.is_empty() {
        return configured.to_string();
    }
    default_executor_paths()
        .get(key)
        .cloned()
        .unwrap_or_default()
}

#[cfg(desktop)]
fn temp_makefile(app: &AppHandle, code: &str) -> Result<PathBuf, String> {
    let dir = ensure_storage_layout(app)?.join("scheduler-temp");
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or(0);
    let counter = IMAGE_COUNTER.fetch_add(1, Ordering::SeqCst);
    let path = dir.join(format!("Makefile-{stamp}-{counter}.mk"));
    fs::write(&path, code).map_err(|error| error.to_string())?;
    Ok(path)
}

#[cfg(desktop)]
fn build_scheduled_command(
    app: &AppHandle,
    action: &ScheduledActionCommand,
    runtimes: &HashMap<String, String>,
) -> Result<(Command, Option<PathBuf>), String> {
    let mut temp_file = None;
    let action_args = split_arguments(action.arguments.trim())?;
    let mut command = if action.action_type == "executable" {
        let program = action.executable_path.trim();
        if program.is_empty() {
            return Err("Executable path is required".to_string());
        }
        let mut command = Command::new(program);
        command.args(action_args);
        command
    } else {
        let interpreter = configured_interpreter(action, runtimes);
        if interpreter.trim().is_empty() {
            return Err(format!("Interpreter for {} was not found", action.language));
        }
        let mut command = Command::new(interpreter.trim());
        if action.script_mode == "path" {
            let file_path = action.file_path.trim();
            if file_path.is_empty() {
                return Err("Script file path is required".to_string());
            }
            match action.language.as_str() {
                "powershell" => {
                    command.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", file_path]);
                }
                "makefile" => {
                    command.args(["-f", file_path]);
                }
                _ => {
                    command.arg(file_path);
                }
            }
        } else {
            let code = action.code.as_str();
            if code.trim().is_empty() {
                return Err("Inline script code is required".to_string());
            }
            match action.language.as_str() {
                "python" => {
                    command.args(["-c", code]);
                }
                "javascript" => {
                    command.args(["-e", code]);
                }
                "powershell" => {
                    command.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", code]);
                }
                "bash" => {
                    command.args(["-lc", code]);
                }
                "makefile" => {
                    let path = temp_makefile(app, code)?;
                    command.arg("-f").arg(&path);
                    temp_file = Some(path);
                }
                _ => {
                    command.args(["-c", code]);
                }
            }
        }
        command.args(action_args);
        command
    };

    let cwd = action.working_directory.trim();
    if !cwd.is_empty() {
        let path = PathBuf::from(cwd);
        if !path.is_dir() {
            return Err("Working directory does not exist".to_string());
        }
        command.current_dir(path);
    }

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }

    Ok((command, temp_file))
}

#[cfg(desktop)]
#[tauri::command]
async fn run_scheduled_action(
    app: AppHandle,
    action: ScheduledActionCommand,
    runtimes: HashMap<String, String>,
) -> Result<ScheduledActionOutput, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (mut command, temp_file) = build_scheduled_command(&app, &action, &runtimes)?;
        let output = command.output().map_err(|error| error.to_string());
        if let Some(path) = temp_file {
            let _ = fs::remove_file(path);
        }
        let output = output?;
        Ok(ScheduledActionOutput {
            exit_code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(not(desktop))]
#[tauri::command]
async fn run_scheduled_action(
    _action: ScheduledActionCommand,
    _runtimes: HashMap<String, String>,
) -> Result<ScheduledActionOutput, String> {
    Err("Scheduled task execution is not supported on mobile".to_string())
}

#[tauri::command]
fn export_data(payload: Value, path: String) -> Result<(), String> {
    let output_path = PathBuf::from(path);
    write_json(output_path, payload)
}

static IMAGE_COUNTER: AtomicU64 = AtomicU64::new(0);

fn safe_image_name(filename: &str) -> Result<(), String> {
    if filename.is_empty()
        || filename.contains('/')
        || filename.contains('\\')
        || filename.contains("..")
    {
        return Err("Invalid image filename".to_string());
    }
    Ok(())
}

fn extension_for_mime(meta: &str) -> &'static str {
    if meta.contains("image/jpeg") || meta.contains("image/jpg") {
        "jpg"
    } else if meta.contains("image/gif") {
        "gif"
    } else if meta.contains("image/webp") {
        "webp"
    } else if meta.contains("image/bmp") {
        "bmp"
    } else if meta.contains("image/svg") {
        "svg"
    } else {
        "png"
    }
}

fn mime_for_extension(ext: &str) -> &'static str {
    match ext {
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        _ => "image/png",
    }
}

#[tauri::command]
fn save_background_image(app: AppHandle, data_url: String) -> Result<String, String> {
    let (meta, payload) = data_url
        .split_once(',')
        .ok_or_else(|| "Invalid image data".to_string())?;
    let ext = extension_for_mime(meta);
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(payload.trim().as_bytes())
        .map_err(|error| error.to_string())?;
    let dir = images_dir(&app)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or(0);
    let counter = IMAGE_COUNTER.fetch_add(1, Ordering::SeqCst);
    let filename = format!("bg-{stamp}-{counter}.{ext}");
    fs::write(dir.join(&filename), bytes).map_err(|error| error.to_string())?;
    Ok(filename)
}

#[tauri::command]
fn load_background_image(app: AppHandle, filename: String) -> Result<String, String> {
    safe_image_name(&filename)?;
    let path = images_dir(&app)?.join(&filename);
    let bytes = fs::read(&path).map_err(|error| error.to_string())?;
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("png")
        .to_lowercase();
    let mime = mime_for_extension(&ext);
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    Ok(format!("data:{mime};base64,{encoded}"))
}

#[tauri::command]
fn delete_background_image(app: AppHandle, filename: String) -> Result<(), String> {
    safe_image_name(&filename)?;
    let path = images_dir(&app)?.join(&filename);
    let _ = fs::remove_file(path);
    Ok(())
}

fn extension_for_path(path: &str) -> String {
    std::path::Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_lowercase())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "png".to_string())
}

/// Copy a picked image file into the images dir without any base64 round-trip.
/// Returns the stored filename. Works for arbitrarily large images.
#[tauri::command]
fn import_background_image(app: AppHandle, src_path: String) -> Result<String, String> {
    let src = std::path::Path::new(&src_path);
    if !src.is_file() {
        return Err("File not found".to_string());
    }
    let ext = extension_for_path(&src_path);
    let dir = images_dir(&app)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or(0);
    let counter = IMAGE_COUNTER.fetch_add(1, Ordering::SeqCst);
    let filename = format!("bg-{stamp}-{counter}.{ext}");
    fs::copy(src, dir.join(&filename)).map_err(|error| error.to_string())?;
    Ok(filename)
}

/// Absolute filesystem path of a stored background image, for `convertFileSrc`.
#[tauri::command]
fn background_image_path(app: AppHandle, filename: String) -> Result<String, String> {
    safe_image_name(&filename)?;
    let path = images_dir(&app)?.join(&filename);
    if !path.is_file() {
        return Err("File not found".to_string());
    }
    Ok(path.to_string_lossy().to_string())
}

/// Copy a picked image into the avatar directory. Returns stored filename.
#[tauri::command]
fn save_avatar_image(app: AppHandle, src_path: String) -> Result<String, String> {
    let src = std::path::Path::new(&src_path);
    if !src.is_file() {
        return Err("File not found".to_string());
    }
    let ext = extension_for_path(&src_path);
    let dir = avatar_dir(&app)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or(0);
    let filename = format!("avatar-{stamp}.{ext}");
    for entry in fs::read_dir(&dir).into_iter().flatten() {
        if let Ok(entry) = entry {
            let _ = fs::remove_file(entry.path());
        }
    }
    fs::copy(src, dir.join(&filename)).map_err(|error| error.to_string())?;
    Ok(filename)
}

/// Delete the avatar image file.
#[tauri::command]
fn delete_avatar_image(app: AppHandle, filename: String) -> Result<(), String> {
    safe_image_name(&filename)?;
    let path = avatar_dir(&app)?.join(&filename);
    let _ = fs::remove_file(path);
    Ok(())
}

/// Absolute path of a stored avatar image, for `convertFileSrc`.
#[tauri::command]
fn avatar_image_path(app: AppHandle, filename: String) -> Result<String, String> {
    safe_image_name(&filename)?;
    let path = avatar_dir(&app)?.join(&filename);
    if !path.is_file() {
        return Err("File not found".to_string());
    }
    Ok(path.to_string_lossy().to_string())
}

/// Copy a picked image into img/<node_id>/ for markdown embedding. Returns stored filename.
#[tauri::command]
fn save_md_image(app: AppHandle, src_path: String, node_id: String) -> Result<String, String> {
    let src = std::path::Path::new(&src_path);
    if !src.is_file() {
        return Err("File not found".to_string());
    }
    let ext = extension_for_path(&src_path);
    let dir = md_images_dir(&app, &node_id)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or(0);
    let counter = IMAGE_COUNTER.fetch_add(1, Ordering::SeqCst);
    let filename = format!("md-{stamp}-{counter}.{ext}");
    fs::copy(src, dir.join(&filename)).map_err(|error| error.to_string())?;
    Ok(filename)
}

/// Delete a single markdown image file.
#[tauri::command]
fn delete_md_image(app: AppHandle, node_id: String, filename: String) -> Result<(), String> {
    safe_image_name(&filename)?;
    let path = md_images_dir(&app, &node_id)?.join(&filename);
    let _ = fs::remove_file(path);
    Ok(())
}

/// Delete all markdown images for a node (called when entry is deleted).
#[tauri::command]
fn delete_node_images(app: AppHandle, node_id: String) -> Result<(), String> {
    if node_id.is_empty()
        || node_id.contains('/')
        || node_id.contains('\\')
        || node_id.contains("..")
    {
        return Err("Invalid node id".to_string());
    }
    let dir = ensure_storage_layout(&app)?
        .join(IMG_DIR)
        .join(ENTRY_IMAGE_DIR)
        .join(&node_id);
    if dir.exists() {
        let _ = fs::remove_dir_all(&dir);
    }
    Ok(())
}

/// Absolute path of a stored markdown image, for `convertFileSrc`.
#[tauri::command]
fn md_image_path(app: AppHandle, node_id: String, filename: String) -> Result<String, String> {
    safe_image_name(&filename)?;
    let path = md_images_dir(&app, &node_id)?.join(&filename);
    if !path.is_file() {
        return Err("File not found".to_string());
    }
    Ok(path.to_string_lossy().to_string())
}

/// Save a base64 data URL as a markdown image in img/<node_id>/. Returns stored filename.
#[tauri::command]
fn save_md_image_data(app: AppHandle, data_url: String, node_id: String) -> Result<String, String> {
    let (meta, payload) = data_url
        .split_once(',')
        .ok_or_else(|| "Invalid image data".to_string())?;
    let ext = extension_for_mime(meta);
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(payload.trim().as_bytes())
        .map_err(|error| error.to_string())?;
    let dir = md_images_dir(&app, &node_id)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or(0);
    let counter = IMAGE_COUNTER.fetch_add(1, Ordering::SeqCst);
    let filename = format!("md-{stamp}-{counter}.{ext}");
    fs::write(dir.join(&filename), bytes).map_err(|error| error.to_string())?;
    Ok(filename)
}

#[cfg(desktop)]
fn shortcut_from_string(raw: &str) -> Result<Shortcut, String> {
    let mut modifiers = Modifiers::empty();
    let mut code: Option<Code> = None;

    for part in raw.split('+') {
        match part.trim().to_lowercase().as_str() {
            "ctrl" | "control" => modifiers |= Modifiers::CONTROL,
            "shift" => modifiers |= Modifiers::SHIFT,
            "alt" | "option" => modifiers |= Modifiers::ALT,
            "super" | "meta" | "win" | "cmd" => modifiers |= Modifiers::SUPER,
            "space" => code = Some(Code::Space),
            "enter" => code = Some(Code::Enter),
            "n" => code = Some(Code::KeyN),
            "l" => code = Some(Code::KeyL),
            "f" => code = Some(Code::KeyF),
            "t" => code = Some(Code::KeyT),
            "m" => code = Some(Code::KeyM),
            "d" => code = Some(Code::KeyD),
            "e" => code = Some(Code::KeyE),
            key => return Err(format!("Unsupported shortcut key: {key}")),
        }
    }

    code.map(|key| Shortcut::new(Some(modifiers), key))
        .ok_or_else(|| "Shortcut must include a key".to_string())
}

#[cfg(desktop)]
fn register_global_toggle(app: &AppHandle, raw: &str) -> Result<(), String> {
    let shortcut = shortcut_from_string(raw)?;
    app.global_shortcut()
        .unregister_all()
        .map_err(|error| error.to_string())?;

    let app_handle = app.clone();
    app.global_shortcut()
        .on_shortcut(shortcut, move |_app, _shortcut, event| {
            if event.state() != ShortcutState::Pressed {
                return;
            }

            if let Some(window) = app_handle.get_webview_window("main") {
                if window.is_visible().unwrap_or(true) {
                    let _ = window.hide();
                } else {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        })
        .map_err(|error| error.to_string())
}

#[cfg(desktop)]
fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

#[cfg(desktop)]
fn fallback_tray_icon() -> Image<'static> {
    let mut rgba = vec![0_u8; 32 * 32 * 4];
    for y in 0..32 {
        for x in 0..32 {
            let index = (y * 32 + x) * 4;
            rgba[index] = 37;
            rgba[index + 1] = 100;
            rgba[index + 2] = 207;
            rgba[index + 3] = 255;
            if (10..=22).contains(&x) && (14..=18).contains(&y) && x >= y - 2 {
                rgba[index] = 255;
                rgba[index + 1] = 255;
                rgba[index + 2] = 255;
            }
        }
    }
    Image::new_owned(rgba, 32, 32)
}

#[cfg(desktop)]
fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "tray_open", "打开 KXToDo", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "tray_quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let icon = app
        .default_window_icon()
        .cloned()
        .map(Image::to_owned)
        .unwrap_or_else(fallback_tray_icon);

    TrayIconBuilder::with_id("main-tray")
        .tooltip("KXToDo")
        .icon(icon)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "tray_open" => show_main_window(app),
            "tray_quit" => {
                app.state::<LifecycleState>()
                    .quitting
                    .store(true, Ordering::SeqCst);
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| match event {
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            }
            | TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } => show_main_window(tray.app_handle()),
            _ => {}
        })
        .build(app)?;

    Ok(())
}

#[cfg(desktop)]
#[tauri::command]
fn register_global_shortcut(app: AppHandle, shortcut: String) -> Result<(), String> {
    register_global_toggle(&app, &shortcut)
}

#[cfg(not(desktop))]
#[tauri::command]
fn register_global_shortcut(_shortcut: String) -> Result<(), String> {
    Ok(())
}

#[cfg(desktop)]
#[tauri::command]
fn set_close_to_tray(state: State<LifecycleState>, enabled: bool) {
    state.close_to_tray.store(enabled, Ordering::SeqCst);
}

#[cfg(not(desktop))]
#[tauri::command]
fn set_close_to_tray(_enabled: bool) {}

#[cfg(desktop)]
#[tauri::command]
fn set_autostart(app: AppHandle, enabled: bool) -> Result<(), String> {
    if enabled {
        app.autolaunch().enable().map_err(|error| error.to_string())
    } else {
        app.autolaunch()
            .disable()
            .map_err(|error| error.to_string())
    }
}

#[cfg(not(desktop))]
#[tauri::command]
fn set_autostart(_enabled: bool) -> Result<(), String> {
    Ok(())
}

#[cfg(desktop)]
#[tauri::command]
fn get_autostart_enabled(app: AppHandle) -> Result<bool, String> {
    app.autolaunch()
        .is_enabled()
        .map_err(|error| error.to_string())
}

#[cfg(not(desktop))]
#[tauri::command]
fn get_autostart_enabled() -> Result<bool, String> {
    Ok(false)
}

#[cfg(desktop)]
#[tauri::command]
fn set_webview_zoom(app: AppHandle, scale: f64) -> Result<(), String> {
    let _ = normalize_ui_scale(Some(scale));
    if let Some(window) = app.get_webview_window("main") {
        window.set_zoom(1.0).map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(not(desktop))]
#[tauri::command]
fn set_webview_zoom(scale: f64) -> Result<(), String> {
    let _ = normalize_ui_scale(Some(scale));
    Ok(())
}

#[cfg(desktop)]
#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    if !(url.starts_with("http://") || url.starts_with("https://") || url.starts_with("mailto:")) {
        return Err("Unsupported link protocol".to_string());
    }

    #[cfg(target_os = "windows")]
    let result = Command::new("rundll32")
        .arg("url.dll,FileProtocolHandler")
        .arg(&url)
        .spawn();

    #[cfg(target_os = "macos")]
    let result = Command::new("open").arg(&url).spawn();

    #[cfg(all(unix, not(target_os = "macos")))]
    let result = Command::new("xdg-open").arg(&url).spawn();

    #[cfg(not(any(target_os = "windows", target_os = "macos", unix)))]
    {
        return Err(
            "Opening links in the system browser is not supported on this platform".to_string(),
        );
    }

    result.map(|_| ()).map_err(|error| error.to_string())
}

#[cfg(not(desktop))]
#[tauri::command]
fn open_url(app: AppHandle, url: String) -> Result<(), String> {
    if !(url.starts_with("http://") || url.starts_with("https://") || url.starts_with("mailto:")) {
        return Err("Unsupported link protocol".to_string());
    }
    tauri_plugin_opener::OpenerExt::opener(&app)
        .open_url(url, None::<&str>)
        .map_err(|error| error.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default().manage(LifecycleState::default());

    #[cfg(desktop)]
    let builder = builder
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app);
        }))
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--from-autostart"]),
        ));

    #[cfg(not(desktop))]
    let builder = builder.plugin(tauri_plugin_opener::init());

    builder
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            load_state,
            save_state,
            load_settings,
            save_settings,
            load_scheduler,
            save_scheduler,
            resolve_executor_paths,
            run_scheduled_action,
            export_data,
            save_background_image,
            load_background_image,
            delete_background_image,
            import_background_image,
            background_image_path,
            save_avatar_image,
            delete_avatar_image,
            avatar_image_path,
            save_md_image,
            delete_md_image,
            delete_node_images,
            md_image_path,
            save_md_image_data,
            register_global_shortcut,
            set_close_to_tray,
            set_autostart,
            get_autostart_enabled,
            set_webview_zoom,
            open_url
        ])
        .setup(|app| {
            ensure_storage_layout(app.handle()).map_err(|error| {
                tauri::Error::Io(std::io::Error::new(std::io::ErrorKind::Other, error))
            })?;
            #[cfg(desktop)]
            {
                if let Some(webview) = app.get_webview_window("main") {
                    let _ = webview.set_zoom(1.0);
                }
                setup_tray(app)?;
                show_main_window(app.handle());
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                #[cfg(desktop)]
                {
                    let lifecycle = window.state::<LifecycleState>();
                    if lifecycle.quitting.load(Ordering::SeqCst)
                        || !lifecycle.close_to_tray.load(Ordering::SeqCst)
                    {
                        lifecycle.quitting.store(true, Ordering::SeqCst);
                        window.app_handle().exit(0);
                        return;
                    }

                    api.prevent_close();
                    let _ = window.hide();
                }
                #[cfg(not(desktop))]
                {
                    let _ = (window, api);
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("failed to run KXToDo");
}
