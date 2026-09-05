//! kxtodo-server：KXToDo 数据同步服务端（单二进制，SQLite 存储）。
//!
//! 部署：`kxtodo-server --name 家里的服务器 --admin-user admin --admin-password xxx`
//! （默认监听 `0.0.0.0:52177`；数据默认 `~/.local/share/kxtodo/server/`；
//! 给过的参数持久化到 settings.json，下次启动未指定的项自动沿用，显式指定则覆盖）。
//! 后台静默运行：`--daemon`（分离/无窗口）+ `--stop`（结束）。
//! 管理界面：http://<host>:<port>/admin（账密登录，查看/管理 SQLite 数据）。
//! 升级：`kxtodo-server --update`。

use std::net::SocketAddr;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use clap::Parser;

mod admin;
mod api;
mod daemon;
mod db;
mod discovery;
mod error;
mod logging;
mod settings;
mod update;
mod util;

use api::AppState;
use kxtodo_core::sync::discovery::{DEFAULT_SERVER_PORT, DISCOVERY_PORT};
use logging::Logger;
use settings::ServerSettings;

#[derive(Debug, Parser)]
#[command(
    name = "kxtodo-server",
    version = env!("KXTODO_VERSION"),
    about = "KXToDo 数据同步服务端：账户 + 端到端加密实体/图片存储（SQLite）+ Web 管理界面",
    long_about = "KXToDo 数据同步服务端。\n\n服务器只保管密文与版本号，不理解业务数据；\n客户端（kxtodo-cli / GUI）通过 sync register / login 配对。\n数据与图片（markdown 插图/列表背景/头像）都以密文存进 SQLite。\n管理界面在 /admin（管理员账密登录）。\n\n【局域网自动发现】客户端「发现」按钮在固定 UDP 端口 52177 上广播/组播查询，\n本服务器单播应答 --name 与真实 TCP 端口。发现端口与 TCP 端口无关：\n改了 --listen 的端口照样能被发现（回包里带的是真实端口）。\n前提有两个：监听在非回环地址上（默认 0.0.0.0；127.0.0.1 不应答局域网查询），\n且 UDP 52177 没被同机其它进程占用（占用时启动日志会提示发现不可用，\n此时客户端只能手动填 ip:port）。\n\n【后台静默运行】--daemon 以分离/无窗口方式重新拉起自己（Linux 新进程组、\nWindows CREATE_NO_WINDOW），关掉终端也继续跑；--stop 结束它。\n日志始终双写 stdout 与 server/log/，后台运行时看日志文件。\n\n启动参数会持久化到 ~/.local/share/kxtodo/server/settings.json：\n下次启动未指定的项自动沿用，显式指定则覆盖。\n首次启动必须提供 --admin-user 与 --admin-password（之后可省略，从配置读取）。\n\n数据默认存放在 ~/.local/share/kxtodo/server/（XDG_DATA_HOME 优先，\nWindows 为 %LOCALAPPDATA%\\kxtodo\\server），可用 --data-dir 指定；\n日志在 server/log/ 下按日轮转。\n\n示例：\n  kxtodo-server --name 家里的服务器 --admin-user admin --admin-password Secret\n  kxtodo-server --listen 0.0.0.0:52177 --daemon\n  kxtodo-server --stop\n  kxtodo-server --update"
)]
struct Args {
    /// 监听地址（持久化；只给 ip 或只给端口时用默认端口 52177）
    #[arg(long, value_name = "ip[:port]")]
    listen: Option<String>,

    /// 服务器展示名（持久化；客户端「发现」列表里显示，缺省用主机名）
    #[arg(long, value_name = "text")]
    name: Option<String>,

    /// 服务器数据目录（settings.json / log / server.pid 的根；默认平台数据目录下 kxtodo/server）
    #[arg(long, value_name = "path")]
    data_dir: Option<std::path::PathBuf>,

    /// SQLite 数据库路径（持久化；默认 <数据目录>/data.db）
    #[arg(long, value_name = "path")]
    db: Option<std::path::PathBuf>,

    /// 管理界面管理员用户名（首次启动必填；持久化哈希，之后可省略）
    #[arg(long, value_name = "name", requires = "admin_password")]
    admin_user: Option<String>,

    /// 管理界面管理员密码（首次启动必填；只存哈希）
    #[arg(long, value_name = "password")]
    admin_password: Option<String>,

    /// 后台静默运行：以分离/无窗口方式重新拉起自己，本进程立即退出
    #[arg(long)]
    daemon: bool,

    /// 结束 --daemon 拉起的后台进程（按 server.pid）
    #[arg(long, conflicts_with = "daemon")]
    stop: bool,

    /// 检查并升级到 GitHub latest release 的 kxtodo-server，然后自动重启
    #[arg(long)]
    update: bool,

    /// 强制升级（跳过版本比较）
    #[arg(long, requires = "update")]
    force: bool,

    /// 内部标志：--update 拉起的新进程，端口被占用时重试绑定
    #[arg(long, hide = true)]
    update_restarted: bool,
}

/// 合并规则：CLI 显式指定 > settings.json > 默认值；显式指定后写回 settings.json。
fn resolve_settings(
    args: &Args,
    data_dir: &std::path::Path,
    logger: &mut Logger,
) -> Result<ServerSettings, String> {
    let existing: ServerSettings = settings::load(data_dir)
        .map_err(|e| format!("读取配置失败：{e}"))?
        .unwrap_or_default();
    let had_settings = settings::load(data_dir)
        .map(|v| v.is_some())
        .unwrap_or(false);

    // 默认监听所有网卡的固定端口：局域网自动发现要求端口可预期
    let listen = args
        .listen
        .clone()
        .or_else(|| {
            if existing.listen.is_empty() {
                None
            } else {
                Some(existing.listen.clone())
            }
        })
        .unwrap_or_else(|| format!("0.0.0.0:{DEFAULT_SERVER_PORT}"));
    let name = args
        .name
        .clone()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .or_else(|| {
            if existing.name.is_empty() {
                None
            } else {
                Some(existing.name.clone())
            }
        })
        .unwrap_or_else(default_server_name);
    let db = args
        .db
        .clone()
        .map(|p| p.display().to_string())
        .filter(|p| !p.is_empty())
        .or_else(|| {
            if existing.db.is_empty() {
                None
            } else {
                Some(existing.db.clone())
            }
        })
        .unwrap_or_else(|| data_dir.join("data.db").display().to_string());

    // 管理员：显式给出（账密成对）→ 哈希后写入；未给出 → 沿用既有；
    // 既无显式也无既有 → 拒绝启动（管理界面必须有门禁）。
    let (admin_user, admin_password_hash, admin_password_salt) =
        match (&args.admin_user, &args.admin_password) {
            (Some(user), Some(password)) => {
                let user = user.trim().to_string();
                if user.is_empty() || password.is_empty() {
                    return Err("管理员用户名/密码不能为空".to_string());
                }
                let (hash, salt) = settings::hash_password(password);
                (user, hash, Some(salt))
            }
            _ => {
                if existing.admin_user.is_empty() {
                    return Err(
                        "首次启动必须提供 --admin-user 与 --admin-password（管理界面登录凭据）"
                            .to_string(),
                    );
                }
                (
                    existing.admin_user.clone(),
                    existing.admin_password_hash.clone(),
                    existing.admin_password_salt.clone(),
                )
            }
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
    if !had_settings || merged != existing {
        settings::save(data_dir, &merged)
            .map_err(|e| format!("写入配置失败：{e}"))?;
        logger.log(
            "info",
            &format!(
                "配置已保存：listen={} name={} db={} adminUser={}",
                merged.listen, merged.name, merged.db, merged.admin_user
            ),
        );
    }
    Ok(merged)
}

/// 缺省展示名：主机名（Windows 的 COMPUTERNAME / unix 的 HOSTNAME），拿不到就用二进制名。
fn default_server_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "kxtodo-server".to_string())
}

/// `--listen` 允许三种写法：`ip:port`、`:port`、`ip`（省略端口时用默认发现端口）；
/// 纯数字仍按端口理解（兼容旧写法）。
fn parse_listen(raw: &str) -> Result<SocketAddr, String> {
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
        .map_err(|error| format!("无效 --listen `{raw}`：{error}"))
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    if args.update {
        if let Err(message) = update::run_update(args.force) {
            eprintln!("升级失败：{message}");
            std::process::exit(1);
        }
        return;
    }

    let data_dir = args
        .data_dir
        .clone()
        .unwrap_or_else(kxtodo_core::repo::default_server_dir);

    if args.stop {
        match daemon::stop(&data_dir) {
            Ok(()) => println!("已停止后台 kxtodo-server"),
            Err(message) => {
                eprintln!("{message}");
                std::process::exit(1);
            }
        }
        return;
    }

    if args.daemon {
        match daemon::spawn_detached() {
            Ok(pid) => {
                println!("已在后台启动 kxtodo-server（pid {pid}）");
                println!("日志：{}", data_dir.join("log").display());
                println!("停止：kxtodo-server --stop");
                // 子进程绑定成功后才会写 pidfile；等一会儿就能知道它是否真的起来了
                let started = (0..50).any(|_| {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    std::fs::read_to_string(daemon::pid_file(&data_dir))
                        .map(|raw| raw.trim() == pid.to_string())
                        .unwrap_or(false)
                });
                if !started {
                    eprintln!(
                        "警告：5 秒内没等到后台进程就绪（端口被占用或配置有误？），\n\
                         请查看日志：{}",
                        data_dir.join("log").display()
                    );
                    std::process::exit(4);
                }
            }
            Err(message) => {
                eprintln!("{message}");
                std::process::exit(1);
            }
        }
        return;
    }

    let mut logger = Logger::new(data_dir.join("log"));

    let merged = match resolve_settings(&args, &data_dir, &mut logger) {
        Ok(merged) => merged,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };

    let listen = match parse_listen(&merged.listen) {
        Ok(listen) => listen,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };
    let db_path = std::path::PathBuf::from(&merged.db);
    let database = match db::Db::open(&db_path) {
        Ok(database) => database,
        Err(error) => {
            eprintln!("数据库打开失败（{}）：{}", db_path.display(), error);
            std::process::exit(3);
        }
    };

    let state = Arc::new(AppState {
        db: database,
        logger: Mutex::new(logger),
        settings: merged.clone(),
        challenges: Mutex::new(std::collections::HashMap::new()),
        admin_sessions: Mutex::new(std::collections::HashMap::new()),
    });
    let app = api::router(state.clone());

    let bind = bind_with_retry(listen, args.update_restarted).await;
    let listener = match bind {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("监听 {listen} 失败：{error}");
            std::process::exit(4);
        }
    };

    if let Err(message) = daemon::write_pid(&data_dir) {
        state.log("info", &format!("pidfile 写入失败：{message}"));
    }

    // 局域网发现应答：只在监听非回环地址时开启（回环服务器对局域网客户端没意义）
    let discovery_running = Arc::new(AtomicBool::new(true));
    if !listen.ip().is_loopback() {
        match discovery::spawn(state.clone(), listen.port(), discovery_running.clone()) {
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
    } else {
        state.log(
            "info",
            &format!("监听在回环地址（{listen}），不开启局域网发现"),
        );
    }

    state.log(
        "info",
        &format!(
            "kxtodo-server v{}「{}」已启动：http://{listen}（数据库：{}）",
            update::APP_VERSION,
            merged.name,
            db_path.display()
        ),
    );
    state.log(
        "info",
        &format!("管理界面：http://{listen}/admin（管理员：{}）", merged.admin_user),
    );
    if listen.port() != DEFAULT_SERVER_PORT {
        state.log(
            "info",
            &format!(
                "TCP 端口是 {port}（非默认 {DEFAULT_SERVER_PORT}）：\
                 自动发现走固定 UDP {DISCOVERY_PORT}，与 TCP 端口无关，\
                 回包里会带上真实端口 {port}，客户端可直接点选",
                port = listen.port()
            ),
        );
    }
    if listen.ip().is_unspecified() {
        state.log(
            "info",
            "监听 0.0.0.0（所有网卡）：局域网内可发现、可连接；只在本机用请 --listen 127.0.0.1",
        );
    }

    let shutdown_flag = discovery_running.clone();
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            shutdown_flag.store(false, std::sync::atomic::Ordering::SeqCst);
        })
        .await
        .expect("server loop");
    daemon::remove_pid(&data_dir);
    println!("kxtodo-server 已停止");
}

async fn bind_with_retry(
    addr: SocketAddr,
    retry: bool,
) -> std::io::Result<tokio::net::TcpListener> {
    if !retry {
        return tokio::net::TcpListener::bind(addr).await;
    }
    // --update 拉起的子进程：父进程退出释放端口需要一点时间
    for attempt in 0..30 {
        match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => return Ok(listener),
            Err(error) if attempt < 29 => {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                let _ = error;
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!()
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
