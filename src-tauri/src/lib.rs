use base64::Engine;
use serde_json::{json, Value};
use std::{
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

fn images_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = data_dir(app)?.join("images");
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(dir)
}

fn avatar_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = data_dir(app)?.join("avatar");
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(dir)
}

fn md_images_dir(app: &AppHandle, node_id: &str) -> Result<PathBuf, String> {
    if node_id.is_empty() || node_id.contains('/') || node_id.contains('\\') || node_id.contains("..") {
        return Err("Invalid node id".to_string());
    }
    let dir = data_dir(app)?.join("img").join(node_id);
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(dir)
}

fn data_file(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(data_dir(app)?.join("data.json"))
}

fn settings_file(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(data_dir(app)?.join("settings.json"))
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
    let dir = data_dir(&app)?.join("img").join(&node_id);
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
            register_global_shortcut,
            set_close_to_tray,
            set_autostart,
            get_autostart_enabled,
            set_webview_zoom,
            open_url
        ])
        .setup(|app| {
            let _ = ensure_data_dir(app.handle());
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
