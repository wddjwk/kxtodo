//! 可嵌入的服务器内核（v0.6.0）。
//!
//! 把「解析配置 → 打开库 → 建路由 → 绑定端口 → 发现应答 → 服务循环」收成一个
//! [`start`]：独立二进制（`main.rs`）与 GUI/APK 的**内置主机**走完全相同的代码路径，
//! 只有「谁调它、活多久、进程级动作谁做」不同。
//!
//! 刻意留在这里之外的东西（由 `main.rs` 负责）：pidfile、`--daemon` 重 spawn、
//! `--stop`、Ctrl+C/SIGTERM 信号处理器、退出码、`--update` 自升级。
//! 内置主机不拥有进程：写 pidfile 会让 `kxtodo-server --stop` 误杀宿主 GUI，
//! 装信号处理器会和 GUI 自己的退出流程打架。
//!
//! # 停机
//!
//! [`ServerHandle::request_shutdown`] 是同步的（任意线程可调，不等待退出），
//! [`ServerHandle::shutdown`] 会额外等服务循环结束（需要在 runtime 上 await）。
//! handle 被 drop 时自动请求停机，宿主忘了收也不会留下野服务。

use std::collections::HashMap;
use std::fmt;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use kxtodo_core::sync::discovery::{DEFAULT_SERVER_PORT, DISCOVERY_PORT};
use tokio::sync::oneshot;

use crate::api::{AppState, SharedState, APP_VERSION};
use crate::db::Db;
use crate::logging::Logger;
use crate::settings::{self, ServerSettings};
use crate::util;

/// 内置主机端口被占用时向上递增的最大尝试次数。
///
/// 同机既跑了独立 `kxtodo-server` 又勾了「本机作为服务器」时必然撞端口；
/// 自动发现回包带的是**真实**端口，所以上移不影响被发现。
pub const DEFAULT_PORT_FALLBACK: u16 = 20;

/// 管理台凭据的来源策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdminProvision {
    /// 独立二进制：首次启动必须显式给账密，否则拒绝启动（管理台必须有门禁）
    RequireExplicit,
    /// 内置主机：沿用 settings.json 里的既有凭据，没有就自动生成随机密码。
    /// 明文只在 [`ServerHandle::generated_admin_password`] 里返回一次（供 GUI 展示），
    /// 落盘的仍然只有哈希。
    GenerateIfMissing,
}

/// 启动配置。独立二进制从 CLI 参数拼，内置主机从同步设置拼。
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// 期望监听地址；写法同 CLI `--listen`（`ip:port` / `:port` / `ip` / 纯端口），空 = `0.0.0.0:52177`
    pub listen: String,
    /// settings.json / log / data.db 的根目录
    pub data_dir: PathBuf,
    /// SQLite 路径；None = `<data_dir>/data.db`
    pub db: Option<PathBuf>,
    /// 展示名（局域网发现列表里显示，也是主机在局域网内的身份）；None = 沿用 settings.json，再退主机名
    pub name: Option<String>,
    /// 显式指定的管理台账密（给了就覆盖并写回 settings.json）
    pub admin: Option<(String, String)>,
    /// 没有持久化凭据时的行为
    pub admin_provision: AdminProvision,
    /// 是否应答局域网 UDP 发现（监听回环地址时自动跳过）
    pub discovery: bool,
    /// 端口被占用时向上递增尝试的次数；0 = 只试期望端口
    pub port_fallback: u16,
    /// 绑定失败后原地重试（`--update` 拉起的子进程等父进程释放端口）
    pub retry_bind: bool,
}

/// 启动失败。分类是为了让二进制映射到与 v0.5.x 完全一致的退出码（2/3/4）。
#[derive(Debug)]
pub enum StartError {
    Config(String),
    Database(String),
    Bind(String),
}

impl StartError {
    /// 二进制入口的退出码：配置 2、数据库 3、绑定 4（与 v0.5.x 一致）。
    pub fn exit_code(&self) -> i32 {
        match self {
            StartError::Config(_) => 2,
            StartError::Database(_) => 3,
            StartError::Bind(_) => 4,
        }
    }
}

impl fmt::Display for StartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StartError::Config(message)
            | StartError::Database(message)
            | StartError::Bind(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for StartError {}

/// 运行中的服务器。宿主拿它读实际端口/身份，并负责停机。
pub struct ServerHandle {
    /// 实际绑定到的地址（端口可能因占用而向上移过）
    pub local_addr: SocketAddr,
    /// 本机数据库的身份（建库时生成、重启不变）。客户端据此判断
    /// 「我连的还是不是同一台主机 / 它的库是不是被重建过」，变了就要重置同步水位。
    pub instance_id: String,
    /// 合并并持久化后的配置（含实际 listen/db/name/adminUser）
    pub settings: ServerSettings,
    /// 本次自动生成的管理台密码明文；沿用既有凭据时为 None
    pub generated_admin_password: Option<String>,
    state: SharedState,
    discovery_running: Arc<AtomicBool>,
    shutdown: Mutex<Option<oneshot::Sender<()>>>,
    serve_task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl ServerHandle {
    pub fn port(&self) -> u16 {
        self.local_addr.port()
    }

    pub fn name(&self) -> &str {
        &self.settings.name
    }

    pub fn admin_user(&self) -> &str {
        &self.settings.admin_user
    }

    /// 本机回环 base url：内置主机自己也是它的一个客户端（每轮只连一个 endpoint，
    /// 自己是主机时就连自己的 localhost）。
    pub fn loopback_base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port())
    }

    /// 管理台地址（本机访问）
    pub fn admin_url(&self) -> String {
        format!("{}/admin", self.loopback_base_url())
    }

    pub fn state(&self) -> SharedState {
        self.state.clone()
    }

    /// 请求优雅停机：同步、任意线程可调、不等待退出。幂等。
    pub fn request_shutdown(&self) {
        self.discovery_running.store(false, Ordering::SeqCst);
        if let Some(sender) = self.shutdown.lock().unwrap().take() {
            let _ = sender.send(());
        }
    }

    /// 请求停机并等服务循环结束（需在 runtime 上 await）。
    pub async fn shutdown(self) {
        self.request_shutdown();
        if let Some(task) = self.serve_task.lock().unwrap().take() {
            let _ = task.await;
        }
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        self.request_shutdown();
    }
}

/// 启动服务器。**必须在 tokio runtime 上调用**（GUI 侧用 `tauri::async_runtime`）。
///
/// 返回时端口已绑定、发现应答已在跑、服务循环已 spawn；调用方持有 handle 期间服务持续，
/// drop 或 [`ServerHandle::request_shutdown`] 后优雅停机。
pub async fn start(config: ServerConfig) -> Result<ServerHandle, StartError> {
    let mut logger = Logger::new(config.data_dir.join("log"));
    let (merged, generated_password) = resolve(&config, &mut logger)?;
    let listen = parse_listen(&merged.listen)?;

    let db_path = PathBuf::from(&merged.db);
    let database = Db::open(&db_path).map_err(|error| {
        StartError::Database(format!(
            "数据库打开失败（{}）：{error}",
            db_path.display()
        ))
    })?;
    // v0.5.1 账户模型改为「用户名 + 密码」：旧库里的账户（用户名+邮箱）整体归档，
    // 否则同名注册会被 ACCOUNT_EXISTS 拒掉且永远登录不上。
    match database.migrate_legacy_accounts() {
        Ok(0) => {}
        Ok(count) => logger.log(
            "info",
            &format!(
                "检测到 v0.5.0 及以前的账户表：{count} 个旧账户已归档到 users_legacy。\
                 旧账户的密钥由「用户名+邮箱」派生，新客户端只填用户名+密码，无法再登录；\
                 其数据仍保留在库中，可在管理界面查看或删除。"
            ),
        ),
        Err(error) => return Err(StartError::Database(format!("旧账户归档失败：{error}"))),
    }
    let instance_id = database
        .instance_id()
        .map_err(|error| StartError::Database(format!("读取数据库身份失败：{error}")))?;

    let state = Arc::new(AppState {
        db: database,
        logger: Mutex::new(logger),
        settings: merged.clone(),
        instance_id: instance_id.clone(),
        metrics: crate::metrics::Metrics::new(util::now_iso()),
        challenges: Mutex::new(HashMap::new()),
        admin_sessions: Mutex::new(HashMap::new()),
    });
    let app = crate::api::router(state.clone());

    let (listener, local_addr) = bind(listen, config.port_fallback, config.retry_bind, &state).await?;

    // 局域网发现应答：只在监听非回环地址时开启（回环服务器对局域网客户端没意义）
    let discovery_running = Arc::new(AtomicBool::new(true));
    if config.discovery && !local_addr.ip().is_loopback() {
        match crate::discovery::spawn(state.clone(), local_addr.port(), discovery_running.clone()) {
            Ok(_) => state.log(
                "info",
                &format!(
                    "局域网发现已开启：UDP {DISCOVERY_PORT}（组播 239.255.77.52 + 广播），展示名「{}」",
                    merged.name
                ),
            ),
            Err(message) => state.log(
                "info",
                &format!("{message}；自动发现不可用，客户端需手动填 ip:port"),
            ),
        }
    } else if config.discovery {
        state.log(
            "info",
            &format!("监听在回环地址（{local_addr}），不开启局域网发现"),
        );
    }

    announce(&state, &merged, local_addr, &db_path, generated_password.is_some());

    let (sender, receiver) = oneshot::channel::<()>();
    let flag = discovery_running.clone();
    // with_connect_info：handler 用 ConnectInfo 取真实对端地址（管理台要展示来源 IP，
    // 局域网直连场景没有 X-Forwarded-For）
    let serve_task = tokio::spawn(async move {
        let result = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async move {
            let _ = receiver.await;
            flag.store(false, Ordering::SeqCst);
        })
        .await;
        if let Err(error) = result {
            eprintln!("kxtodo-server 服务循环异常退出：{error}");
        }
    });

    Ok(ServerHandle {
        local_addr,
        instance_id,
        settings: merged,
        generated_admin_password: generated_password,
        state,
        discovery_running,
        shutdown: Mutex::new(Some(sender)),
        serve_task: Mutex::new(Some(serve_task)),
    })
}

/// 合并规则：显式指定 > settings.json > 默认值；结果写回 settings.json。
///
/// 返回合并后的配置，以及本次**自动生成**的管理台密码明文（沿用既有凭据时为 None）。
fn resolve(
    config: &ServerConfig,
    logger: &mut Logger,
) -> Result<(ServerSettings, Option<String>), StartError> {
    let loaded = settings::load(&config.data_dir).map_err(|error| {
        StartError::Config(format!("读取配置失败：{error}"))
    })?;
    let existing = loaded.clone().unwrap_or_default();

    // 默认监听所有网卡的固定端口：局域网自动发现要求端口可预期
    let listen = non_empty(config.listen.clone())
        .or_else(|| non_empty(existing.listen.clone()))
        .unwrap_or_else(|| format!("0.0.0.0:{DEFAULT_SERVER_PORT}"));
    let name = config
        .name
        .as_ref()
        .map(|name| name.trim().to_string())
        .and_then(non_empty)
        .or_else(|| non_empty(existing.name.clone()))
        .unwrap_or_else(default_server_name);
    let db = config
        .db
        .as_ref()
        .map(|path| path.display().to_string())
        .and_then(non_empty)
        .or_else(|| non_empty(existing.db.clone()))
        .unwrap_or_else(|| config.data_dir.join("data.db").display().to_string());

    // 管理员：显式给出（账密成对）→ 哈希后写入；未给出 → 沿用既有；
    // 两者都没有 → 按 AdminProvision 决定是拒绝启动还是自动生成。
    let explicit = match &config.admin {
        Some((user, password)) => {
            let user = user.trim().to_string();
            if user.is_empty() || password.is_empty() {
                return Err(StartError::Config("管理员用户名/密码不能为空".to_string()));
            }
            Some((user, password.clone()))
        }
        None => None,
    };
    let (admin_user, admin_password_hash, admin_password_salt, generated) = match explicit {
        Some((user, password)) => {
            let (hash, salt) = settings::hash_password(&password);
            (user, hash, Some(salt), None)
        }
        None if !existing.admin_user.is_empty() => (
            existing.admin_user.clone(),
            existing.admin_password_hash.clone(),
            existing.admin_password_salt.clone(),
            None,
        ),
        None => match config.admin_provision {
            AdminProvision::RequireExplicit => {
                return Err(StartError::Config(
                    "首次启动必须提供 --admin-user 与 --admin-password（管理界面登录凭据）"
                        .to_string(),
                ));
            }
            AdminProvision::GenerateIfMissing => {
                let password = util::random_hex(12);
                let (hash, salt) = settings::hash_password(&password);
                ("admin".to_string(), hash, Some(salt), Some(password))
            }
        },
    };

    let merged = ServerSettings {
        listen,
        db,
        name,
        admin_user,
        admin_password_hash,
        admin_password_salt,
        version: 1,
    };
    if loaded.is_none() || merged != existing {
        settings::save(&config.data_dir, &merged)
            .map_err(|error| StartError::Config(format!("写入配置失败：{error}")))?;
        logger.log(
            "info",
            &format!(
                "配置已保存：listen={} name={} db={} adminUser={}",
                merged.listen, merged.name, merged.db, merged.admin_user
            ),
        );
    }
    Ok((merged, generated))
}

fn non_empty(raw: String) -> Option<String> {
    let trimmed = raw.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// 缺省展示名：主机名（Windows 的 COMPUTERNAME / unix 的 HOSTNAME），拿不到就用二进制名。
pub fn default_server_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "kxtodo-server".to_string())
}

/// `--listen` 允许三种写法：`ip:port`、`:port`、`ip`（省略端口时用默认发现端口）；
/// 纯数字仍按端口理解（兼容旧写法）。
pub fn parse_listen(raw: &str) -> Result<SocketAddr, StartError> {
    let trimmed = raw.trim();
    if let Ok(addr) = trimmed.parse::<SocketAddr>() {
        return Ok(addr);
    }
    let normalized = if trimmed.starts_with(':') {
        format!("0.0.0.0{trimmed}")
    } else if !trimmed.contains(':') {
        if trimmed.parse::<u16>().is_ok() {
            format!("0.0.0.0:{trimmed}")
        } else {
            format!("{trimmed}:{DEFAULT_SERVER_PORT}")
        }
    } else {
        trimmed.to_string()
    };
    normalized
        .parse::<SocketAddr>()
        .map_err(|error| StartError::Config(format!("无效监听地址 `{raw}`：{error}")))
}

/// 绑定端口。两种重试语义刻意分开：
/// - `retry_bind`：原地重试同一个端口（`--update` 的子进程等父进程释放端口）
/// - `port_fallback`：端口被占用就向上换一个（内置主机与同机独立 server 撞车）
///
/// 必须是 async：重试最长 30×500ms，同步 sleep 会堵住 runtime 工作线程
/// （内置主机跑在 GUI 的 `tauri::async_runtime` 上）。
async fn bind(
    listen: SocketAddr,
    port_fallback: u16,
    retry_bind: bool,
    state: &SharedState,
) -> Result<(tokio::net::TcpListener, SocketAddr), StartError> {
    let attempts: usize = if port_fallback > 0 {
        port_fallback as usize + 1
    } else {
        1
    };
    let mut last_error = String::new();
    for attempt in 0..attempts {
        let port = match listen.port().checked_add(attempt as u16) {
            Some(port) => port,
            None => break,
        };
        let addr = SocketAddr::new(listen.ip(), port);
        match bind_once(addr, retry_bind && attempt == 0).await {
            Ok(listener) => {
                let local_addr = listener
                    .local_addr()
                    .map_err(|error| StartError::Bind(format!("读取绑定地址失败：{error}")))?;
                if attempt > 0 {
                    state.log(
                        "info",
                        &format!(
                            "端口 {} 已被占用，改用 {port}（自动发现回包带真实端口，客户端不受影响）",
                            listen.port()
                        ),
                    );
                }
                return Ok((listener, local_addr));
            }
            Err(error) => last_error = error.to_string(),
        }
    }
    Err(StartError::Bind(format!("监听 {listen} 失败：{last_error}")))
}

async fn bind_once(addr: SocketAddr, retry: bool) -> std::io::Result<tokio::net::TcpListener> {
    let attempts = if retry { 30 } else { 1 };
    let mut last_error = None;
    for attempt in 0..attempts {
        match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => return Ok(listener),
            Err(error) => {
                last_error = Some(error);
                if attempt + 1 < attempts {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
            }
        }
    }
    Err(last_error.unwrap_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::Other, "绑定失败")
    }))
}

fn announce(
    state: &SharedState,
    settings: &ServerSettings,
    local_addr: SocketAddr,
    db_path: &Path,
    generated_admin: bool,
) {
    state.log(
        "info",
        &format!(
            "kxtodo-server v{APP_VERSION}「{}」已启动：http://{local_addr}（数据库：{}，身份 {}）",
            settings.name,
            db_path.display(),
            state.instance_id
        ),
    );
    if generated_admin {
        state.log(
            "info",
            &format!(
                "管理界面：http://{local_addr}/admin（管理员：{}，密码已自动生成，见应用设置页）",
                settings.admin_user
            ),
        );
    } else {
        state.log(
            "info",
            &format!(
                "管理界面：http://{local_addr}/admin（管理员：{}）",
                settings.admin_user
            ),
        );
    }
    if local_addr.port() != DEFAULT_SERVER_PORT {
        state.log(
            "info",
            &format!(
                "TCP 端口是 {port}（非默认 {DEFAULT_SERVER_PORT}）：\
                 自动发现走固定 UDP {DISCOVERY_PORT}，与 TCP 端口无关，\
                 回包里会带上真实端口 {port}，客户端可直接点选",
                port = local_addr.port()
            ),
        );
    }
    if local_addr.ip().is_unspecified() {
        state.log(
            "info",
            "监听 0.0.0.0（所有网卡）：局域网内可发现、可连接；只在本机用请监听 127.0.0.1",
        );
    }
}
