use base64::Engine;
#[cfg(desktop)]
use std::{
    io::{Read, Write},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};
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
use tauri::{
    image::Image,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    LogicalPosition, WebviewUrl, WebviewWindowBuilder,
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
const DEFAULT_NOTIFICATION_DURATION_MS: u64 = 5_200;

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

#[cfg(desktop)]
#[derive(Default)]
struct SchedulerProcessState {
    children: Mutex<HashMap<String, Arc<Mutex<Child>>>>,
}

#[derive(Debug, Clone, Copy)]
enum NotificationPosition {
    BottomRight,
    TopRight,
    BottomLeft,
    TopLeft,
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

fn default_notification_title() -> String {
    "KXToDo".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NotificationRequest {
    #[serde(default = "default_notification_title")]
    title: String,
    #[serde(default)]
    message: String,
    #[serde(default)]
    duration_ms: u64,
    #[serde(default)]
    tone: String,
    #[serde(default)]
    position: String,
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

fn clamp_notification_duration(duration_ms: u64, fallback: u64) -> u64 {
    let raw = if duration_ms == 0 {
        fallback
    } else {
        duration_ms
    };
    raw.clamp(1_200, 60_000)
}

fn default_notification_duration(app: &AppHandle) -> u64 {
    let Ok(path) = settings_file(app) else {
        return DEFAULT_NOTIFICATION_DURATION_MS;
    };
    let Ok(value) = read_json(path) else {
        return DEFAULT_NOTIFICATION_DURATION_MS;
    };
    value
        .get("notifications")
        .and_then(|item| item.get("durationMs"))
        .and_then(Value::as_u64)
        .map(|duration| clamp_notification_duration(duration, DEFAULT_NOTIFICATION_DURATION_MS))
        .unwrap_or(DEFAULT_NOTIFICATION_DURATION_MS)
}

fn parse_notification_position(raw: &str) -> NotificationPosition {
    match raw.trim().to_ascii_lowercase().as_str() {
        "top-right" => NotificationPosition::TopRight,
        "bottom-left" => NotificationPosition::BottomLeft,
        "top-left" => NotificationPosition::TopLeft,
        _ => NotificationPosition::BottomRight,
    }
}

fn default_notification_position(app: &AppHandle) -> NotificationPosition {
    let Ok(path) = settings_file(app) else {
        return NotificationPosition::BottomRight;
    };
    let Ok(value) = read_json(path) else {
        return NotificationPosition::BottomRight;
    };
    value
        .get("notifications")
        .and_then(|item| item.get("position"))
        .and_then(Value::as_str)
        .map(parse_notification_position)
        .unwrap_or(NotificationPosition::BottomRight)
}

fn normalize_notification(app: &AppHandle, notification: NotificationRequest) -> NotificationRequest {
    let title = notification.title.trim();
    let message = notification.message.trim();
    let tone = match notification.tone.trim().to_ascii_lowercase().as_str() {
        "success" => "success",
        "warning" => "warning",
        "error" => "error",
        _ => "info",
    };
    NotificationRequest {
        title: if title.is_empty() {
            default_notification_title()
        } else {
            title.chars().take(80).collect()
        },
        message: if message.is_empty() {
            "通知".to_string()
        } else {
            message.to_string()
        },
        duration_ms: clamp_notification_duration(
            notification.duration_ms,
            default_notification_duration(app),
        ),
        tone: tone.to_string(),
        position: if notification.position.trim().is_empty() {
            match default_notification_position(app) {
                NotificationPosition::TopRight => "top-right",
                NotificationPosition::BottomLeft => "bottom-left",
                NotificationPosition::TopLeft => "top-left",
                NotificationPosition::BottomRight => "bottom-right",
            }
            .to_string()
        } else {
            match parse_notification_position(&notification.position) {
                NotificationPosition::TopRight => "top-right",
                NotificationPosition::BottomLeft => "bottom-left",
                NotificationPosition::TopLeft => "top-left",
                NotificationPosition::BottomRight => "bottom-right",
            }
            .to_string()
        },
    }
}

#[cfg(desktop)]
static NOTIFICATION_COUNTER: AtomicU64 = AtomicU64::new(0);

#[cfg(desktop)]
fn notification_position(
    app: &AppHandle,
    width: f64,
    height: f64,
    stack_index: u64,
    position_kind: NotificationPosition,
) -> Option<LogicalPosition<f64>> {
    let monitor = app.primary_monitor().ok().flatten()?;
    let scale = monitor.scale_factor();
    let work_area = monitor.work_area();
    let left = f64::from(work_area.position.x) / scale;
    let top = f64::from(work_area.position.y) / scale;
    let screen_width = f64::from(work_area.size.width) / scale;
    let screen_height = f64::from(work_area.size.height) / scale;
    let stack_offset = (stack_index % 5) as f64 * (height + 10.0);
    let x = match position_kind {
        NotificationPosition::BottomLeft | NotificationPosition::TopLeft => left + 22.0,
        NotificationPosition::BottomRight | NotificationPosition::TopRight => {
            left + screen_width - width - 22.0
        }
    };
    let y = match position_kind {
        NotificationPosition::TopLeft | NotificationPosition::TopRight => top + 24.0 + stack_offset,
        NotificationPosition::BottomLeft | NotificationPosition::BottomRight => top + screen_height - height - 18.0 - stack_offset,
    };
    Some(LogicalPosition::new(x, y))
}

#[cfg(desktop)]
fn show_notification_window(
    app: &AppHandle,
    notification: NotificationRequest,
) -> Result<(), String> {
    let notification = normalize_notification(app, notification);
    let counter = NOTIFICATION_COUNTER.fetch_add(1, Ordering::SeqCst);
    let label = format!("notification-{counter}");
    let payload = serde_json::to_vec(&notification).map_err(|error| error.to_string())?;
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload);
    let url = WebviewUrl::App(format!("notification.html?payload={encoded}").into());
    let position_kind = parse_notification_position(&notification.position);
    let width = 486.0;
    let height = 108.0;

    let window = WebviewWindowBuilder::new(app, label.clone(), url)
        .title("KXToDo 通知")
        .inner_size(width, height)
        .min_inner_size(width, height)
        .max_inner_size(width, height)
        .resizable(false)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .transparent(true)
        .shadow(true)
        .visible(false)
        .build()
        .map_err(|error| error.to_string())?;

    if let Some(position) = notification_position(app, width, height, counter, position_kind) {
        let _ = window.set_position(position);
    }
    window.show().map_err(|error| error.to_string())?;

    Ok(())
}

#[cfg(desktop)]
fn cli_usage() -> String {
    [
        "KXToDo 命令行用法：",
        "  KXToDo.exe                         打开 / 隐藏主窗口",
        "  KXToDo.exe -h | --help             显示帮助",
        "  KXToDo.exe notify <消息> [选项]     发送悬浮通知",
        "",
        "notify 选项：",
        "  -h, --help                         显示 notify 帮助",
        "  -t, --title <标题>                  通知标题",
        "  -m, --message <消息>                通知消息",
        "  -d, --duration <5200|5s|5200ms>     自动隐藏时长",
        "  --tone <info|success|warning|error> 通知样式",
        "  --position <bottom-right|top-right|bottom-left|top-left> 弹窗位置",
    ]
    .join("\n")
}

#[cfg(desktop)]
fn notify_usage() -> String {
    [
        "KXToDo notify 用法：",
        "  KXToDo.exe notify <消息> [--title 标题] [--duration 5s] [--tone success]",
        "  KXToDo.exe notify --message \"构建完成\" --title \"CI\" --position bottom-right",
    ]
    .join("\n")
}

#[cfg(desktop)]
fn parse_duration_ms(raw: &str) -> Result<u64, String> {
    let value = raw.trim();
    if let Some(ms) = value.strip_suffix("ms") {
        return ms
            .trim()
            .parse::<u64>()
            .map_err(|_| "duration 必须是毫秒数或 5s 这样的秒数".to_string());
    }
    if let Some(seconds) = value.strip_suffix('s') {
        let parsed = seconds
            .trim()
            .parse::<f64>()
            .map_err(|_| "duration 必须是毫秒数或 5s 这样的秒数".to_string())?;
        if parsed.is_finite() && parsed > 0.0 {
            return Ok((parsed * 1000.0).round() as u64);
        }
        return Err("duration 必须大于 0".to_string());
    }
    value
        .parse::<u64>()
        .map_err(|_| "duration 必须是毫秒数或 5s 这样的秒数".to_string())
}

#[cfg(desktop)]
fn take_cli_value(args: &[String], index: &mut usize, flag: &str) -> Result<String, String> {
    *index += 1;
    args.get(*index)
        .cloned()
        .ok_or_else(|| format!("{flag} 需要一个参数"))
}

#[cfg(desktop)]
#[derive(Debug, Clone)]
enum CliAction {
    Gui,
    Help(String),
    Notify(NotificationRequest),
    Error(String),
}

#[cfg(all(desktop, target_os = "windows"))]
fn print_cli_text(message: &str) {
    use windows_sys::Win32::{
        Foundation::INVALID_HANDLE_VALUE,
        Storage::FileSystem::WriteFile,
        System::Console::{
            AttachConsole, GetStdHandle, WriteConsoleW, ATTACH_PARENT_PROCESS, STD_OUTPUT_HANDLE,
        },
    };
    let stdout_text = format!("{message}\n");
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
    unsafe {
        let handle = GetStdHandle(STD_OUTPUT_HANDLE);
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            let _ = std::io::stdout().write_all(stdout_text.as_bytes());
            let _ = std::io::stdout().flush();
            return;
        }
        let text = format!("{message}\r\n");
        let wide = text.encode_utf16().collect::<Vec<_>>();
        let mut written = 0;
        if WriteConsoleW(
            handle,
            wide.as_ptr(),
            wide.len() as u32,
            &mut written,
            std::ptr::null_mut(),
        ) != 0
        {
            return;
        }
        let bytes = stdout_text.as_bytes();
        let _ = WriteFile(
            handle,
            bytes.as_ptr(),
            bytes.len() as u32,
            &mut written,
            std::ptr::null_mut(),
        );
    }
}

#[cfg(all(desktop, not(target_os = "windows")))]
fn print_cli_text(message: &str) {
    println!("{message}");
}

#[cfg(desktop)]
fn parse_cli_action(args: &[String]) -> CliAction {
    let Some(command) = args.first().map(String::as_str) else {
        return CliAction::Gui;
    };
    match command {
        "-h" | "--help" | "help" => CliAction::Help(cli_usage()),
        "notify" => match parse_cli_notification(args) {
            Ok(notification) => CliAction::Notify(notification),
            Err(message) if message == notify_usage() => CliAction::Help(message),
            Err(message) => CliAction::Error(message),
        },
        unsupported => CliAction::Error(format!(
            "不支持的命令行参数：{unsupported}\n\n{}",
            cli_usage()
        )),
    }
}

#[cfg(desktop)]
fn parse_cli_notification(args: &[String]) -> Result<NotificationRequest, String> {
    let mut notification = NotificationRequest {
        title: default_notification_title(),
        message: String::new(),
        duration_ms: 0,
        tone: "info".to_string(),
        position: String::new(),
    };
    let mut message_parts = Vec::new();
    let mut index = 1;
    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "--help" | "-h" => return Err(notify_usage()),
            "--title" | "-t" => {
                notification.title = take_cli_value(args, &mut index, arg)?;
            }
            "--message" | "--body" | "-m" => {
                notification.message = take_cli_value(args, &mut index, arg)?;
            }
            "--duration" | "--timeout" | "-d" => {
                let value = take_cli_value(args, &mut index, arg)?;
                notification.duration_ms = parse_duration_ms(&value)?;
            }
            "--tone" | "--type" => {
                notification.tone = take_cli_value(args, &mut index, arg)?;
            }
            "--position" => {
                notification.position = take_cli_value(args, &mut index, arg)?;
            }
            value if value.starts_with("--title=") => {
                notification.title = value.trim_start_matches("--title=").to_string();
            }
            value if value.starts_with("--message=") => {
                notification.message = value.trim_start_matches("--message=").to_string();
            }
            value if value.starts_with("--body=") => {
                notification.message = value.trim_start_matches("--body=").to_string();
            }
            value if value.starts_with("--duration=") => {
                notification.duration_ms = parse_duration_ms(value.trim_start_matches("--duration="))?;
            }
            value if value.starts_with("--timeout=") => {
                notification.duration_ms = parse_duration_ms(value.trim_start_matches("--timeout="))?;
            }
            value if value.starts_with("--tone=") => {
                notification.tone = value.trim_start_matches("--tone=").to_string();
            }
            value if value.starts_with("--type=") => {
                notification.tone = value.trim_start_matches("--type=").to_string();
            }
            value if value.starts_with("--position=") => {
                notification.position = value.trim_start_matches("--position=").to_string();
            }
            value => message_parts.push(value.to_string()),
        }
        index += 1;
    }

    if notification.message.trim().is_empty() {
        notification.message = message_parts.join(" ");
    }
    if notification.message.trim().is_empty() {
        return Err(notify_usage());
    }

    Ok(notification)
}

#[cfg(desktop)]
#[tauri::command]
fn send_notification(notification: NotificationRequest) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut cmd = Command::new(&exe);
    cmd.arg("notify")
        .arg("--message")
        .arg(&notification.message)
        .arg("--title")
        .arg(&notification.title);

    if notification.duration_ms > 0 {
        cmd.arg("--duration")
            .arg(format!("{}ms", notification.duration_ms));
    }
    if !notification.tone.trim().is_empty() {
        cmd.arg("--tone").arg(&notification.tone);
    }
    if !notification.position.trim().is_empty() {
        cmd.arg("--position").arg(&notification.position);
    }

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    cmd.spawn()
        .map_err(|e| format!("Failed to spawn notification process: {}", e))?;
    Ok(())
}

#[cfg(desktop)]
#[tauri::command]
fn close_notification_window(window: tauri::Window) -> Result<(), String> {
    if !window.label().starts_with("notification-") {
        return Err("当前窗口不是通知窗口".to_string());
    }
    window.close().map_err(|error| error.to_string())
}

#[cfg(not(desktop))]
#[tauri::command]
fn send_notification(_notification: NotificationRequest) -> Result<(), String> {
    Ok(())
}

#[cfg(not(desktop))]
#[tauri::command]
fn close_notification_window() -> Result<(), String> {
    Ok(())
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
    if action.action_type == "notification" {
        return Err("Notification actions are handled by the scheduler".to_string());
    }
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
fn mutex_error<T>(error: std::sync::PoisonError<T>) -> String {
    error.to_string()
}

#[cfg(desktop)]
fn read_pipe_to_string<R: Read>(mut reader: R) -> String {
    let mut bytes = Vec::new();
    let _ = reader.read_to_end(&mut bytes);
    String::from_utf8_lossy(&bytes).to_string()
}

#[cfg(desktop)]
fn wait_for_child(child: &Arc<Mutex<Child>>) -> Result<std::process::ExitStatus, String> {
    loop {
        if let Some(status) = child
            .lock()
            .map_err(mutex_error)?
            .try_wait()
            .map_err(|error| error.to_string())?
        {
            return Ok(status);
        }
        thread::sleep(Duration::from_millis(80));
    }
}

#[cfg(desktop)]
fn run_cancellable_command(
    mut command: Command,
    process_state: Arc<SchedulerProcessState>,
    task_id: Option<String>,
) -> Result<ScheduledActionOutput, String> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_thread = stdout.map(|reader| thread::spawn(move || read_pipe_to_string(reader)));
    let stderr_thread = stderr.map(|reader| thread::spawn(move || read_pipe_to_string(reader)));
    let child = Arc::new(Mutex::new(child));

    if let Some(task_id) = task_id.as_ref().filter(|value| !value.trim().is_empty()) {
        process_state
            .children
            .lock()
            .map_err(mutex_error)?
            .insert(task_id.to_string(), child.clone());
    }

    let status = wait_for_child(&child);
    if let Some(task_id) = task_id.as_ref().filter(|value| !value.trim().is_empty()) {
        let _ = process_state
            .children
            .lock()
            .map_err(mutex_error)
            .map(|mut children| children.remove(task_id));
    }
    let status = status?;
    let stdout = stdout_thread
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();
    let stderr = stderr_thread
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();

    Ok(ScheduledActionOutput {
        exit_code: status.code(),
        stdout,
        stderr,
    })
}

#[cfg(desktop)]
#[tauri::command]
async fn run_scheduled_action(
    app: AppHandle,
    process_state: State<'_, Arc<SchedulerProcessState>>,
    task_id: Option<String>,
    action: ScheduledActionCommand,
    runtimes: HashMap<String, String>,
) -> Result<ScheduledActionOutput, String> {
    let process_state = process_state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (command, temp_file) = build_scheduled_command(&app, &action, &runtimes)?;
        let output = run_cancellable_command(command, process_state, task_id);
        if let Some(path) = temp_file {
            let _ = fs::remove_file(path);
        }
        output
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(desktop)]
#[tauri::command]
fn stop_scheduled_action(
    process_state: State<'_, Arc<SchedulerProcessState>>,
    task_id: String,
) -> Result<(), String> {
    let child = process_state
        .children
        .lock()
        .map_err(mutex_error)?
        .get(task_id.trim())
        .cloned();
    if let Some(child) = child {
        child
            .lock()
            .map_err(mutex_error)?
            .kill()
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(not(desktop))]
#[tauri::command]
async fn run_scheduled_action(
    _task_id: Option<String>,
    _action: ScheduledActionCommand,
    _runtimes: HashMap<String, String>,
) -> Result<ScheduledActionOutput, String> {
    Err("Scheduled task execution is not supported on mobile".to_string())
}

#[cfg(not(desktop))]
#[tauri::command]
fn stop_scheduled_action(_task_id: String) -> Result<(), String> {
    Ok(())
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
fn toggle_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let visible = window.is_visible().unwrap_or(false);
        let minimized = window.is_minimized().unwrap_or(false);
        if visible && !minimized {
            let _ = window.hide();
        } else {
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
        }
    }
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

            toggle_main_window(&app_handle);
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
    #[cfg(desktop)]
    let cli_action = {
        let args = env::args().skip(1).collect::<Vec<_>>();
        parse_cli_action(&args)
    };
    #[cfg(desktop)]
    {
        match &cli_action {
            CliAction::Help(message) => {
                print_cli_text(message);
                std::process::exit(0);
            }
            CliAction::Error(message) => {
                print_cli_text(message);
                std::process::exit(2);
            }
            CliAction::Gui | CliAction::Notify(_) => {}
        }
    }
    #[cfg(desktop)]
    let is_cli_notify = matches!(cli_action, CliAction::Notify(_));

    let builder = tauri::Builder::default().manage(LifecycleState::default());

    #[cfg(desktop)]
    let builder = builder.manage(Arc::new(SchedulerProcessState::default()));

    #[cfg(desktop)]
    let builder = if is_cli_notify {
        builder
    } else {
        builder
            .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
                toggle_main_window(app);
            }))
            .plugin(tauri_plugin_window_state::Builder::default().build())
            .plugin(tauri_plugin_global_shortcut::Builder::new().build())
            .plugin(tauri_plugin_autostart::init(
                MacosLauncher::LaunchAgent,
                None,
            ))
    };

    #[cfg(not(desktop))]
    let builder = builder.plugin(tauri_plugin_opener::init());

    let mut context = tauri::generate_context!();
    #[cfg(desktop)]
    if is_cli_notify {
        for window in &mut context.config_mut().app.windows {
            window.create = false;
        }
    }

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
            stop_scheduled_action,
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
            send_notification,
            close_notification_window,
            register_global_shortcut,
            set_close_to_tray,
            set_autostart,
            get_autostart_enabled,
            set_webview_zoom,
            open_url
        ])
        .setup(move |app| {
            ensure_storage_layout(app.handle()).map_err(|error| {
                tauri::Error::Io(std::io::Error::new(std::io::ErrorKind::Other, error))
            })?;
            #[cfg(desktop)]
            {
                if let CliAction::Notify(notification) = cli_action.clone() {
                    show_notification_window(app.handle(), notification).map_err(|error| {
                        tauri::Error::Io(std::io::Error::new(std::io::ErrorKind::Other, error))
                    })?;
                    return Ok(());
                }
                if let Some(webview) = app.get_webview_window("main") {
                    let _ = webview.set_zoom(1.0);
                }
                setup_tray(app)?;
                show_main_window(app.handle());
            }
            Ok(())
        })
        .on_window_event(move |window, event| {
            #[cfg(desktop)]
            {
                if window.label().starts_with("notification-") {
                    if let tauri::WindowEvent::CloseRequested { .. } = event {
                        if is_cli_notify {
                            window.app_handle().exit(0);
                        }
                    }
                    return;
                }
            }
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                #[cfg(desktop)]
                {
                    if window.label() != "main" {
                        return;
                    }
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
        .run(context)
        .expect("failed to run KXToDo");
}
