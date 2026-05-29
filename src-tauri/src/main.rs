#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde_json::{json, Value};
use std::{fs, path::PathBuf, process::Command};
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            return Ok(dir.join("todo-note-data"));
        }
    }

    app.path().app_data_dir().map_err(|error| error.to_string())
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

#[tauri::command]
fn register_global_shortcut(app: AppHandle, shortcut: String) -> Result<(), String> {
    register_global_toggle(&app, &shortcut)
}

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
        return Err("Opening links in the system browser is not supported on this platform".to_string());
    }

    result.map(|_| ()).map_err(|error| error.to_string())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            load_state,
            save_state,
            load_settings,
            save_settings,
            export_data,
            register_global_shortcut,
            open_url
        ])
        .setup(|app| {
            if let Some(webview) = app.get_webview_window("main") {
                let _ = webview.show();
                let _ = webview.set_focus();
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("failed to run Todo Note");
}
