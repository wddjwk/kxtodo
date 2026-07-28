//! Background Host: single-instance owner of IPC, notifications and the
//! scheduler (§4.4), plus CLI routing (host IPC vs standalone execution).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};

use crate::domain::cli::Routing;
use crate::domain::core::{execute, ExecContext, ExecOutcome, HostServices, Invocation};
use crate::domain::error::{CoreError, CoreResult, ErrorKind};
use crate::domain::ids::request_id;
use crate::domain::ipc::{
    discover_host, endpoint_for, outcome_envelope, read_host_descriptor, remove_host_descriptor,
    validate_request, write_host_descriptor, HostDescriptor, IpcClient, IpcRequest, IpcServer,
    PROTOCOL_VERSION,
};
use crate::domain::model::{Notification, SettingsFile};
use crate::domain::repo::{Domain, Repository};
use crate::domain::scheduler::{render_notification, SchedulerHandle};
use crate::domain::time::now_iso;

// ---------------------------------------------------------------------------
// Host backend (window/system capabilities; headless in tests)
// ---------------------------------------------------------------------------

pub trait HostBackend: Send + Sync {
    /// Show a notification window; returns its id. `wait_rx` completes on close.
    fn show_notification(
        &self,
        payload: &Value,
        wait_rx: Option<Receiver<()>>,
    ) -> CoreResult<String>;
    /// Emit an event to GUI clients (Tauri emit or test recorder).
    fn emit(&self, event: &str, payload: Value);
    /// Apply native side effects.
    fn apply_native_effect(&self, name: &str, settings: &SettingsFile) -> CoreResult<()>;
    /// Ask the host process to exit (idle auto-exit).
    fn request_exit(&self);
    /// Whether a GUI main window is present in this host.
    fn has_gui(&self) -> bool;
    /// Create/show the main window (no-arg launch forwarded to a hidden host).
    fn show_main_window(&self) -> CoreResult<()>;
    /// Query actual OS autostart registration when available.
    fn autostart_enabled(&self) -> CoreResult<bool> {
        Err(CoreError::internal(
            "当前 Host backend 不支持查询开机启动状态",
        ))
    }
}

// ---------------------------------------------------------------------------
// HostCore
// ---------------------------------------------------------------------------

pub struct HostCore {
    pub repo: Repository,
    pub data_dir: PathBuf,
    pub processes: crate::domain::exec::ProcessRegistry,
    pub backend: RwLock<Option<Box<dyn HostBackend>>>,
    pub mode: String,
    pub scheduler: RwLock<Option<SchedulerHandle>>,
    pub notifications: NotificationTracker,
    pub started_at: String,
    pub ipc_endpoint: RwLock<String>,
    pub ipc_token: String,
    pub owner_lock: Mutex<Option<crate::domain::repo::HostOwnerLock>>,
}

impl HostCore {
    pub fn new(repo: Repository, data_dir: PathBuf, mode: &str) -> Arc<Self> {
        Arc::new(Self {
            repo,
            data_dir: crate::domain::ipc::normalize_data_dir(&data_dir),
            processes: Default::default(),
            backend: RwLock::new(None),
            mode: mode.to_string(),
            scheduler: RwLock::new(None),
            notifications: NotificationTracker::default(),
            started_at: now_iso(),
            ipc_endpoint: RwLock::new(String::new()),
            ipc_token: crate::domain::ipc::generate_token(),
            owner_lock: Mutex::new(None),
        })
    }

    pub fn set_backend(&self, backend: Box<dyn HostBackend>) {
        if let Ok(mut slot) = self.backend.write() {
            *slot = Some(backend);
        }
    }

    pub fn start_scheduler(self: &Arc<Self>) {
        let handle = crate::domain::scheduler::start(self.clone());
        if let Ok(mut slot) = self.scheduler.write() {
            *slot = Some(handle);
        }
    }

    pub fn notify_scheduler_reload(&self) {
        if let Ok(slot) = self.scheduler.read() {
            if let Some(handle) = slot.as_ref() {
                handle.reload();
            }
        }
    }

    pub fn emit_domain_event(&self, domain: Domain, revision: u64, ids: Vec<String>) {
        if matches!(domain, Domain::Schedule) {
            self.notify_scheduler_reload();
        }
        if let Ok(backend) = self.backend.read() {
            if let Some(backend) = backend.as_ref() {
                backend.emit(
                    "kxtodo://domain-changed",
                    json!({
                        "domain": domain.as_str(),
                        "revision": revision,
                        "ids": ids,
                    }),
                );
            }
        }
    }

    /// Render + inherit settings, then show a notification (§3.3 dynamic inheritance).
    pub fn show_notification_rendered(
        &self,
        notification: &Notification,
        task_name: &str,
        vars: &[(&str, &str)],
    ) -> CoreResult<Value> {
        let rendered = render_notification(notification, task_name, vars);
        let payload = self.resolve_notification_payload(
            rendered.title.as_deref(),
            &rendered.message,
            rendered.duration.as_deref(),
            rendered.tone.map(|tone| tone.as_str()),
            rendered.position.map(|position| position.as_str()),
            false,
        )?;
        self.show_notification_payload(payload)
    }

    /// Merge a single-shot payload with current settings defaults.
    pub fn resolve_notification_payload(
        &self,
        title: Option<&str>,
        message: &str,
        duration: Option<&str>,
        tone: Option<&str>,
        position: Option<&str>,
        wait: bool,
    ) -> CoreResult<Value> {
        let settings = self.repo.load_settings()?;
        let duration_ms = match duration {
            Some(raw) => crate::domain::time::parse_duration_ms(raw)?,
            None => settings.notifications.duration_ms,
        };
        if !(1_200..=60_000).contains(&duration_ms) {
            return Err(CoreError::validation(
                "DURATION_OUT_OF_RANGE",
                "通知时长必须在 1200ms 到 60000ms 之间",
            ));
        }
        Ok(json!({
            "title": title.filter(|value| !value.trim().is_empty()).unwrap_or("KXToDo"),
            "message": if message.trim().is_empty() { "通知" } else { message },
            "durationMs": duration_ms,
            "tone": tone.unwrap_or("info"),
            "position": position
                .map(str::to_string)
                .unwrap_or_else(|| settings.notifications.position.as_str().to_string()),
            "width": settings.notifications.width,
            "height": settings.notifications.height,
            "titleFontSize": settings.notifications.title_font_size,
            "bodyFontSize": settings.notifications.body_font_size,
            "wait": wait,
        }))
    }

    pub fn show_notification_payload(&self, payload: Value) -> CoreResult<Value> {
        let wait = payload
            .get("wait")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let (wait_tx, wait_rx) = if wait {
            let (tx, rx) = channel::<()>();
            (Some(tx), Some(rx))
        } else {
            (None, None)
        };
        let id = {
            let backend = self
                .backend
                .read()
                .map_err(|error| CoreError::internal(format!("backend lock：{error}")))?;
            let backend = backend.as_ref().ok_or_else(|| {
                CoreError::new(
                    ErrorKind::Execution,
                    "NO_DISPLAY",
                    "当前 Host 无法显示通知窗口",
                )
            })?;
            backend.show_notification(&payload, None)?
        };
        if let Some(tx) = wait_tx {
            self.notifications.register_waiter(&id, tx);
            // Block until the window closes (or a generous safety timeout).
            if let Some(rx) = wait_rx {
                let _ = rx.recv_timeout(Duration::from_secs(120));
            }
        } else {
            self.notifications.track(&id);
        }
        Ok(json!({
            "notificationId": id,
            "delivered": true,
            "wait": wait,
        }))
    }

    /// Idle check for hidden hosts (§4.4): no notifications, no GUI, no enabled
    /// schedules, no running children → exit.
    pub fn hidden_host_should_exit(&self) -> bool {
        if self.notifications.active_count() > 0 {
            return false;
        }
        if !self.processes.running_ids().is_empty() {
            return false;
        }
        if self
            .scheduler
            .read()
            .ok()
            .and_then(|slot| {
                slot.as_ref()
                    .map(|scheduler| !scheduler.running_ids().is_empty())
            })
            .unwrap_or(false)
        {
            return false;
        }
        if let Ok(backend) = self.backend.read() {
            if let Some(backend) = backend.as_ref() {
                if backend.has_gui() {
                    return false;
                }
            }
        }
        if let Ok(file) = self.repo.load_schedule() {
            if file.tasks.iter().any(|entry| entry.spec.enabled) {
                return false;
            }
        }
        true
    }
}

impl HostServices for HostCore {
    fn show_notification(&self, payload: Value) -> CoreResult<Value> {
        let merged = self.resolve_notification_payload(
            payload.get("title").and_then(Value::as_str),
            payload.get("message").and_then(Value::as_str).unwrap_or(""),
            payload.get("duration").and_then(Value::as_str),
            payload.get("tone").and_then(Value::as_str),
            payload.get("position").and_then(Value::as_str),
            payload
                .get("wait")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        )?;
        self.show_notification_payload(merged)
    }

    fn apply_native_effect(&self, name: &str, settings: &SettingsFile) -> CoreResult<()> {
        let backend = self
            .backend
            .read()
            .map_err(|error| CoreError::internal(format!("backend lock：{error}")))?;
        let backend = backend
            .as_ref()
            .ok_or_else(|| CoreError::internal("无 Host backend"))?;
        backend.apply_native_effect(name, settings)
    }

    fn run_schedule_now(&self, id: &str, wait: bool) -> CoreResult<Value> {
        let scheduler = self
            .scheduler
            .read()
            .map_err(|error| CoreError::internal(format!("scheduler lock：{error}")))?;
        let scheduler = scheduler.as_ref().ok_or_else(|| {
            CoreError::new(
                ErrorKind::Execution,
                "SCHEDULER_UNAVAILABLE",
                "调度器不可用",
            )
        })?;
        scheduler.run_now(id, wait)
    }

    fn stop_schedule(&self, id: &str) -> CoreResult<Value> {
        let scheduler = self
            .scheduler
            .read()
            .map_err(|error| CoreError::internal(format!("scheduler lock：{error}")))?;
        let scheduler = scheduler.as_ref().ok_or_else(|| {
            CoreError::new(
                ErrorKind::Execution,
                "SCHEDULER_UNAVAILABLE",
                "调度器不可用",
            )
        })?;
        scheduler.stop(id)
    }

    fn autostart_status(&self) -> Option<bool> {
        self.backend.read().ok().and_then(|backend| {
            backend
                .as_ref()
                .and_then(|backend| backend.autostart_enabled().ok())
        })
    }

    fn host_status(&self) -> Value {
        let has_gui = self
            .backend
            .read()
            .ok()
            .and_then(|backend| backend.as_ref().map(|b| b.has_gui()))
            .unwrap_or(false);
        json!({
            "state": "running",
            "mode": self.mode,
            "pid": std::process::id(),
            "hasGui": has_gui,
            "startedAt": self.started_at,
            "endpoint": self.ipc_endpoint.read().map(|value| value.clone()).unwrap_or_default(),
            "protocolVersion": PROTOCOL_VERSION,
        })
    }

    fn emit_domain_event(&self, domain: Domain, revision: u64, ids: Vec<String>) {
        HostCore::emit_domain_event(self, domain, revision, ids);
    }
}

#[derive(Default)]
pub struct NotificationTracker {
    active: Mutex<HashMap<String, Option<Sender<()>>>>,
}

impl NotificationTracker {
    pub fn track(&self, id: &str) {
        if let Ok(mut active) = self.active.lock() {
            active.insert(id.to_string(), None);
        }
    }

    pub fn register_waiter(&self, id: &str, tx: Sender<()>) {
        if let Ok(mut active) = self.active.lock() {
            active.insert(id.to_string(), Some(tx));
        }
    }

    pub fn closed(&self, id: &str) {
        if let Ok(mut active) = self.active.lock() {
            if let Some(waiter) = active.remove(id) {
                if let Some(tx) = waiter {
                    let _ = tx.send(());
                }
            }
        }
    }

    pub fn active_count(&self) -> usize {
        self.active.lock().map(|active| active.len()).unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// CLI routing
// ---------------------------------------------------------------------------

fn requires_host(inv: &Invocation) -> bool {
    match inv.command.as_str() {
        "notify" | "schedule.run" | "schedule.stop" | "schedule.enable" => true,
        "schedule.add" => inv
            .params
            .get("spec")
            .and_then(|spec| spec.get("enabled"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
        "schedule.modify" => inv
            .params
            .get("patch")
            .and_then(|patch| patch.get("enabled"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
        _ => false,
    }
}

pub fn route(inv: &Invocation, data_dir: &Path, cwd: &Path, routing: Routing) -> ExecOutcome {
    let normalized_data_dir = crate::domain::ipc::normalize_data_dir(data_dir);
    let data_dir = normalized_data_dir.as_path();
    // IPC-host special commands never leave the host process.
    if routing == Routing::Auto {
        if let Some((descriptor, _)) = discover_host(data_dir) {
            return invoke_via_ipc(&descriptor, inv, data_dir, cwd).unwrap_or_else(|error| {
                ExecOutcome {
                    code: error.exit_code(),
                    envelope: crate::domain::envelope::failure(
                        &inv.command,
                        &error,
                        crate::domain::envelope::Meta::default(),
                    ),
                }
            });
        }
        if requires_host(inv) {
            return match launch_hidden_host(data_dir) {
                Ok(descriptor) => {
                    invoke_via_ipc(&descriptor, inv, data_dir, cwd).unwrap_or_else(|error| {
                        ExecOutcome {
                            code: error.exit_code(),
                            envelope: crate::domain::envelope::failure(
                                &inv.command,
                                &error,
                                crate::domain::envelope::Meta::default(),
                            ),
                        }
                    })
                }
                Err(error) => ExecOutcome {
                    code: error.exit_code(),
                    envelope: crate::domain::envelope::failure(
                        &inv.command,
                        &error,
                        crate::domain::envelope::Meta::default(),
                    ),
                },
            };
        }
        if inv.command == "doctor" {
            return execute_standalone(inv, data_dir, cwd);
        }
        // Standalone data CRUD under the launch mutex with a host re-check (§4.4).
        return with_launch_mutex(data_dir, || {
            if let Some((descriptor, _)) = discover_host(data_dir) {
                return invoke_via_ipc(&descriptor, inv, data_dir, cwd).unwrap_or_else(|error| {
                    ExecOutcome {
                        code: error.exit_code(),
                        envelope: crate::domain::envelope::failure(
                            &inv.command,
                            &error,
                            crate::domain::envelope::Meta::default(),
                        ),
                    }
                });
            }
            execute_standalone(inv, data_dir, cwd)
        });
    }
    execute_standalone(inv, data_dir, cwd)
}

fn execute_standalone(inv: &Invocation, data_dir: &Path, cwd: &Path) -> ExecOutcome {
    let repo = if inv.command == "doctor" {
        Repository::open_readonly(data_dir.to_path_buf())
    } else {
        match Repository::open(data_dir.to_path_buf()) {
            Ok(repo) => repo,
            Err(error) => {
                return ExecOutcome {
                    code: error.exit_code(),
                    envelope: crate::domain::envelope::failure(
                        &inv.command,
                        &error,
                        crate::domain::envelope::Meta::default(),
                    ),
                }
            }
        }
    };
    // Run pending migrations before any operation. doctor is exempt: it must
    // be able to report corrupted/unmigrated data instead of failing up front.
    if inv.command != "doctor" {
        if let Err(error) = repo.load_all() {
            return ExecOutcome {
                code: error.exit_code(),
                envelope: crate::domain::envelope::failure(
                    &inv.command,
                    &error,
                    crate::domain::envelope::Meta::default(),
                ),
            };
        }
    }
    let ctx = ExecContext {
        repo: &repo,
        cwd: crate::domain::ipc::normalize_absolute_path(
            cwd,
            &std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        ),
        host: None,
        custom_data_dir: !is_default_data_dir(data_dir),
    };
    execute(inv, &ctx)
}

pub fn is_default_data_dir(data_dir: &Path) -> bool {
    crate::domain::ipc::same_data_dir(data_dir, &crate::domain::cli::default_data_dir())
}

/// Serialize host launches and standalone fallbacks (§4.4 启动互斥).
fn with_launch_mutex<F>(data_dir: &Path, f: F) -> ExecOutcome
where
    F: FnOnce() -> ExecOutcome,
{
    let runtime_dir = data_dir.join(crate::domain::repo::RUNTIME_DIR);
    let _ = std::fs::create_dir_all(&runtime_dir);
    let lock_path = runtime_dir.join(crate::domain::repo::HOST_LAUNCH_LOCK);
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&lock_path)
        .and_then(|file| {
            use fs2::FileExt;
            file.lock_exclusive().map(|_| file)
        });
    match lock {
        Ok(_file) => f(),
        Err(error) => ExecOutcome {
            code: 5,
            envelope: crate::domain::envelope::failure(
                "host",
                &CoreError::io(format!("无法取得 Host 启动互斥：{error}")),
                crate::domain::envelope::Meta::default(),
            ),
        },
    }
}

fn invoke_via_ipc(
    descriptor: &HostDescriptor,
    inv: &Invocation,
    data_dir: &Path,
    cwd: &Path,
) -> CoreResult<ExecOutcome> {
    let mut client = IpcClient::connect(&descriptor.endpoint)?;
    let normalized_data_dir = crate::domain::ipc::normalize_data_dir(data_dir);
    let normalized_cwd = crate::domain::ipc::normalize_absolute_path(
        cwd,
        &std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    );
    let request = IpcRequest {
        protocol_version: PROTOCOL_VERSION,
        request_id: request_id(),
        data_dir: normalized_data_dir.to_string_lossy().to_string(),
        cwd: normalized_cwd.to_string_lossy().to_string(),
        token: descriptor.token.clone(),
        command: inv.command.clone(),
        params: inv.params.clone(),
        controls: crate::domain::ipc::IpcControls::from(&inv.controls),
    };
    let envelope = client.invoke(&request)?;
    let code = envelope
        .get("meta")
        .and_then(|meta| meta.get("exitCode"))
        .and_then(Value::as_i64)
        .map(|code| code as i32)
        .unwrap_or(
            if envelope.get("ok").and_then(Value::as_bool).unwrap_or(false) {
                0
            } else {
                1
            },
        );
    let mut envelope = envelope;
    if let Some(meta) = envelope.get_mut("meta").and_then(Value::as_object_mut) {
        meta.remove("exitCode");
    }
    Ok(ExecOutcome { code, envelope })
}

/// Spawn this executable as a hidden Host and wait for readiness.
fn launch_hidden_host(data_dir: &Path) -> CoreResult<HostDescriptor> {
    with_launch_mutex_plain(data_dir, || {
        // Re-check inside the mutex: another launcher may have won the race.
        if let Some((descriptor, _)) = discover_host(data_dir) {
            return Ok(descriptor);
        }
        let exe = std::env::current_exe()?;
        let mut command = std::process::Command::new(exe);
        command.arg("--kxtodo-host").arg("--data-dir").arg(data_dir);
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        command.spawn().map_err(|error| {
            CoreError::new(
                ErrorKind::Execution,
                "HOST_LAUNCH_FAILED",
                format!("无法启动 Background Host：{error}"),
            )
        })?;
        // Wait for the descriptor + live endpoint.
        for _ in 0..100 {
            thread::sleep(Duration::from_millis(100));
            if let Some((descriptor, _)) = discover_host(data_dir) {
                if let Ok(mut client) = IpcClient::connect(&descriptor.endpoint) {
                    if client.ping(data_dir, &descriptor.token) {
                        return Ok(descriptor);
                    }
                }
            }
        }
        Err(CoreError::new(
            ErrorKind::Execution,
            "HOST_NOT_READY",
            "Background Host 启动超时",
        ))
    })
}

fn with_launch_mutex_plain<T, F>(data_dir: &Path, f: F) -> CoreResult<T>
where
    F: FnOnce() -> CoreResult<T>,
{
    let runtime_dir = data_dir.join(crate::domain::repo::RUNTIME_DIR);
    std::fs::create_dir_all(&runtime_dir)?;
    let lock_path = runtime_dir.join(crate::domain::repo::HOST_LAUNCH_LOCK);
    let file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&lock_path)?;
    use fs2::FileExt;
    file.lock_exclusive()?;
    f()
}

// ---------------------------------------------------------------------------
// Host server (shared by GUI host and hidden host)
// ---------------------------------------------------------------------------

pub fn descriptor_for(data_dir: &Path, mode: &str, token: &str) -> HostDescriptor {
    let data_dir = crate::domain::ipc::normalize_data_dir(data_dir);
    HostDescriptor {
        protocol_version: PROTOCOL_VERSION,
        pid: std::process::id(),
        data_dir: data_dir.to_string_lossy().to_string(),
        endpoint: endpoint_for(&data_dir),
        mode: mode.to_string(),
        started_at: now_iso(),
        token: token.to_string(),
    }
}

/// Start the IPC accept loop on a background thread. Returns the bound endpoint.
pub fn start_ipc_server(core: Arc<HostCore>) -> CoreResult<String> {
    let owner = crate::domain::repo::HostOwnerLock::acquire(&core.repo.layout)?;
    let server = IpcServer::bind(&core.data_dir)?;
    *core
        .owner_lock
        .lock()
        .map_err(|error| CoreError::internal(format!("Host owner lock：{error}")))? = Some(owner);
    let endpoint = server.endpoint.clone();
    let data_dir = core.data_dir.clone();
    write_host_descriptor(
        &core.repo.layout.host_descriptor(),
        &descriptor_for(&data_dir, &core.mode, &core.ipc_token),
    )?;
    thread::Builder::new()
        .name("kxtodo-ipc".to_string())
        .spawn(move || loop {
            match server.accept_raw() {
                Ok(mut stream) => {
                    let core = core.clone();
                    thread::spawn(move || {
                        let _ =
                            crate::domain::ipc::serve_connection(&mut stream, &move |request| {
                                handle_ipc_request(&core, request)
                            });
                    });
                }
                Err(_) => thread::sleep(Duration::from_millis(200)),
            }
        })
        .map_err(|error| CoreError::internal(format!("无法启动 IPC 线程：{error}")))?;
    Ok(endpoint)
}

fn handle_ipc_request(core: &Arc<HostCore>, request: IpcRequest) -> Value {
    if let Err(error) = validate_request(&request, &core.data_dir, &core.ipc_token) {
        return crate::domain::envelope::failure(
            &request.command,
            &error,
            crate::domain::envelope::Meta::default(),
        );
    }
    // Host-internal commands.
    match request.command.as_str() {
        "host.ping" => {
            return json!({
                "ok": true,
                "command": "host.ping",
                "data": { "pong": true, "mode": core.mode, "pid": std::process::id() },
                "meta": { "requestId": request.request_id },
            })
        }
        "host.show" => {
            let backend = core.backend.read().ok();
            let result = backend.and_then(|backend| backend.as_ref().map(|b| b.show_main_window()));
            return match result {
                Some(Ok(())) => json!({
                    "ok": true,
                    "command": "host.show",
                    "data": { "shown": true },
                    "meta": { "requestId": request.request_id },
                }),
                Some(Err(error)) => crate::domain::envelope::failure(
                    "host.show",
                    &error,
                    crate::domain::envelope::Meta::default(),
                ),
                None => crate::domain::envelope::failure(
                    "host.show",
                    &CoreError::internal("Host backend 不可用"),
                    crate::domain::envelope::Meta::default(),
                ),
            };
        }
        _ => {}
    }
    let invocation = Invocation {
        command: request.command.clone(),
        params: request.params.clone(),
        controls: crate::domain::core::Controls {
            dry_run: request.controls.dry_run,
            yes: request.controls.yes,
            idempotency_key: request.controls.idempotency_key.clone(),
            if_revision: request.controls.if_revision,
        },
    };
    let cwd = PathBuf::from(&request.cwd);
    let ctx = ExecContext {
        repo: &core.repo,
        cwd,
        host: Some(core.as_ref()),
        custom_data_dir: !is_default_data_dir(&core.data_dir),
    };
    let outcome = execute(&invocation, &ctx);
    outcome_envelope(&outcome)
}

/// Idle watchdog for hidden hosts (§4.4 自动退出).
pub fn start_idle_watchdog(core: Arc<HostCore>) {
    thread::Builder::new()
        .name("kxtodo-idle".to_string())
        .spawn(move || loop {
            thread::sleep(Duration::from_secs(2));
            if core.hidden_host_should_exit() {
                let backend = core.backend.read().ok();
                if let Some(guard) = backend.as_ref() {
                    if let Some(backend) = guard.as_ref() {
                        backend.request_exit();
                    }
                }
                return;
            }
        })
        .ok();
}

/// Cleanup on host shutdown.
pub fn shutdown_host(core: &Arc<HostCore>) {
    let scheduler = core
        .scheduler
        .read()
        .ok()
        .and_then(|slot| slot.as_ref().cloned());
    if let Some(handle) = scheduler {
        handle.shutdown();
    }
    remove_host_descriptor(&core.repo.layout.host_descriptor());
    if let Ok(mut owner) = core.owner_lock.lock() {
        owner.take();
    }
}

/// Retry committed attachment cleanup records. Paths are reconstructed from
/// entry ids under img/data; persisted arbitrary paths are never trusted.
pub fn retry_pending_recovery(core: &Arc<HostCore>) {
    let Ok(records) = core.repo.read_recovery_records() else {
        return;
    };
    let data = core.repo.load_data().ok();
    for record in records {
        if record.get("kind").and_then(Value::as_str) != Some("delete-entry-images") {
            continue;
        }
        let Some(id) = record.get("id").and_then(Value::as_str) else {
            continue;
        };
        let entry_ids: Vec<String> = record
            .get("pendingPaths")
            .or_else(|| record.get("entryIds"))
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .filter(|entry_id| {
                        !entry_id.is_empty()
                            && !entry_id.contains('/')
                            && !entry_id.contains('\\')
                            && *entry_id != "."
                            && *entry_id != ".."
                    })
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let logical_delete_committed = data
            .as_ref()
            .map(|data| {
                entry_ids
                    .iter()
                    .all(|entry_id| !data.nodes.iter().any(|node| node.id == *entry_id))
            })
            .unwrap_or(false);
        if !logical_delete_committed {
            continue;
        }
        let mut pending = Vec::new();
        let mut errors = Vec::new();
        for entry_id in &entry_ids {
            let path = core.repo.layout.entry_img_dir(entry_id);
            let result = if path.is_dir() {
                std::fs::remove_dir_all(&path)
            } else if path.exists() {
                std::fs::remove_file(&path)
            } else {
                Ok(())
            };
            if let Err(error) = result {
                pending.push(entry_id.clone());
                errors.push(format!("{}：{error}", path.display()));
            }
        }
        let error = if errors.is_empty() {
            None
        } else {
            Some(errors.join("；"))
        };
        let _ = core.repo.finish_recovery(id, error.as_deref(), &pending);
    }
}

pub fn stale_descriptor_cleanup(data_dir: &Path) {
    let path = data_dir
        .join(crate::domain::repo::RUNTIME_DIR)
        .join(crate::domain::repo::HOST_DESCRIPTOR);
    if let Some(descriptor) = read_host_descriptor(&path) {
        if !crate::domain::ipc::host_process_alive(descriptor.pid) {
            remove_host_descriptor(&path);
        }
    }
}
