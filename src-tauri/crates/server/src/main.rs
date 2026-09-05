//! kxtodo-server：KXToDo 数据同步服务端（单二进制，SQLite 存储）。
//!
//! 部署：`kxtodo-server --listen 0.0.0.0:8765 --admin-user admin --admin-password xxx`
//! （数据默认 `~/.local/share/kxtodo/server/`；给过的参数持久化到 settings.json，
//! 下次启动未指定的项自动沿用，显式指定则覆盖）。
//! 管理界面：http://<host>:<port>/admin（账密登录，查看/管理 SQLite 数据）。
//! 升级：`kxtodo-server --update`。

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use clap::Parser;

mod admin;
mod api;
mod db;
mod error;
mod logging;
mod settings;
mod update;
mod util;

use api::AppState;
use logging::Logger;
use settings::ServerSettings;

#[derive(Debug, Parser)]
#[command(
    name = "kxtodo-server",
    version = env!("KXTODO_VERSION"),
    about = "KXToDo 数据同步服务端：账户 + 端到端加密实体存储（SQLite）+ Web 管理界面",
    long_about = "KXToDo 数据同步服务端。\n\n服务器只保管密文与版本号，不理解业务数据；\n客户端（kxtodo-cli / GUI）通过 sync register / login 配对。\n管理界面在 /admin（管理员账密登录）。\n\n启动参数会持久化到 ~/.local/share/kxtodo/server/settings.json：\n下次启动未指定的项自动沿用，显式指定则覆盖。\n首次启动必须提供 --admin-user 与 --admin-password（之后可省略，从配置读取）。\n\n数据默认存放在 ~/.local/share/kxtodo/server/（XDG_DATA_HOME 优先），\n日志在 server/log/ 下按日轮转。\n\n示例：\n  kxtodo-server --listen 0.0.0.0:8765 --admin-user admin --admin-password Secret\n  kxtodo-server --update"
)]
struct Args {
    /// 监听地址（持久化；未指定时从 settings.json 读取，缺省 127.0.0.1:8765）
    #[arg(long, value_name = "ip:port")]
    listen: Option<String>,

    /// SQLite 数据库路径（持久化；默认 ~/.local/share/kxtodo/server/data.db）
    #[arg(long, value_name = "path")]
    db: Option<std::path::PathBuf>,

    /// 管理界面管理员用户名（首次启动必填；持久化哈希，之后可省略）
    #[arg(long, value_name = "name", requires = "admin_password")]
    admin_user: Option<String>,

    /// 管理界面管理员密码（首次启动必填；只存哈希）
    #[arg(long, value_name = "password")]
    admin_password: Option<String>,

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
        .unwrap_or_else(|| "127.0.0.1:8765".to_string());
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
                "配置已保存：listen={} db={} adminUser={}",
                merged.listen, merged.db, merged.admin_user
            ),
        );
    }
    Ok(merged)
}

fn parse_listen(raw: &str) -> Result<SocketAddr, String> {
    if let Ok(addr) = raw.parse::<SocketAddr>() {
        return Ok(addr);
    }
    // 允许只写端口或省略地址
    let with_host = if raw.starts_with(':') {
        format!("0.0.0.0{raw}")
    } else if !raw.contains(':') {
        format!("0.0.0.0:{raw}")
    } else {
        raw.to_string()
    };
    with_host
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

    let data_dir = kxtodo_core::repo::default_server_dir();
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

    state.log(
        "info",
        &format!(
            "kxtodo-server v{} 已启动：http://{listen}（数据库：{}）",
            update::APP_VERSION,
            db_path.display()
        ),
    );
    state.log(
        "info",
        &format!("管理界面：http://{listen}/admin（管理员：{}）", merged.admin_user),
    );

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("server loop");
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
