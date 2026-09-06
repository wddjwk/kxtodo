//! kxtodo-server：KXToDo 数据同步服务端（单二进制，SQLite 存储）。
//!
//! 部署：`kxtodo-server --name 家里的服务器 --admin-user admin --admin-password xxx`
//! （默认监听 `0.0.0.0:52177`；数据默认 `~/.local/share/kxtodo/server/`；
//! 给过的参数持久化到 settings.json，下次启动未指定的项自动沿用，显式指定则覆盖）。
//! 后台静默运行：`--daemon`（分离/无窗口）+ `--stop`（结束）。
//! 管理界面：http://<host>:<port>/admin（账密登录，查看/管理 SQLite 数据）。
//! 升级：`kxtodo-server --update`。
//!
//! 这里只是**薄壳**：CLI 参数、pidfile、`--daemon`/`--stop`、信号处理、退出码、自升级。
//! 真正的服务器内核在 `kxtodo_server::host::start`——GUI/APK 的「本机作为服务器」
//! （v0.6.0 内置主机）用的是同一个函数，因此行为与这个二进制完全一致。

use clap::Parser;

use kxtodo_server::host::{self, AdminProvision, ServerConfig};
use kxtodo_server::{daemon, update};

#[derive(Debug, Parser)]
#[command(
    name = "kxtodo-server",
    version = env!("KXTODO_VERSION"),
    about = "KXToDo 数据同步服务端：账户 + 端到端加密实体/图片存储（SQLite）+ Web 管理界面",
    long_about = "KXToDo 数据同步服务端。\n\n服务器只保管密文与版本号，不理解业务数据；\n客户端（kxtodo-cli / GUI）通过 sync pair 配对。\n数据与图片（markdown 插图/列表背景/头像）都以密文存进 SQLite。\n管理界面在 /admin（管理员账密登录）。\n\n【局域网自动发现】客户端「发现」按钮在固定 UDP 端口 52177 上广播/组播查询，\n本服务器单播应答 --name 与真实 TCP 端口。发现端口与 TCP 端口无关：\n改了 --listen 的端口照样能被发现（回包里带的是真实端口）。\n前提有两个：监听在非回环地址上（默认 0.0.0.0；127.0.0.1 不应答局域网查询），\n且 UDP 52177 没被同机其它进程占用（占用时启动日志会提示发现不可用，\n此时客户端只能手动填 ip:port）。\n\n【后台静默运行】--daemon 以分离/无窗口方式重新拉起自己（Linux 新进程组、\nWindows CREATE_NO_WINDOW），关掉终端也继续跑；--stop 结束它。\n日志始终双写 stdout 与 server/log/，后台运行时看日志文件。\n\n启动参数会持久化到 ~/.local/share/kxtodo/server/settings.json：\n下次启动未指定的项自动沿用，显式指定则覆盖。\n首次启动必须提供 --admin-user 与 --admin-password（之后可省略，从配置读取）。\n\n数据默认存放在 ~/.local/share/kxtodo/server/（XDG_DATA_HOME 优先，\nWindows 为 %LOCALAPPDATA%\\kxtodo\\server），可用 --data-dir 指定；\n日志在 server/log/ 下按日轮转。\n\n示例：\n  kxtodo-server --name 家里的服务器 --admin-user admin --admin-password Secret\n  kxtodo-server --listen 0.0.0.0:52177 --daemon\n  kxtodo-server --stop\n  kxtodo-server --update"
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

    let config = ServerConfig {
        listen: args.listen.clone().unwrap_or_default(),
        data_dir: data_dir.clone(),
        db: args.db.clone(),
        name: args.name.clone(),
        admin: match (args.admin_user.clone(), args.admin_password.clone()) {
            (Some(user), Some(password)) => Some((user, password)),
            _ => None,
        },
        // 独立二进制的管理台必须显式给凭据（沿用 settings.json 里的既有值也行）；
        // 自动生成只给 GUI/APK 的内置主机用。
        admin_provision: AdminProvision::RequireExplicit,
        discovery: true,
        // 端口被占用就往后监听（52177 → 52178 …）：发现应答与客户端探测都带**真实**端口，
        // 所以换端口不影响被连上；比直接拒绝启动有用得多。
        port_fallback: host::DEFAULT_PORT_FALLBACK,
        retry_bind: args.update_restarted,
    };

    let handle = match host::start(config).await {
        Ok(handle) => handle,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(error.exit_code());
        }
    };
    let wanted_port = host::parse_listen(&handle.settings.listen)
        .map(|addr| addr.port())
        .unwrap_or_else(|_| handle.port());
    if handle.port() != wanted_port {
        println!(
            "注意：端口 {wanted_port} 已被占用，实际监听在 {}（发现应答与客户端都用真实端口）",
            handle.port()
        );
    }

    // pidfile 只属于独立进程：--daemon 的父进程靠它确认子进程起来了，--stop 靠它找目标。
    // 内置主机绝不能写（`--stop` 按 pid 动手会误杀宿主 GUI）。
    if let Err(message) = daemon::write_pid(&data_dir) {
        handle.state().log("info", &format!("pidfile 写入失败：{message}"));
    }

    shutdown_signal().await;
    handle.shutdown().await;
    daemon::remove_pid(&data_dir);
    println!("kxtodo-server 已停止");
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
