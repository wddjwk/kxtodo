//! kxtodo-server：KXToDo 数据同步服务端（单二进制，SQLite 存储）。
//!
//! 部署：`kxtodo-server --listen 0.0.0.0:8765`（数据默认落在
//! `~/.local/share/kxtodo/server/`，用 --db 覆盖）。
//! 升级：`kxtodo-server --update`（从 GitHub latest release 下载固定名
//! kxtodo-server 替换自身并自动重启）。

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use clap::Parser;

mod api;
mod db;
mod error;
mod update;
mod util;

use api::AppState;

#[derive(Debug, Parser)]
#[command(
    name = "kxtodo-server",
    version = env!("KXTODO_VERSION"),
    about = "KXToDo 数据同步服务端：账户 + 端到端加密实体存储（SQLite）",
    long_about = "KXToDo 数据同步服务端。\n\n服务器只保管密文与版本号，不理解也无法解密任何业务数据；\n客户端（kxtodo-cli / GUI）通过 sync register / login 配对。\n\n数据默认存放在 ~/.local/share/kxtodo/server/（XDG_DATA_HOME 优先）。\n\n示例：\n  kxtodo-server --listen 0.0.0.0:8765\n  kxtodo-server --db /var/lib/kxtodo/server.db --token-ttl-days 60\n  kxtodo-server --update"
)]
struct Args {
    /// 监听地址
    #[arg(long, value_name = "ip:port", default_value = "127.0.0.1:8765")]
    listen: String,

    /// SQLite 数据库路径（默认 ~/.local/share/kxtodo/server/data.db）
    #[arg(long, value_name = "path")]
    db: Option<std::path::PathBuf>,

    /// 登录 token 有效期（天）
    #[arg(long, value_name = "days", default_value_t = 30)]
    token_ttl_days: i64,

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

fn resolve_db_path(args: &Args) -> std::path::PathBuf {
    if let Some(path) = &args.db {
        return path.clone();
    }
    if let Ok(from_env) = std::env::var("KXTODO_SERVER_DB") {
        if !from_env.trim().is_empty() {
            return std::path::PathBuf::from(from_env);
        }
    }
    kxtodo_core::repo::default_server_dir().join("data.db")
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

    let listen = match parse_listen(&args.listen) {
        Ok(listen) => listen,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };
    let db_path = resolve_db_path(&args);
    let database = match db::Db::open(&db_path) {
        Ok(database) => database,
        Err(error) => {
            eprintln!("数据库打开失败（{}）：{}", db_path.display(), error);
            std::process::exit(3);
        }
    };

    let state = Arc::new(AppState {
        db: database,
        token_ttl_days: args.token_ttl_days,
        challenges: Mutex::new(std::collections::HashMap::new()),
    });
    let app = api::router(state).layer(axum::extract::DefaultBodyLimit::max(64 * 1024 * 1024));

    let bind = bind_with_retry(listen, args.update_restarted).await;
    let listener = match bind {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("监听 {listen} 失败：{error}");
            std::process::exit(4);
        }
    };

    println!(
        "kxtodo-server v{} 已启动：http://{listen}（数据库：{}）",
        update::APP_VERSION,
        db_path.display()
    );
    println!("提示：HTTP 明文下抓包只能看到加密实体；数据密钥永不出客户端。");

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
