use base64::Engine;
use serde::Deserialize;
#[cfg(desktop)]
use serde::Serialize;
use serde_json::{json, Value};
#[cfg(desktop)]
use std::collections::HashMap;
#[cfg(desktop)]
use std::env;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    sync::Arc,
};
#[cfg(desktop)]
use std::sync::atomic::AtomicBool;
use tauri::{AppHandle, Manager, State};

use kxtodo_core as domain;

#[cfg(desktop)]
use tauri::{
    image::Image,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    WebviewUrl, WebviewWindowBuilder,
};
#[cfg(all(desktop, not(target_os = "linux")))]
use tauri::LogicalPosition;
#[cfg(desktop)]
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
#[cfg(desktop)]
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
#[cfg(desktop)]
use tauri_plugin_window_state::StateFlags;

#[cfg(desktop)]
const DEFAULT_UI_SCALE: f64 = 0.75;
const IMG_DIR: &str = "img";
const AVATAR_DIR: &str = "avator";
const BACKGROUND_DIR: &str = "background";
const ENTRY_IMAGE_DIR: &str = "data";
#[cfg(all(desktop, not(target_os = "linux")))]
const DEFAULT_NOTIFICATION_DURATION_MS: u64 = 3_000;

#[cfg(desktop)]
struct LifecycleState {
    close_to_tray: AtomicBool,
    quitting: AtomicBool,
}

#[cfg(desktop)]
impl Default for LifecycleState {
    fn default() -> Self {
        Self {
            // Linux 默认关闭即退出：WSLg/GNOME 托盘常不可见，隐藏到看不见的托盘
            // 等于把应用弄丢；设置里仍可显式开启关闭到托盘。
            close_to_tray: AtomicBool::new(!cfg!(target_os = "linux")),
            quitting: AtomicBool::new(false),
        }
    }
}

#[cfg(all(desktop, not(target_os = "linux")))]
#[derive(Debug, Clone, Copy)]
enum NotificationPosition {
    BottomRight,
    TopRight,
    BottomLeft,
    TopLeft,
}

/// GUI 的默认数据目录（桌面端：系统标准数据目录 kxtodo/todo-note-data/）。
#[cfg(desktop)]
fn default_data_dir() -> PathBuf {
    domain::repo::default_data_dir()
}

fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    #[cfg(desktop)]
    let dir = {
        let _ = app;
        default_data_dir()
    };
    #[cfg(not(desktop))]
    let dir = app.path().app_data_dir().map_err(|error| error.to_string())?;
    Ok(dir)
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

#[cfg(all(desktop, not(target_os = "linux")))]
fn settings_file(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(data_dir(app)?.join("settings.json"))
}

#[cfg(desktop)]
fn ensure_parent(path: &PathBuf) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(all(desktop, not(target_os = "linux")))]
fn read_json(path: PathBuf) -> Result<Value, String> {
    if !path.exists() {
        return Ok(json!(null));
    }

    let raw = fs::read_to_string(path).map_err(|error| error.to_string())?;
    serde_json::from_str(&raw).map_err(|error| error.to_string())
}

#[cfg(desktop)]
fn write_json(path: PathBuf, value: Value) -> Result<(), String> {
    ensure_parent(&path)?;
    let raw = serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?;
    fs::write(path, raw).map_err(|error| error.to_string())
}

#[cfg(desktop)]
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

#[cfg(desktop)]
fn default_notification_title() -> String {
    "KXToDo".to_string()
}

#[cfg(desktop)]
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
    #[serde(default)]
    title_font_size: f64,
    #[serde(default)]
    body_font_size: f64,
}

#[cfg(all(desktop, target_os = "windows"))]
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

#[cfg(all(desktop, not(target_os = "windows")))]
fn executable_candidates(name: &str) -> Vec<String> {
    vec![name.to_string()]
}

#[cfg(desktop)]
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

#[cfg(desktop)]
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

#[cfg(desktop)]
fn default_executor_paths() -> HashMap<String, String> {
    let mut paths = HashMap::new();
    paths.insert(
        "python".to_string(),
        find_executable(
            &["python", "python3", "py"],
            &["PYTHON", "PYTHON_EXECUTABLE"],
        ),
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

#[cfg(desktop)]
#[tauri::command]
fn resolve_executor_paths() -> HashMap<String, String> {
    default_executor_paths()
}

#[cfg(desktop)]
#[tauri::command]
fn resolve_executable_path(name: String) -> Option<String> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let path = find_executable(&[name], &[]);
    if path.is_empty() {
        None
    } else {
        Some(path)
    }
}

#[cfg(all(desktop, not(target_os = "linux")))]
fn clamp_notification_duration(duration_ms: u64, fallback: u64) -> u64 {
    let raw = if duration_ms == 0 {
        fallback
    } else {
        duration_ms
    };
    raw.clamp(1_200, 60_000)
}

#[cfg(all(desktop, not(target_os = "linux")))]
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

#[cfg(all(desktop, not(target_os = "linux")))]
fn parse_notification_position(raw: &str) -> NotificationPosition {
    match raw.trim().to_ascii_lowercase().as_str() {
        "top-right" => NotificationPosition::TopRight,
        "bottom-left" => NotificationPosition::BottomLeft,
        "top-left" => NotificationPosition::TopLeft,
        _ => NotificationPosition::BottomRight,
    }
}

#[cfg(all(desktop, not(target_os = "linux")))]
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

#[cfg(all(desktop, not(target_os = "linux")))]
fn notification_setting_f64(app: &AppHandle, field: &str, fallback: f64) -> f64 {
    let Ok(path) = settings_file(app) else {
        return fallback;
    };
    let Ok(value) = read_json(path) else {
        return fallback;
    };
    value
        .get("notifications")
        .and_then(|item| item.get(field))
        .and_then(Value::as_f64)
        .filter(|v| *v > 0.0)
        .unwrap_or(fallback)
}

#[cfg(all(desktop, not(target_os = "linux")))]
fn normalize_notification(
    app: &AppHandle,
    notification: NotificationRequest,
) -> NotificationRequest {
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
        title_font_size: if notification.title_font_size > 0.0 {
            notification.title_font_size
        } else {
            notification_setting_f64(app, "titleFontSize", 14.0)
        },
        body_font_size: if notification.body_font_size > 0.0 {
            notification.body_font_size
        } else {
            notification_setting_f64(app, "bodyFontSize", 12.0)
        },
    }
}

#[cfg(all(desktop, not(target_os = "linux")))]
static NOTIFICATION_COUNTER: AtomicU64 = AtomicU64::new(0);

/// 近 3 秒内新建通知窗的出生时间：show 有最多 900ms 兜底延迟，新建时可能还不可见。
#[cfg(all(desktop, not(target_os = "linux")))]
static NOTIFICATION_BIRTHS: std::sync::Mutex<Vec<std::time::Instant>> = std::sync::Mutex::new(Vec::new());

#[cfg(all(desktop, not(target_os = "linux")))]
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
    // 窗口外框含 DWM 阴影（左右各约 7px），水平边距 +7 补偿，让卡片本体的
    // 视觉右/左边距与底部 18px 一致。
    let x = match position_kind {
        NotificationPosition::BottomLeft | NotificationPosition::TopLeft => left + 25.0,
        NotificationPosition::BottomRight | NotificationPosition::TopRight => {
            left + screen_width - width - 25.0
        }
    };
    let y = match position_kind {
        NotificationPosition::TopLeft | NotificationPosition::TopRight => top + 24.0 + stack_offset,
        NotificationPosition::BottomLeft | NotificationPosition::BottomRight => {
            top + screen_height - height - 18.0 - stack_offset
        }
    };
    Some(LogicalPosition::new(x, y))
}

#[cfg(all(desktop, not(target_os = "linux")))]
fn show_notification_window(
    app: &AppHandle,
    notification: NotificationRequest,
) -> Result<String, String> {
    let notification = normalize_notification(app, notification);
    let counter = NOTIFICATION_COUNTER.fetch_add(1, Ordering::SeqCst);
    let label = format!("notification-{counter}");
    // 建窗/定位/显示全部放主线程执行：调用方是 IPC/命令工作线程，跨线程
    // dispatch 在事件循环繁忙时丢过 show 与 position（窗口建好却不可见）。
    let app_clone = app.clone();
    let label_clone = label.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(build_notification_window(&app_clone, label_clone, notification));
    })
    .map_err(|error| error.to_string())?;
    rx.recv().map_err(|error| error.to_string())?
}

#[cfg(all(desktop, not(target_os = "linux")))]
fn build_notification_window(
    app: &AppHandle,
    label: String,
    notification: NotificationRequest,
) -> Result<String, String> {
    let payload = serde_json::to_vec(&notification).map_err(|error| error.to_string())?;
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload);
    let url = WebviewUrl::App(format!("notification.html?payload={encoded}").into());
    let position_kind = parse_notification_position(&notification.position);
    let width = notification_setting_f64(app, "width", 400.0);
    let height = notification_setting_f64(app, "height", 68.0);
    // 堆叠序号 = 当前可见通知窗数 与 近 3 秒新建数 取大。生命周期计数器会把
    // 已消失窗口的位置继续算给新窗；只数存活窗口又会把“视觉已关、销毁尚未
    // 完成”的窗口算进去，连发时越叠越高。
    let visible_count = app
        .webview_windows()
        .iter()
        .filter(|(label, window)| {
            label.starts_with("notification-") && window.is_visible().unwrap_or(false)
        })
        .count();
    let now = std::time::Instant::now();
    let stack_index = {
        let mut births = NOTIFICATION_BIRTHS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        births.retain(|at| now.duration_since(*at).as_millis() < 3_000);
        let index = visible_count.max(births.len()) as u64;
        births.push(now);
        index
    };
    let position = notification_position(app, width, height, stack_index, position_kind);
    let duration_ms = notification.duration_ms;

    // 隐藏创建，内容首帧渲染后由 notification_ready 命令显示（消除白帧闪烁）；
    // JS 若没跑起来，900ms 兜底 show + duration+4s 看门狗强关，窗口绝不僵死。
    let mut builder = WebviewWindowBuilder::new(app, label.clone(), url)
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
        .visible(false);
    if let Some(position) = position {
        builder = builder.position(position.x, position.y);
    }
    builder.build().map_err(|error| error.to_string())?;

    let app_clone = app.clone();
    let label_clone = label.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(900));
        if let Some(window) = app_clone.get_webview_window(&label_clone) {
            let _ = window.show();
        }
        std::thread::sleep(std::time::Duration::from_millis(duration_ms.saturating_add(4000)));
        if let Some(window) = app_clone.get_webview_window(&label_clone) {
            let _ = window.close();
        }
    });

    Ok(label)
}

#[cfg(desktop)]
#[tauri::command]
fn notification_ready(window: tauri::Window) -> Result<(), String> {
    if window.label().starts_with("notification-") {
        let _ = window.show();
    }
    Ok(())
}

#[cfg(desktop)]
#[tauri::command]
// async 让命令跑在线程池：同步命令占主线程时，show_notification_window 内的
// run_on_main_thread + 阻塞 recv 会在主线程上自等待，直接把整个应用锁死。
async fn send_notification(
    notification: NotificationRequest,
    core: State<'_, Arc<domain::host::HostCore>>,
) -> Result<(), String> {
    let payload = serde_json::json!({
        "title": notification.title,
        "message": notification.message,
        "duration": if notification.duration_ms > 0 {
            Some(format!("{}ms", notification.duration_ms))
        } else {
            None
        },
        "tone": if notification.tone.trim().is_empty() {
            None
        } else {
            Some(notification.tone.clone())
        },
        "position": if notification.position.trim().is_empty() {
            None
        } else {
            Some(notification.position.clone())
        },
    });
    let merged = core
        .resolve_notification_payload(
            payload.get("title").and_then(Value::as_str),
            payload.get("message").and_then(Value::as_str).unwrap_or(""),
            payload.get("duration").and_then(Value::as_str),
            payload.get("tone").and_then(Value::as_str),
            payload.get("position").and_then(Value::as_str),
            false,
        )
        .map_err(|error| error.to_string())?;
    core.show_notification_payload(merged)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(desktop)]
#[tauri::command]
fn close_notification_window(window: tauri::Window) -> Result<(), String> {
    if !window.label().starts_with("notification-") {
        return Err("当前窗口不是通知窗口".to_string());
    }
    window.close().map_err(|error| error.to_string())
}

#[cfg(desktop)]
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

/// 存储图像转 base64 dataURL（kind: avatar/background/md）。Linux 部分环境的
/// WebKitGTK 对 asset 协议子资源不发请求（strace 实测零次文件打开），图像改走
/// dataURL——与移动端既有路径同款；其余平台继续用 asset 协议。
#[tauri::command]
fn image_data_url(
    app: AppHandle,
    kind: String,
    filename: String,
    node_id: Option<String>,
) -> Result<String, String> {
    safe_image_name(&filename)?;
    let dir = match kind.as_str() {
        "avatar" => avatar_dir(&app)?,
        "background" => images_dir(&app)?,
        "md" => md_images_dir(&app, &node_id.unwrap_or_default())?,
        _ => return Err("Invalid image kind".to_string()),
    };
    let path = dir.join(&filename);
    let bytes = fs::read(&path).map_err(|_| "File not found".to_string())?;
    if bytes.len() > 24 * 1024 * 1024 {
        return Err("Image too large for data URL".to_string());
    }
    let mime = match path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        _ => "application/octet-stream",
    };
    Ok(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&bytes)
    ))
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

#[cfg(desktop)]
#[tauri::command]
fn set_close_to_tray(state: State<LifecycleState>, enabled: bool) {
    state.close_to_tray.store(enabled, Ordering::SeqCst);
}

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

#[cfg(desktop)]
#[tauri::command]
fn get_autostart_enabled(app: AppHandle) -> Result<bool, String> {
    app.autolaunch()
        .is_enabled()
        .map_err(|error| error.to_string())
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

#[tauri::command]
fn app_version() -> String {
    env!("KXTODO_VERSION").to_string()
}

// ---------------------------------------------------------------------------
// 应用更新：GitHub release 双产物下载到 exe 目录 → 稳定名 shim → 重启。
// Windows 用硬链接（同名目录无需特权，退出后由 bat 换链）；Unix 用符号链接。
// ---------------------------------------------------------------------------

#[cfg(desktop)]
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateApplyParams {
    gui_url: String,
    cli_url: String,
}

fn update_emit(app: &AppHandle, event: &str, payload: Value) {
    use tauri::Emitter;
    let _ = app.emit(event, payload);
}

fn update_agent() -> ureq::Agent {
    // ureq 默认探测 HTTP(S)_PROXY 环境变量，无需手工配置。
    ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(15))
        .build()
}

/// GitHub 直连失败时的下载回退代理：把完整 GitHub 链接直接拼在它后面
/// （`https://ghfast.top/https://github.com/…/KXToDo.exe`）。
/// 只用于**下载**：版本检查仍直连 api.github.com（实测该代理不接受 api 域名，返回 403）。
const UPDATE_PROXY_PREFIX: &str = "https://ghfast.top/";

/// 下载一个更新制品：先直连 GitHub，失败则静默换代理重试；
/// 两条路都失败时把两个原因一起报出来（用户要求「如果也失败了，那就再把失败原因爆出来」）。
fn update_download_file(
    app: &AppHandle,
    agent: &ureq::Agent,
    stage: &str,
    url: &str,
    dest: &std::path::Path,
) -> Result<(), String> {
    match stream_update_file(app, agent, stage, url, dest) {
        Ok(()) => Ok(()),
        Err(direct_error) => {
            let proxied = format!("{UPDATE_PROXY_PREFIX}{url}");
            update_emit(
                app,
                "update://progress",
                serde_json::json!({
                    "stage": stage,
                    "received": 0,
                    "total": 0,
                    "percent": 0,
                    "message": "GitHub 直连失败，改用加速代理重试"
                }),
            );
            stream_update_file(app, agent, stage, &proxied, dest).map_err(|proxy_error| {
                format!(
                    "下载 {stage} 失败\n  直连：{direct_error}\n  代理（{UPDATE_PROXY_PREFIX}）：{proxy_error}"
                )
            })
        }
    }
}

fn stream_update_file(
    app: &AppHandle,
    agent: &ureq::Agent,
    stage: &str,
    url: &str,
    dest: &std::path::Path,
) -> Result<(), String> {
    let response = agent
        .get(url)
        .call()
        .map_err(|error| error.to_string())?;
    let total = response
        .header("content-length")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let mut reader = response.into_reader();
    let file_name = dest
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("download")
        .to_string();
    let part = dest.with_file_name(format!("{file_name}.part"));
    use std::io::{Read, Write};
    let write_result = (|| -> std::io::Result<()> {
        let mut file = fs::File::create(&part)?;
        let mut buffer = [0u8; 64 * 1024];
        let mut received: u64 = 0;
        let mut last_percent: u64 = u64::MAX;
        loop {
            let count = reader.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            file.write_all(&buffer[..count])?;
            received += count as u64;
            if total > 0 {
                let percent = received * 100 / total;
                if percent != last_percent {
                    last_percent = percent;
                    update_emit(
                        app,
                        "update://progress",
                        serde_json::json!({
                            "stage": stage,
                            "received": received,
                            "total": total,
                            "percent": percent
                        }),
                    );
                }
            }
        }
        Ok(())
    })();
    if let Err(error) = write_result {
        let _ = fs::remove_file(&part);
        return Err(format!("写入临时文件失败：{error}"));
    }
    if dest.exists() {
        fs::remove_file(dest).map_err(|error| format!("替换旧 {stage} 安装包失败：{error}"))?;
    }
    fs::rename(&part, dest).map_err(|error| format!("保存 {stage} 安装包失败：{error}"))?;
    Ok(())
}

/// Windows 换文件脚本：等本进程退出 → 删旧固定名产物 → 把 .new 暂存文件改名就位 → 重启 → 自删。
/// 删名/改名各最多等 15 秒（防其他 kxtodo 进程短暂占用）。产物固定名不带版本号，也不写任何更新日志。
#[cfg(all(desktop, windows))]
fn write_update_bat(dir: &std::path::Path, pid: u32) -> Result<PathBuf, String> {
    let bat = dir.join("kxtodo-update.bat");
    let script = format!(
        "@echo off\r\ncd /d \"{dir}\"\r\n:wait\r\ntasklist /FI \"PID eq {pid}\" | findstr /C:\" {pid} \" >nul\r\nif %errorlevel%==0 (timeout /t 1 /nobreak >nul & goto wait)\r\nfor /l %%i in (1,1,15) do (\r\n  if not exist \"KXToDo.exe\" goto movegui\r\n  del /f /q \"KXToDo.exe\" >nul 2>&1\r\n  timeout /t 1 /nobreak >nul\r\n)\r\n:movegui\r\nmove /y \"KXToDo.exe.new\" \"KXToDo.exe\" >nul 2>&1\r\nfor /l %%i in (1,1,15) do (\r\n  if not exist \"kxtodo-cli.exe\" goto movecli\r\n  del /f /q \"kxtodo-cli.exe\" >nul 2>&1\r\n  timeout /t 1 /nobreak >nul\r\n)\r\n:movecli\r\nmove /y \"kxtodo-cli.exe.new\" \"kxtodo-cli.exe\" >nul 2>&1\r\nstart \"\" \"KXToDo.exe\"\r\ndel \"%~f0\"\r\n",
        dir = dir.display(),
        pid = pid
    );
    fs::write(&bat, script).map_err(|error| format!("写入更新脚本失败：{error}"))?;
    Ok(bat)
}

/// Linux 更新制品目录：$XDG_DATA_HOME/kxtodo/bin，缺省回退 ~/.local/share/kxtodo/bin。
/// 与数据目录（kxtodo/todo-note-data）同一数据根，是 GUI/CLI 的规范安装位置。
#[cfg(all(desktop, not(windows)))]
fn linux_update_bin_dir() -> Result<PathBuf, String> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local").join("share"))
        })
        .ok_or_else(|| "无法定位 ~/.local/share（XDG_DATA_HOME 与 HOME 均未设置）".to_string())?;
    Ok(base.join("kxtodo").join("bin"))
}

/// 桌面更新应用结果：决定命令层是自动退出重启，还是提示用户手动重启。
#[cfg(desktop)]
enum UpdateOutcome {
    /// 新实例已/将被拉起，本进程应退出。
    Restarting,
    /// 制品已替换但无法自动拉起（如 Linux AppImage 启动失败），提示用户手动重启。
    /// 仅非 Windows 桌面会构造（Windows 一律走 bat 自动重启），故对 Windows 构建放行 dead_code。
    #[allow(dead_code)]
    ManualRestart(String),
}

#[cfg(desktop)]
fn run_update(app: &AppHandle, params: &UpdateApplyParams) -> Result<UpdateOutcome, String> {
    let agent = update_agent();

    #[cfg(windows)]
    {
        // Windows：下载到 exe 同目录的 .new 暂存文件（运行中的 KXToDo.exe 被锁定，不能直接覆盖），
        // 由 bat 等本进程退出后删旧、把 .new 改名就位并重启。产物固定名不带版本号，也不写更新日志。
        let dir = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf))
            .ok_or_else(|| "无法定位程序目录".to_string())?;
        let gui_dest = dir.join("KXToDo.exe.new");
        let cli_dest = dir.join("kxtodo-cli.exe.new");
        update_download_file(app, &agent, "GUI", &params.gui_url, &gui_dest)?;
        update_download_file(app, &agent, "CLI", &params.cli_url, &cli_dest)?;
        // 自检：错误页/JSON 响应体积远小于真实产物，拦下明显存坏的下载。
        for (stage, path) in [("GUI", &gui_dest), ("CLI", &cli_dest)] {
            let size = fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
            if size < 64 * 1024 {
                return Err(format!("{stage} 更新包异常（仅 {size} 字节），已中止"));
            }
        }
        // CREATE_NO_WINDOW：cmd 拿到一个不可见控制台跑完整个 bat，更新重启全程无黑窗。
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let bat = write_update_bat(&dir, std::process::id())?;
        std::process::Command::new("cmd")
            .arg("/c")
            .arg(&bat)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|error| format!("无法启动更新脚本：{error}"))?;
        return Ok(UpdateOutcome::Restarting);
    }

    #[cfg(not(windows))]
    {
        // Linux：下载固定名 KXToDo.AppImage + kxtodo-cli 到 ~/.local/share/kxtodo/bin 替换，
        // 再尝试拉起新 AppImage；成功则本进程退出（新实例接管），失败则提示用户手动重启。
        use std::os::unix::fs::PermissionsExt;
        let bin = linux_update_bin_dir()?;
        fs::create_dir_all(&bin)
            .map_err(|error| format!("无法创建 {}：{error}", bin.display()))?;
        let gui_dest = bin.join("KXToDo.AppImage");
        let cli_dest = bin.join("kxtodo-cli");
        update_download_file(app, &agent, "GUI", &params.gui_url, &gui_dest)?;
        update_download_file(app, &agent, "CLI", &params.cli_url, &cli_dest)?;
        // 自检：错误页/JSON 响应体积远小于真实产物，拦下明显存坏的下载。
        for (stage, path) in [("GUI", &gui_dest), ("CLI", &cli_dest)] {
            let size = fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
            if size < 64 * 1024 {
                return Err(format!("{stage} 更新包异常（仅 {size} 字节），已中止"));
            }
        }
        // AppImage 与 CLI 都要可执行位，否则 spawn / 用户直接跑会 EACCES。
        for path in [&gui_dest, &cli_dest] {
            let mut perms = fs::metadata(path)
                .map(|meta| meta.permissions())
                .map_err(|error| format!("读取 {} 权限失败：{error}", path.display()))?;
            perms.set_mode(0o755);
            fs::set_permissions(path, perms)
                .map_err(|error| format!("设置 {} 可执行位失败：{error}", path.display()))?;
        }
        // 脱离终端拉起新 AppImage；本进程退出后新实例由 init 接管。
        match std::process::Command::new(&gui_dest)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(_) => Ok(UpdateOutcome::Restarting),
            Err(error) => Ok(UpdateOutcome::ManualRestart(format!(
                "已下载新版本到 {}，自动重启失败（{error}），请手动重启应用完成更新",
                bin.display()
            ))),
        }
    }
}

/// 下载 GUI + CLI（固定名制品）替换并重启（后台线程执行，进度走事件）。
#[cfg(desktop)]
#[tauri::command]
fn update_download_and_apply(app: AppHandle, params: UpdateApplyParams) -> Result<(), String> {
    for url in [&params.gui_url, &params.cli_url] {
        if !url.starts_with("https://github.com/wddjwk/kxtodo/releases/download/")
            && !url.starts_with("https://objects.githubusercontent.com/")
        {
            return Err(format!("更新地址不受信任：{url}"));
        }
    }
    std::thread::spawn(move || match run_update(&app, &params) {
        Ok(UpdateOutcome::Restarting) => {
            update_emit(&app, "update://applied", serde_json::json!({}));
            let _ = app.exit(0);
        }
        Ok(UpdateOutcome::ManualRestart(message)) => {
            // 制品已替换但无法自动拉起：通知前端提示手动重启，本进程不退出。
            update_emit(
                &app,
                "update://applied",
                serde_json::json!({ "manualRestart": true, "message": message }),
            );
        }
        Err(message) => {
            update_emit(&app, "update://failed", serde_json::json!({ "message": message }));
        }
    });
    Ok(())
}

// ---------------------------------------------------------------------------
// 移动端应用更新：下载 APK 到 cacheDir，安装动作由前端经 Kotlin 桥
// （window.kxtodoAndroid.installApk）触发 PackageInstaller。
// ---------------------------------------------------------------------------

#[cfg(not(desktop))]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateApkParams {
    version: String,
    apk_url: String,
}

#[cfg(not(desktop))]
#[tauri::command]
fn update_download_apk(app: AppHandle, params: UpdateApkParams) -> Result<(), String> {
    if !params.apk_url.starts_with("https://github.com/wddjwk/kxtodo/releases/download/")
        && !params.apk_url.starts_with("https://objects.githubusercontent.com/")
    {
        return Err(format!("更新地址不受信任：{}", params.apk_url));
    }
    let version = params.version.trim().to_string();
    if version.is_empty()
        || !version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return Err(format!("非法版本号：{version}"));
    }
    let cache_dir = app
        .path()
        .app_cache_dir()
        .map_err(|error| error.to_string())?;
    fs::create_dir_all(&cache_dir).map_err(|error| error.to_string())?;
    // 标准安卓更新流程：固定文件名覆盖下载，不留历史版本包。
    let dest = cache_dir.join("KXToDo.apk");
    let url = params.apk_url;
    std::thread::spawn(move || {
        let result = (|| -> Result<(), String> {
            let agent = update_agent();
            update_download_file(&app, &agent, "APK", &url, &dest)?;
            // 自检：错误页/JSON 响应体积远小于真实 APK，拦下明显存坏的下载。
            let size = fs::metadata(&dest).map(|meta| meta.len()).unwrap_or(0);
            if size < 1_000_000 {
                let _ = fs::remove_file(&dest);
                return Err(format!("APK 更新包异常（仅 {size} 字节），已中止"));
            }
            Ok(())
        })();
        match result {
            Ok(()) => {
                update_emit(&app, "update://applied", json!({ "path": dest }));
            }
            Err(message) => {
                update_emit(&app, "update://failed", json!({ "message": message }));
            }
        }
    });
    Ok(())
}

#[cfg(desktop)]
#[tauri::command]
fn reveal_main_window(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
    Ok(())
}

#[tauri::command]
fn open_url(app: AppHandle, url: String) -> Result<(), String> {
    if !(url.starts_with("http://") || url.starts_with("https://") || url.starts_with("mailto:")) {
        return Err("Unsupported link protocol".to_string());
    }
    tauri_plugin_opener::OpenerExt::opener(&app)
        .open_url(url, None::<&str>)
        .map_err(|error| error.to_string())
}

// ---------------------------------------------------------------------------
// v9 host wiring (desktop)
// ---------------------------------------------------------------------------

#[cfg(desktop)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum AppMode {
    Gui,
    HiddenHost,
}

// ---------------------------------------------------------------------------
// 内置同步服务器（设置里的「本机作为服务器」，v0.6.0）
// ---------------------------------------------------------------------------
//
// core 决定「该不该跑」（`HostCore::reconcile_sync_host` 读设置，挂在 Settings 域事件上），
// 这里只负责「怎么跑」：在**本进程**内起 lib 化的 kxtodo-server，跑在 tauri 的
// async_runtime（tokio）上。in-process 是唯一能同时满足三端的做法——Android 不能 exec
// 外部二进制（targetSdk≥29 的 W^X 限制），P2P 会话期的临时主机也不该每次拉一个子进程。
//
// 桌面与移动端后端都只是转发到下面这两个函数：机制完全相同，不写两份。

/// 进程内只有一个内置主机（与 IPC server 一样是进程级单例），所以用全局槽位。
///
/// 不放在 backend 结构里：`TauriBackend` 会被临时构造（单实例转发时），
/// 句柄跟着临时对象走容易搞错生命周期。
static SYNC_HOST: std::sync::OnceLock<std::sync::Mutex<Option<kxtodo_server::ServerHandle>>> =
    std::sync::OnceLock::new();

fn sync_host_slot() -> &'static std::sync::Mutex<Option<kxtodo_server::ServerHandle>> {
    SYNC_HOST.get_or_init(|| std::sync::Mutex::new(None))
}

/// 非阻塞启动内置主机：绑定与 serve 循环丢进 tauri 的运行时，句柄落进全局槽位，
/// 实际端口 / 库身份 / 管理台凭据写进 `runtime/sync-host.json`
/// （core 与另开的 CLI 进程都读那个文件来解析「本机就是主机时连哪儿」）。
fn start_embedded_sync_host(
    request: &domain::host::SyncHostRequest,
) -> Result<(), domain::CoreError> {
    let layout = domain::repo::Layout::new(request.data_dir.clone());
    // 管理台密码只在**首次生成**时拿得到明文（之后 server/settings.json 里只有哈希），
    // 所以重启（改名字/改端口）时要从旧描述符里带过去，否则用户再也看不到自己的密码。
    let previous = domain::sync::state::load_host_state(&layout);
    stop_embedded_sync_host(&request.data_dir);

    let config = kxtodo_server::ServerConfig {
        // 监听所有网卡：局域网内的其它设备要能连进来（回环监听不应答发现查询）
        listen: format!("0.0.0.0:{}", request.port),
        data_dir: domain::repo::embedded_server_dir(&request.data_dir),
        db: None,
        name: Some(request.name.clone()),
        admin: None,
        // 管理台凭据自动生成：用户勾一个框就该能用，不该先被逼着想一个密码
        admin_provision: kxtodo_server::AdminProvision::GenerateIfMissing,
        discovery: true,
        // 同机可能还跑着独立的 kxtodo-server，撞上端口就自动向上找
        // （发现应答带的是真实端口，客户端不受影响）
        port_fallback: kxtodo_server::host::DEFAULT_PORT_FALLBACK,
        retry_bind: false,
    };
    let host_name = request.name.clone();
    let configured_port = request.port;
    tauri::async_runtime::spawn(async move {
        let mut state = domain::sync::state::EmbeddedHostState {
            name: host_name,
            configured_port,
            pid: std::process::id(),
            ..Default::default()
        };
        match kxtodo_server::host::start(config).await {
            Ok(handle) => {
                state.running = true;
                state.port = handle.port();
                state.name = handle.name().to_string();
                state.instance_id = handle.instance_id.clone();
                state.admin_url = handle.admin_url();
                state.admin_user = handle.admin_user().to_string();
                state.admin_password = handle
                    .generated_admin_password
                    .clone()
                    .filter(|password| !password.is_empty())
                    .unwrap_or(previous.admin_password);
                state.started_at = Some(domain::time::now_iso());
                if let Ok(mut slot) = sync_host_slot().lock() {
                    *slot = Some(handle);
                }
            }
            Err(error) => state.last_error = Some(error.to_string()),
        }
        let _ = domain::sync::state::save_host_state(&layout, &state);
    });
    Ok(())
}

/// 停掉内置主机并清掉描述符（幂等）。
///
/// 只请求停机、不 await 服务循环结束：这个方法会在应用退出路径上被调用，
/// 等一个正在收尾的 axum 循环有可能把退出卡住。
fn stop_embedded_sync_host(data_dir: &std::path::Path) {
    if let Some(handle) = sync_host_slot().lock().ok().and_then(|mut slot| slot.take()) {
        handle.request_shutdown();
    }
    let layout = domain::repo::Layout::new(data_dir.to_path_buf());
    let _ = domain::sync::state::clear_host_state(&layout);
}

#[cfg(desktop)]
struct TauriBackend {
    app: AppHandle,
    allow_autostart: bool,
}

#[cfg(desktop)]
impl TauriBackend {
    fn show_or_create_main_window(&self) -> Result<(), String> {
        if let Some(window) = self.app.get_webview_window("main") {
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
            return Ok(());
        }
        create_main_window(&self.app)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

#[cfg(desktop)]
impl domain::host::HostBackend for TauriBackend {
    fn show_notification(
        &self,
        payload: &Value,
        _wait_rx: Option<std::sync::mpsc::Receiver<()>>,
    ) -> Result<String, domain::CoreError> {
        #[cfg(target_os = "linux")]
        {
            // Linux 用官方 notification 插件走系统通知守护进程（libnotify/D-Bus），
            // 时长/位置/字号等呈现细节由守护进程决定，只映射标题与正文。
            use tauri_plugin_notification::NotificationExt;
            let title = payload
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("KXToDo");
            let body = payload
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default();
            self.app
                .notification()
                .builder()
                .title(title)
                .body(body)
                .show()
                .map_err(|error| {
                    domain::CoreError::execution(
                        "NOTIFY_FAILED",
                        format!("系统通知发送失败：{error}"),
                    )
                })?;
            // 系统通知没有窗口关闭事件：core 在返回后才登记该 id，延迟把它关闭，
            // 否则隐藏 Host 看门狗视通知为活跃永不退出，wait=true 也会挂满超时。
            // id 必须唯一：NotificationTracker 按 id 键控，固定 id 会让并发通知
            // 互相覆盖登记，先到的 wait=true 丢失唤醒方只能挂满超时。
            static SYSTEM_NOTIFICATION_SEQ: AtomicU64 = AtomicU64::new(0);
            let id = format!(
                "system-notification-{}",
                SYSTEM_NOTIFICATION_SEQ.fetch_add(1, Ordering::SeqCst) + 1
            );
            if let Some(core) = self.app.try_state::<Arc<domain::host::HostCore>>() {
                let core = core.inner().clone();
                let closed_id = id.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    core.notifications.closed(&closed_id);
                });
            }
            return Ok(id);
        }
        #[cfg(not(target_os = "linux"))]
        {
            let request = NotificationRequest {
                title: payload
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or("KXToDo")
                    .to_string(),
                message: payload
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                duration_ms: payload
                    .get("durationMs")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                tone: payload
                    .get("tone")
                    .and_then(Value::as_str)
                    .unwrap_or("info")
                    .to_string(),
                position: payload
                    .get("position")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                title_font_size: payload
                    .get("titleFontSize")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0),
                body_font_size: payload
                    .get("bodyFontSize")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0),
            };
            show_notification_window(&self.app, request).map_err(|error| {
                domain::CoreError::execution("NOTIFY_FAILED", format!("通知窗口创建失败：{error}"))
            })
        }
    }

    fn emit(&self, event: &str, payload: Value) {
        use tauri::Emitter;
        let _ = self.app.emit(event, payload);
    }

    fn apply_native_effect(
        &self,
        name: &str,
        settings: &domain::model::SettingsFile,
    ) -> Result<(), domain::CoreError> {
        match name {
            "autostart" => {
                if !self.allow_autostart {
                    return Err(domain::CoreError::validation(
                        "CUSTOM_DATA_DIR_AUTOSTART_UNSUPPORTED",
                        "自定义 --data-dir 的 Host 不会注册系统开机启动",
                    ));
                }
                if settings.lifecycle.launch_at_startup {
                    self.app.autolaunch().enable()
                } else {
                    self.app.autolaunch().disable()
                }
                .map_err(|error| {
                    domain::CoreError::internal(format!("开机自启应用失败：{error}"))
                })?;
            }
            "closeToTray" => {
                self.app
                    .state::<LifecycleState>()
                    .close_to_tray
                    .store(settings.lifecycle.close_to_tray, Ordering::SeqCst);
            }
            "globalShortcut" => {
                register_global_toggle(&self.app, &settings.shortcuts.toggle_window).map_err(
                    |error| domain::CoreError::internal(format!("全局快捷键注册失败：{error}")),
                )?;
            }
            "webviewZoom" | "shortcuts" => {}
            _ => {}
        }
        Ok(())
    }

    fn request_exit(&self) {
        self.app
            .state::<LifecycleState>()
            .quitting
            .store(true, Ordering::SeqCst);
        self.app.exit(0);
    }

    fn has_gui(&self) -> bool {
        self.app.get_webview_window("main").is_some()
    }

    fn show_main_window(&self) -> Result<(), domain::CoreError> {
        self.show_or_create_main_window()
            .map_err(domain::CoreError::internal)
    }

    fn autostart_enabled(&self) -> Result<bool, domain::CoreError> {
        if !self.allow_autostart {
            return Ok(false);
        }
        self.app
            .autolaunch()
            .is_enabled()
            .map_err(|error| domain::CoreError::internal(format!("读取开机启动状态失败：{error}")))
    }

    fn sync_host_start(
        &self,
        request: &domain::host::SyncHostRequest,
    ) -> Result<(), domain::CoreError> {
        start_embedded_sync_host(request)
    }

    fn sync_host_stop(&self, data_dir: &std::path::Path) {
        stop_embedded_sync_host(data_dir)
    }
}

#[cfg(desktop)]
fn create_main_window(app: &AppHandle) -> tauri::Result<tauri::WebviewWindow> {
    WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("KXToDo")
        .inner_size(1180.0, 820.0)
        .min_inner_size(900.0, 640.0)
        .resizable(true)
        .fullscreen(false)
        .decorations(false)
        .shadow(true)
        .build()
}

#[cfg(desktop)]
fn init_host_core(
    app: &AppHandle,
    mode: AppMode,
    dir: PathBuf,
) -> Result<Arc<domain::host::HostCore>, String> {
    let dir = domain::ipc::normalize_data_dir(&dir);
    domain::host::stale_descriptor_cleanup(&dir);
    let repo = domain::repo::Repository::open(dir.clone()).map_err(|error| error.to_string())?;
    if let Err(error) = repo.load_all() {
        eprintln!("数据迁移失败：{error}");
    }
    if let Err(error) = repo.ensure_initialized() {
        eprintln!("数据初始化失败：{error}");
    }
    let custom_data_dir = !domain::ipc::same_data_dir(&dir, &default_data_dir());
    let allow_autostart = !custom_data_dir;
    let core = domain::host::HostCore::new(
        repo,
        dir,
        match mode {
            AppMode::Gui => "gui",
            AppMode::HiddenHost => "hidden",
        },
        custom_data_dir,
    );
    core.set_backend(Box::new(TauriBackend {
        app: app.clone(),
        allow_autostart,
    }));
    // manage 必须先于 IPC/恢复/调度启动：Linux 系统通知的延迟关闭经 try_state
    // 取 HostCore，状态未挂载的窗口期进来的通知会丢失关闭方（wait 挂满超时、
    // 看门狗视通知为活跃不退出）。
    app.manage(core.clone());
    let endpoint = domain::host::start_ipc_server(core.clone())
        .map_err(|error| format!("IPC/Host 所有权启动失败：{error}"))?;
    if let Ok(mut slot) = core.ipc_endpoint.write() {
        *slot = endpoint;
    }
    domain::host::retry_pending_recovery(&core);
    core.start_scheduler();
    // 「本机作为服务器」随应用启动恢复（隐藏 Host 模式也一样：那正是开机自启后常驻的进程）。
    // 之后设置一变就由 emit_domain_event(Domain::Settings) → reconcile_sync_host 跟进。
    core.reconcile_sync_host();
    Ok(core)
}

#[cfg(desktop)]
fn run_desktop_app(mode: AppMode, host_data_dir: PathBuf) {
    let host_data_dir = domain::ipc::normalize_data_dir(&host_data_dir);
    let default_host = domain::ipc::same_data_dir(&host_data_dir, &default_data_dir());
    let builder = tauri::Builder::default()
        .manage(LifecycleState::default())
        // 通知窗标签按进程内计数器复用（notification-0/1/…），window-state 会把
        // 历史"不可见/旧位置"状态恢复到新通知窗上，导致通知建了却看不见，必须排除。
        // VISIBLE 也不持久化：主窗口可见性由 visible:false + reveal_main_window 接管，
        // 否则恢复可见会让 WebView2 初始化的黑帧直接可见。
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_filter(|label| !label.starts_with("notification-"))
                .with_state_flags(StateFlags::all() & !StateFlags::VISIBLE)
                .build(),
        )
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--kxtodo-host"]),
        ))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        // 官方 os 插件：前端 capabilities 用同步 platform() 判定宿主系统（替代 UA 嗅探）。
        .plugin(tauri_plugin_os::init());
    // Linux 通知交给系统守护进程（libnotify/D-Bus）；Windows 保留自绘弹窗通知窗。
    #[cfg(target_os = "linux")]
    let builder = builder.plugin(tauri_plugin_notification::init());
    let builder = if default_host {
        builder.plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            let wants_host = args.iter().any(|arg| arg == "--kxtodo-host");
            if wants_host {
                return;
            }
            let backend = TauriBackend {
                app: app.clone(),
                allow_autostart: true,
            };
            let _ = backend.show_or_create_main_window();
        }))
    } else {
        builder
    };

    let mut context = tauri::generate_context!();
    if mode == AppMode::HiddenHost {
        for window in &mut context.config_mut().app.windows {
            window.create = false;
        }
    }

    let builder = builder
        .invoke_handler(tauri::generate_handler![
            resolve_executor_paths,
            resolve_executable_path,
            export_data,
            save_background_image,
            load_background_image,
            delete_background_image,
            import_background_image,
            background_image_path,
            image_data_url,
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
            notification_ready,
            register_global_shortcut,
            set_close_to_tray,
            set_autostart,
            get_autostart_enabled,
            set_webview_zoom,
            reveal_main_window,
            app_version,
            update_download_and_apply,
            open_url,
            core_dispatch,
            core_snapshot,
            core_ping
        ])
        .setup(move |app| {
            let core =
                init_host_core(app.handle(), mode, host_data_dir.clone()).map_err(|error| {
                    tauri::Error::Io(std::io::Error::new(std::io::ErrorKind::Other, error))
                })?;
            if mode == AppMode::HiddenHost {
                domain::host::start_idle_watchdog(core);
            }
            if let Some(webview) = app.get_webview_window("main") {
                let _ = webview.set_zoom(1.0);
            }
            if default_host {
                setup_tray(app)?;
            }
            if mode == AppMode::Gui {
                // 窗口创建时保持隐藏（conf visible:false），由前端首帧渲染后
                // invoke reveal_main_window 显示，避免 WebView2 初始化黑边。
                // 前端异常时 4 秒兜底强制显示，保证进程不成为无窗僵尸。
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(4));
                    if let Some(window) = handle.get_webview_window("main") {
                        if !window.is_visible().unwrap_or(true) {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                });
            }
            Ok(())
        })
        .on_window_event(move |window, event| {
            #[cfg(desktop)]
            {
                if window.label().starts_with("notification-") {
                    if let tauri::WindowEvent::CloseRequested { .. } = event {
                        if let Some(core) = window
                            .app_handle()
                            .try_state::<Arc<domain::host::HostCore>>()
                        {
                            core.notifications.closed(window.label());
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
                        if let Some(core) = window
                            .app_handle()
                            .try_state::<Arc<domain::host::HostCore>>()
                        {
                            domain::host::shutdown_host(&core);
                        }
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
        });
    let app = builder.build(context).expect("failed to build KXToDo");
    app.run(move |app_handle, event| {
        match event {
            // Hidden Host 的窗口（通知窗）全部关闭不应退出进程；退出只由看门狗/显式请求触发。
            tauri::RunEvent::ExitRequested { api, .. } => {
                #[cfg(desktop)]
                if mode == AppMode::HiddenHost
                    && !app_handle
                        .state::<LifecycleState>()
                        .quitting
                        .load(Ordering::SeqCst)
                {
                    api.prevent_exit();
                }
            }
            // 任何退出路径（托盘退出、看门狗自动退出、窗口关闭）都回收 Host 资源。
            tauri::RunEvent::Exit => {
                #[cfg(desktop)]
                if let Some(core) = app_handle.try_state::<Arc<domain::host::HostCore>>() {
                    domain::host::shutdown_host(&core);
                }
                let _ = app_handle;
            }
            _ => {}
        }
    });
}

/// Generic GUI → Domain Core bridge (§4.3): frontend submits business commands.
///
/// **必须是 async 命令**：Tauri 的同步命令跑在主线程上，而业务命令里可能有
/// 30 秒超时的 HTTP 同步请求、约 1 秒的 Argon2id 密钥派生——服务器掉线时
/// 打开设置界面就会把整个窗口卡住几秒。这里把执行丢进阻塞线程池，
/// 主线程只负责收发 IPC。
#[tauri::command]
async fn core_dispatch(
    core: State<'_, Arc<domain::host::HostCore>>,
    command: String,
    params: Value,
) -> Result<Value, String> {
    let host = core.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut invocation = domain::core::Invocation::new(command, params);
        // GUI 操作本身就是用户在界面上的确认行为，跳过 CLI 的 --yes 确认门。
        invocation.controls.yes = true;
        let ctx = domain::core::ExecContext {
            repo: &host.repo,
            cwd: host.data_dir.clone(),
            host: Some(host.as_ref()),
            custom_data_dir: host.custom_data_dir,
        };
        let outcome = domain::core::execute(&invocation, &ctx);
        if outcome.code == 0 {
            Ok(outcome.envelope)
        } else {
            Err(serde_json::to_string(&outcome.envelope).unwrap_or_default())
        }
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Capability probe that never reads business data. Desktop answers true even
/// when a domain file is corrupt so the frontend fails closed instead of using
/// legacy full-file writes.
#[tauri::command]
fn core_ping() -> Value {
    json!({ "available": true, "protocolVersion": domain::ipc::PROTOCOL_VERSION })
}

/// Snapshot read for GUI hydration (replaces full-file load_* paths).
/// 异步 + 阻塞线程池：每次领域变化都会调一次，读三个 JSON 也是磁盘 IO，别占主线程。
#[tauri::command]
async fn core_snapshot(core: State<'_, Arc<domain::host::HostCore>>) -> Result<Value, String> {
    let host = core.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let data = host.repo.load_data().map_err(|error| error.to_string())?;
        let settings = host
            .repo
            .load_settings()
            .map_err(|error| error.to_string())?;
        let schedule = host
            .repo
            .load_schedule()
            .map_err(|error| error.to_string())?;
        Ok(serde_json::json!({
            "data": data,
            "settings": settings,
            "schedule": schedule,
            "revisions": {
                "data": data.meta.revision,
                "settings": settings.meta.revision,
                "schedule": schedule.meta.revision,
            }
        }))
    })
    .await
    .map_err(|error| error.to_string())?
}

/// 内部启动参数：`--kxtodo-host [--data-dir <path>]` → 隐藏 Host 模式。
/// 由 CLI（notify/schedule run）或开机自启拉起；不是用户接口。
#[cfg(desktop)]
fn parse_host_mode_args(args: &[String]) -> Option<PathBuf> {
    if !args.iter().any(|arg| arg == "--kxtodo-host") {
        return None;
    }
    let mut data_dir = default_data_dir();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--data-dir" {
            if let Some(value) = iter.next() {
                data_dir = PathBuf::from(value);
            }
        } else if let Some(value) = arg.strip_prefix("--data-dir=") {
            data_dir = PathBuf::from(value);
        }
    }
    Some(data_dir)
}

// ---------------------------------------------------------------------------
// v0.2.0 mobile host wiring：进程内 HostCore（无 IPC 服务端、无调度器、
// 无看门狗、无托盘）。前端与桌面走同一条 core_dispatch 业务命令层。
// ---------------------------------------------------------------------------

#[cfg(not(desktop))]
struct MobileBackend {
    app: AppHandle,
}

#[cfg(not(desktop))]
impl domain::host::HostBackend for MobileBackend {
    fn show_notification(
        &self,
        _payload: &Value,
        _wait_rx: Option<std::sync::mpsc::Receiver<()>>,
    ) -> Result<String, domain::CoreError> {
        Err(domain::CoreError::execution(
            "NOTIFY_UNSUPPORTED",
            "移动端暂不支持后台通知",
        ))
    }

    fn emit(&self, event: &str, payload: Value) {
        use tauri::Emitter;
        let _ = self.app.emit(event, payload);
    }

    fn apply_native_effect(
        &self,
        _name: &str,
        _settings: &domain::model::SettingsFile,
    ) -> Result<(), domain::CoreError> {
        Ok(())
    }

    fn request_exit(&self) {}

    fn has_gui(&self) -> bool {
        true
    }

    fn show_main_window(&self) -> Result<(), domain::CoreError> {
        Ok(())
    }

    fn autostart_enabled(&self) -> Result<bool, domain::CoreError> {
        Ok(false)
    }

    fn sync_host_start(
        &self,
        request: &domain::host::SyncHostRequest,
    ) -> Result<(), domain::CoreError> {
        start_embedded_sync_host(request)
    }

    fn sync_host_stop(&self, data_dir: &std::path::Path) {
        stop_embedded_sync_host(data_dir)
    }
}

#[cfg(not(desktop))]
fn init_mobile_core(app: &AppHandle) -> Result<(), String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    ensure_storage_layout(app)?;
    let repo = domain::repo::Repository::open(dir.clone()).map_err(|error| error.to_string())?;
    if let Err(error) = repo.load_all() {
        eprintln!("数据迁移失败：{error}");
    }
    if let Err(error) = repo.ensure_initialized() {
        eprintln!("数据初始化失败：{error}");
    }
    let core = domain::host::HostCore::new(repo, dir, "gui", false);
    core.set_backend(Box::new(MobileBackend { app: app.clone() }));
    domain::host::retry_pending_recovery(&core);
    // 「本机作为服务器」在 Android 上同样是 in-process 起（不能 exec 外部二进制）。
    // 只在应用前台可靠：Doze 会掐掉后台监听，本期不做前台服务保活（设置页有说明）。
    core.reconcile_sync_host();
    app.manage(core);
    Ok(())
}

/// AppImage 的 AppRun（linuxdeploy-plugin-gtk）为绕旧版 WebKitGTK 的 Wayland 崩溃
/// （tauri#8541）强制 GDK_BACKEND=x11；但 WSLg 的 XWayland 光标通路是坏的（实测任何
/// X11 客户端都丢鼠标光标，Wayland 原生一切正常）。Wayland 会话里撤掉这个强制值回
/// 原生后端；纯 X11 会话（无 WAYLAND_DISPLAY）保持 x11 不动。
#[cfg(target_os = "linux")]
fn restore_wayland_backend_in_appimage() {
    let forced_x11 = std::env::var_os("GDK_BACKEND").as_deref()
        == Some(std::ffi::OsStr::new("x11"));
    if std::env::var_os("APPDIR").is_some()
        && std::env::var_os("WAYLAND_DISPLAY").is_some()
        && forced_x11
    {
        std::env::remove_var("GDK_BACKEND");
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(desktop)]
    {
        #[cfg(target_os = "linux")]
        restore_wayland_backend_in_appimage();
        let args: Vec<String> = env::args().skip(1).collect();
        match parse_host_mode_args(&args) {
            Some(data_dir) => run_desktop_app(AppMode::HiddenHost, data_dir),
            None => run_desktop_app(AppMode::Gui, default_data_dir()),
        }
    }

    #[cfg(not(desktop))]
    {
        let context = tauri::generate_context!();
        tauri::Builder::default()
            .plugin(tauri_plugin_opener::init())
            .plugin(tauri_plugin_dialog::init())
            .plugin(tauri_plugin_notification::init())
            .invoke_handler(tauri::generate_handler![
                core_dispatch,
                core_snapshot,
                core_ping,
                app_version,
                open_url,
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
                image_data_url,
                update_download_apk
            ])
            .setup(move |app| {
                init_mobile_core(app.handle()).map_err(|error| {
                    tauri::Error::Io(std::io::Error::new(std::io::ErrorKind::Other, error))
                })?;
                Ok(())
            })
            .run(context)
            .expect("failed to run KXToDo");
    }
}
