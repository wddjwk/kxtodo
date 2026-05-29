#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

#[derive(Debug, Deserialize)]
struct ExportRequest {
    scope: String,
    #[serde(rename = "listId")]
    list_id: Option<String>,
    state: Value,
    #[serde(rename = "outputPath")]
    output_path: Option<String>,
}

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

fn exported_at() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    format!("{seconds}")
}

fn value_id(value: &Value, key: &str) -> Option<String> {
    value.get(key)?.as_str().map(ToOwned::to_owned)
}

fn descendant_ids(state: &Value, root_id: &str) -> HashSet<String> {
    let mut ids = HashSet::new();
    ids.insert(root_id.to_string());

    let Some(lists) = state.get("lists").and_then(Value::as_array) else {
        return ids;
    };

    let mut changed = true;
    while changed {
        changed = false;
        for list in lists {
            let Some(id) = value_id(list, "id") else {
                continue;
            };
            let Some(parent_id) = value_id(list, "parentId") else {
                continue;
            };
            if ids.contains(&parent_id) && !ids.contains(&id) {
                ids.insert(id);
                changed = true;
            }
        }
    }

    ids
}

fn export_payload(request: &ExportRequest) -> Value {
    if request.scope == "all" {
        return json!({
            "schemaVersion": 3,
            "exportedAt": exported_at(),
            "scope": "all",
            "state": request.state
        });
    }

    let root_id = request
        .list_id
        .clone()
        .or_else(|| value_id(&request.state, "selectedListId"))
        .unwrap_or_else(|| "quick-notes".to_string());
    let ids = descendant_ids(&request.state, &root_id);

    let lists = request
        .state
        .get("lists")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter(|item| value_id(item, "id").is_some_and(|id| ids.contains(&id)))
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let tasks = request
        .state
        .get("tasks")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter(|item| value_id(item, "listId").is_some_and(|id| ids.contains(&id)))
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    json!({
        "schemaVersion": 3,
        "exportedAt": exported_at(),
        "scope": "list",
        "rootListId": root_id,
        "lists": lists,
        "tasks": tasks
    })
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
fn export_data(app: AppHandle, request: ExportRequest) -> Result<String, String> {
    let filename = if request.scope == "all" {
        "todo-note-all-export.json".to_string()
    } else {
        format!(
            "todo-note-{}-export.json",
            request
                .list_id
                .clone()
                .or_else(|| value_id(&request.state, "selectedListId"))
                .unwrap_or_else(|| "list".to_string())
        )
    };
    let output_path = request
        .output_path
        .clone()
        .filter(|path| !path.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or(data_dir(&app)?.join(filename));

    write_json(output_path.clone(), export_payload(&request))?;
    Ok(output_path.to_string_lossy().to_string())
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
            register_global_shortcut
        ])
        .setup(|app| {
            let webview = WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
                .title("Todo Note")
                .inner_size(1180.0, 820.0)
                .min_inner_size(940.0, 640.0)
                .decorations(false)
                .resizable(true)
                .build()?;

            let _ = webview.set_focus();
            let _ = register_global_toggle(&app.handle(), "Ctrl+Shift+Space");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
