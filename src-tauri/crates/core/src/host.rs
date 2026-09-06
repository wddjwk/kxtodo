//! Background Host: single-instance owner of IPC, notifications and the
//! scheduler (§4.4), plus CLI routing (host IPC vs standalone execution).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};

use crate::cli::Routing;
use crate::core::{execute, ExecContext, ExecOutcome, HostServices, Invocation};
use crate::error::{CoreError, CoreResult, ErrorKind};
use crate::ids::request_id;
use crate::ipc::{
    discover_host, endpoint_for, outcome_envelope, read_host_descriptor, remove_host_descriptor,
    validate_request, write_host_descriptor, HostDescriptor, IpcClient, IpcRequest, IpcServer,
    PROTOCOL_VERSION,
};
use crate::model::{Notification, SettingsFile};
use crate::repo::{Domain, Repository};
use crate::scheduler::{render_notification, SchedulerHandle};
use crate::time::now_iso;

// ---------------------------------------------------------------------------
// Host backend (window/system capabilities; headless in tests)
// ---------------------------------------------------------------------------

/// 启动内置同步服务器所需的参数（core 从设置算好，宿主只管起）。
#[derive(Debug, Clone)]
pub struct SyncHostRequest {
    /// todo 数据目录：宿主据此推导 server 数据目录与 `runtime/sync-host.json` 的位置
    pub data_dir: PathBuf,
    /// 展示名 = 这台主机在局域网里的身份（客户端按名字选定主机）
    pub name: String,
    /// 期望监听端口；被占用时宿主自动向上找，**实际端口写进 `runtime/sync-host.json`**
    pub port: u16,
    /// 只绑回环（P2P 模式：内置库只服务本机隧道拨入，不该暴露到网卡上）；
    /// 局域网模式为 false（0.0.0.0 + UDP 发现应答）
    pub loopback_only: bool,
}

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

    /// 启动内置同步服务器（设置里的「本机作为服务器」）。
    ///
    /// core 不能依赖 kxtodo-server（依赖方向是 server → core），所以机制交给宿主：
    /// 在自己进程内跑 lib 化的 kxtodo-server，并把实际端口与库身份写进
    /// `runtime/sync-host.json`——core 只读那个文件来解析「本机就是主机时连哪儿」，
    /// 于是另开的 CLI 进程也能连上同一个内置主机（与 `runtime/host.json` 同套路）。
    ///
    /// 实现必须**非阻塞**（把绑定与 serve 丢进宿主的异步运行时后立即返回）：
    /// 它可能在一次命令执行途中被调用，同步等待会卡住 IPC 与 UI。
    /// 绑定失败之类只能事后知道的错误，写进 `runtime/sync-host.json` 的 `lastError`。
    fn sync_host_start(&self, request: &SyncHostRequest) -> CoreResult<()> {
        let _ = request;
        Err(CoreError::internal("当前 Host backend 不支持内置同步服务器"))
    }

    /// 停掉内置同步服务器并清掉 `runtime/sync-host.json`（幂等）。
    fn sync_host_stop(&self, data_dir: &Path) {
        let _ = data_dir;
    }

    /// 启动 P2P 运行时（iroh 端点 + 目录发布 + 接受拨入）。
    ///
    /// 与 `sync_host_start` 同一条规矩：**必须非阻塞**（可能在一次命令执行途中被调用）。
    /// P2P 的内置服务器必须先于它起好（被叫方要把隧道接进去）。
    fn p2p_start(&self, request: &P2pRequest) -> CoreResult<()> {
        let _ = request;
        Err(CoreError::internal("当前 Host backend 不支持 P2P"))
    }

    /// 停掉 P2P 运行时（尽力从目录撤掉自己；幂等）。
    fn p2p_stop(&self, data_dir: &Path) {
        let _ = data_dir;
    }

    /// 按「将要生效」的设置把 P2P 需要的两样宿主能力起好：只绑回环的内置库 + iroh 运行时。
    /// 配对早于设置落盘，等 Settings 域事件再启就鸡生蛋了（见 `HostServices::ensure_p2p_services`）。
    fn ensure_p2p_services(&self, data_dir: &Path, sync: &crate::model::SyncSettings) -> CoreResult<()> {
        let _ = (data_dir, sync);
        Ok(())
    }
}

/// 启动 P2P 运行时需要的东西：账户密钥派生目录签名密钥，其余是可选的自部署覆盖。
#[derive(Clone)]
pub struct P2pRequest {
    pub data_dir: PathBuf,
    pub keys: crate::sync::crypto::SyncKeys,
    /// None/空 = n0 免费公共 relay；`disabled` = 不用 relay；其它 = 自部署地址
    pub relay: Option<String>,
    /// 空 = n0 免费公共目录（dns.iroh.link/pkarr）
    pub directory_url: String,
    /// 本机设备名（与局域网主机名共用 `sync.lanName`）：发布进目录，
    /// 其它设备的列表里显示的就是它，而不是一串 EndpointId
    pub name: String,
}

// ---------------------------------------------------------------------------
// HostCore
// ---------------------------------------------------------------------------

pub struct HostCore {
    pub repo: Repository,
    pub data_dir: PathBuf,
    /// 该 Host 服务的数据目录是否为自定义目录（影响开机自启注册与 meta 提示）。
    pub custom_data_dir: bool,
    pub processes: crate::exec::ProcessRegistry,
    pub backend: RwLock<Option<Box<dyn HostBackend>>>,
    pub mode: String,
    pub scheduler: RwLock<Option<SchedulerHandle>>,
    pub notifications: NotificationTracker,
    pub started_at: String,
    pub ipc_endpoint: RwLock<String>,
    pub ipc_token: String,
    pub owner_lock: Mutex<Option<crate::repo::HostOwnerLock>>,
}

impl HostCore {
    pub fn new(repo: Repository, data_dir: PathBuf, mode: &str, custom_data_dir: bool) -> Arc<Self> {
        Arc::new(Self {
            repo,
            data_dir: crate::ipc::normalize_data_dir(&data_dir),
            custom_data_dir,
            processes: Default::default(),
            backend: RwLock::new(None),
            mode: mode.to_string(),
            scheduler: RwLock::new(None),
            notifications: NotificationTracker::default(),
            started_at: now_iso(),
            ipc_endpoint: RwLock::new(String::new()),
            ipc_token: crate::ipc::generate_token(),
            owner_lock: Mutex::new(None),
        })
    }

    pub fn set_backend(&self, backend: Box<dyn HostBackend>) {
        if let Ok(mut slot) = self.backend.write() {
            *slot = Some(backend);
        }
    }

    pub fn start_scheduler(self: &Arc<Self>) {
        let handle = crate::scheduler::start(self.clone());
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

    /// 按当前设置启停内置同步服务器与 P2P 运行时。
    ///
    /// 挂在 [`HostCore::emit_domain_event`] 的 Settings 分支上，所以设置不管是 GUI 面板改的、
    /// CLI `sync configure` 改的还是 `config set` 改的，常驻进程都会跟着调整——
    /// 与调度引擎的 `notify_scheduler_reload` 同一套路，不引入新机制。
    ///
    /// 两种「本机当服务器」的角色：
    /// - 局域网主机：内置服务器监听 0.0.0.0 并应答 UDP 发现
    /// - P2P：内置服务器**只绑回环**（只服务 iroh 隧道拨入）+ iroh 端点常驻发布目录
    pub fn reconcile_sync_host(&self) {
        let Ok(settings) = self.repo.load_settings() else {
            return;
        };
        let sync = &settings.sync;
        let mode = sync.effective_mode();
        let p2p_wanted = mode == crate::model::SyncMode::P2p && sync.is_paired();
        // 只有局域网方式下「本机作为服务器」才有意义
        let lan_wanted = sync.lan_host && mode == crate::model::SyncMode::Lan;
        let wanted = lan_wanted || p2p_wanted;
        // P2P 的内置库只给本机隧道用：绑回环、不应答发现（暴露到网卡上纯属风险）
        let loopback_only = p2p_wanted;
        let host_name = crate::sync::endpoint::desired_host_name(&sync.lan_name);
        let mut host = crate::sync::state::load_host_state(&self.repo.layout);
        // 残留自愈：描述符说在跑，但写它的那个进程已经没了（宿主崩溃/被杀）。
        // 端口随进程一起消失了，可残留的描述符会让客户端一直去连一个不存在的地址，
        // 设置页也永远显示「已在运行」。
        if host.running && host.pid != 0 && !crate::ipc::host_process_alive(host.pid) {
            let _ = crate::sync::state::clear_host_state(&self.repo.layout);
            host = crate::sync::state::EmbeddedHostState::default();
        }
        // 名字或端口变了要重启（名字是局域网身份，端口是客户端地址缓存的一部分）；
        // 上次启动失败也要重试，否则用户改完设置还是起不来。
        // 比的是 configured_port 而不是实际端口：实际端口可能因占用自动上移过，
        // 拿它跟配置值比会每轮都判定为「需要重启」。loopback 翻转（lan ↔ p2p）同理要重启。
        let up_to_date = host.running
            && host.last_error.is_none()
            && host.name == host_name
            && host.configured_port == sync.lan_port
            && host.loopback == loopback_only;
        let Ok(slot) = self.backend.read() else {
            return;
        };
        let Some(backend) = slot.as_ref() else {
            return;
        };
        if wanted && !up_to_date {
            let request = SyncHostRequest {
                data_dir: self.data_dir.clone(),
                name: host_name.clone(),
                port: sync.lan_port,
                loopback_only,
            };
            if let Err(error) = backend.sync_host_start(&request) {
                // 起不来的原因必须落进描述符：设置页得说得出「为什么没起来」，
                // 否则用户只看到一个没反应的勾选框。
                let mut state = crate::sync::state::EmbeddedHostState::default();
                state.name = request.name.clone();
                state.configured_port = request.port;
                state.loopback = loopback_only;
                state.last_error = Some(error.message.clone());
                let _ = crate::sync::state::save_host_state(&self.repo.layout, &state);
            }
        } else if !wanted && (host.running || host.last_error.is_some()) {
            backend.sync_host_stop(&self.data_dir);
        }

        // P2P 运行时在内置服务器之后起：被叫方要把隧道接进内置服务器
        if p2p_wanted {
            let Ok(keys) = crate::sync::crypto::derive_keys(&sync.username, &sync.secret) else {
                return;
            };
            let request = P2pRequest {
                data_dir: self.data_dir.clone(),
                keys,
                relay: if sync.p2p_relay.trim().is_empty() {
                    None
                } else {
                    Some(sync.p2p_relay.trim().to_string())
                },
                directory_url: sync.p2p_directory.trim().to_string(),
                name: host_name,
            };
            if let Err(error) = backend.p2p_start(&request) {
                crate::sync::engine::debug_log(format!("p2p runtime start failed: {}", error.message));
            }
        } else {
            backend.p2p_stop(&self.data_dir);
        }
    }

    pub fn emit_domain_event(&self, domain: Domain, revision: u64, ids: Vec<String>) {
        match domain {
            Domain::Schedule => self.notify_scheduler_reload(),
            // 设置里带着「本机作为服务器」的开关与主机名/端口：变了就要启停内置服务器
            Domain::Settings => self.reconcile_sync_host(),
            Domain::Data => {}
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
            Some(raw) => crate::time::parse_duration_ms(raw)?,
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
    fn ensure_p2p_services(&self, data_dir: &Path, sync: &crate::model::SyncSettings) -> CoreResult<()> {
        let backend = self
            .backend
            .read()
            .map_err(|error| CoreError::internal(format!("backend lock：{error}")))?;
        let Some(backend) = backend.as_ref() else {
            return Ok(());
        };
        backend.ensure_p2p_services(data_dir, sync)
    }

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

pub fn route(
    inv: &Invocation,
    data_dir: &Path,
    cwd: &Path,
    routing: Routing,
    custom_data_dir: bool,
) -> ExecOutcome {
    let normalized_data_dir = crate::ipc::normalize_data_dir(data_dir);
    let data_dir = normalized_data_dir.as_path();
    // IPC-host special commands never leave the host process.
    if routing == Routing::Auto {
        if let Some((descriptor, _)) = discover_host(data_dir) {
            return invoke_via_ipc(&descriptor, inv, data_dir, cwd).unwrap_or_else(|error| {
                ExecOutcome {
                    code: error.exit_code(),
                    envelope: crate::envelope::failure(
                        &inv.command,
                        &error,
                        crate::envelope::Meta::default(),
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
                            envelope: crate::envelope::failure(
                                &inv.command,
                                &error,
                                crate::envelope::Meta::default(),
                            ),
                        }
                    })
                }
                Err(error) => ExecOutcome {
                    code: error.exit_code(),
                    envelope: crate::envelope::failure(
                        &inv.command,
                        &error,
                        crate::envelope::Meta::default(),
                    ),
                },
            };
        }
        if inv.command == "doctor" {
            return execute_standalone(inv, data_dir, cwd, custom_data_dir);
        }
        // Standalone data CRUD under the launch mutex with a host re-check (§4.4).
        return with_launch_mutex(data_dir, || {
            if let Some((descriptor, _)) = discover_host(data_dir) {
                return invoke_via_ipc(&descriptor, inv, data_dir, cwd).unwrap_or_else(|error| {
                    ExecOutcome {
                        code: error.exit_code(),
                        envelope: crate::envelope::failure(
                            &inv.command,
                            &error,
                            crate::envelope::Meta::default(),
                        ),
                    }
                });
            }
            execute_standalone(inv, data_dir, cwd, custom_data_dir)
        });
    }
    execute_standalone(inv, data_dir, cwd, custom_data_dir)
}

fn execute_standalone(
    inv: &Invocation,
    data_dir: &Path,
    cwd: &Path,
    custom_data_dir: bool,
) -> ExecOutcome {
    let repo = if inv.command == "doctor" {
        Repository::open_readonly(data_dir.to_path_buf())
    } else {
        match Repository::open(data_dir.to_path_buf()) {
            Ok(repo) => repo,
            Err(error) => {
                return ExecOutcome {
                    code: error.exit_code(),
                    envelope: crate::envelope::failure(
                        &inv.command,
                        &error,
                        crate::envelope::Meta::default(),
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
                envelope: crate::envelope::failure(
                    &inv.command,
                    &error,
                    crate::envelope::Meta::default(),
                ),
            };
        }
    }
    let ctx = ExecContext {
        repo: &repo,
        cwd: crate::ipc::normalize_absolute_path(
            cwd,
            &std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        ),
        host: None,
        custom_data_dir,
    };
    execute(inv, &ctx)
}

/// Serialize host launches and standalone fallbacks (§4.4 启动互斥).
fn with_launch_mutex<F>(data_dir: &Path, f: F) -> ExecOutcome
where
    F: FnOnce() -> ExecOutcome,
{
    let runtime_dir = data_dir.join(crate::repo::RUNTIME_DIR);
    let _ = std::fs::create_dir_all(&runtime_dir);
    let lock_path = runtime_dir.join(crate::repo::HOST_LAUNCH_LOCK);
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
            envelope: crate::envelope::failure(
                "host",
                &CoreError::io(format!("无法取得 Host 启动互斥：{error}")),
                crate::envelope::Meta::default(),
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
    let normalized_data_dir = crate::ipc::normalize_data_dir(data_dir);
    let normalized_cwd = crate::ipc::normalize_absolute_path(
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
        controls: crate::ipc::IpcControls::from(&inv.controls),
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

/// GUI binary 名（与 kxtodo-cli 同目录）；隐藏 Host 由它承担（通知窗口、调度器）。
pub fn gui_exe_name() -> &'static str {
    if cfg!(windows) {
        "kxtodo.exe"
    } else {
        "kxtodo"
    }
}

/// 在 CLI 同目录里找 GUI 产物（均为固定名，不带版本号）：
/// - Windows：kxtodo.exe（即发布产物 KXToDo.exe，大小写不敏感命中）。
/// - Linux：先认稳定名 kxtodo（用户可把 AppImage 软链为它），再认发布产物 KXToDo.AppImage 本身
///   （应用内更新会把 KXToDo.AppImage 与 kxtodo-cli 一并下载到 ~/.local/share/kxtodo/bin）。
fn find_gui_exe(dir: &Path) -> Option<PathBuf> {
    let plain = dir.join(gui_exe_name());
    if crate::exec::is_executable_file(&plain) {
        return Some(plain);
    }
    #[cfg(target_os = "linux")]
    {
        let appimage = dir.join("KXToDo.AppImage");
        if crate::exec::is_executable_file(&appimage) {
            return Some(appimage);
        }
    }
    None
}

fn gui_exe_path() -> CoreResult<PathBuf> {
    let exe = std::env::current_exe()?;
    let Some(dir) = exe.parent() else {
        return Err(CoreError::new(
            ErrorKind::Execution,
            "HOST_LAUNCH_FAILED",
            "无法定位 kxtodo-cli 所在目录",
        ));
    };
    match find_gui_exe(dir) {
        Some(gui) => Ok(gui),
        None => Err(CoreError::new(
            ErrorKind::Execution,
            "GUI_NOT_FOUND",
            format!("未找到 GUI 程序（{}）", dir.join(gui_exe_name()).display()),
        )
        .with_hint(if cfg!(windows) {
            "notify / schedule run 需要 GUI 承担 Background Host：将 KXToDo.exe 与 kxtodo-cli.exe 放在同一目录"
        } else {
            "notify / schedule run 需要 GUI 承担 Background Host：将 KXToDo.AppImage 与 kxtodo-cli 放在同一目录（或把 AppImage 软链为 kxtodo）"
        })),
    }
}

/// Spawn the GUI executable as a hidden Host and wait for readiness.
fn launch_hidden_host(data_dir: &Path) -> CoreResult<HostDescriptor> {
    with_launch_mutex_plain(data_dir, || {
        // Re-check inside the mutex: another launcher may have won the race.
        if let Some((descriptor, _)) = discover_host(data_dir) {
            return Ok(descriptor);
        }
        let gui = gui_exe_path()?;
        let mut command = std::process::Command::new(gui);
        command.arg("--kxtodo-host").arg("--data-dir").arg(data_dir);
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // 脱离 CLI：不持有终端、不随 CLI 进程组收到 SIGHUP/SIGINT。
            command
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .process_group(0);
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
    let runtime_dir = data_dir.join(crate::repo::RUNTIME_DIR);
    std::fs::create_dir_all(&runtime_dir)?;
    let lock_path = runtime_dir.join(crate::repo::HOST_LAUNCH_LOCK);
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
    let data_dir = crate::ipc::normalize_data_dir(data_dir);
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
    let owner = crate::repo::HostOwnerLock::acquire(&core.repo.layout)?;
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
                            crate::ipc::serve_connection(&mut stream, &move |request| {
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
        return crate::envelope::failure(
            &request.command,
            &error,
            crate::envelope::Meta::default(),
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
                Some(Err(error)) => crate::envelope::failure(
                    "host.show",
                    &error,
                    crate::envelope::Meta::default(),
                ),
                None => crate::envelope::failure(
                    "host.show",
                    &CoreError::internal("Host backend 不可用"),
                    crate::envelope::Meta::default(),
                ),
            };
        }
        _ => {}
    }
    let invocation = Invocation {
        command: request.command.clone(),
        params: request.params.clone(),
        controls: crate::core::Controls {
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
        custom_data_dir: core.custom_data_dir,
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
    // 内置同步服务器随宿主一起收：托盘退出、关窗退出、看门狗自动退出三条路都汇聚到这里，
    // 否则会在进程里留下一个没人管的监听端口与一份说「还在跑」的描述符。
    if let Ok(slot) = core.backend.read() {
        if let Some(backend) = slot.as_ref() {
            backend.sync_host_stop(&core.data_dir);
            backend.p2p_stop(&core.data_dir);
        }
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
        .join(crate::repo::RUNTIME_DIR)
        .join(crate::repo::HOST_DESCRIPTOR);
    if let Some(descriptor) = read_host_descriptor(&path) {
        if !crate::ipc::host_process_alive(descriptor.pid) {
            remove_host_descriptor(&path);
        }
    }
}
