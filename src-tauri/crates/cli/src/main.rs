//! kxtodo-cli：标准控制台程序，操作 KXToDo 用户数据。
//! 数据目录解析：--data-dir > 系统默认数据目录；找不到数据直接报错。

fn main() {
    // Windows 下 env::args() 按系统 ANSI 代码页（如 936/GBK）解码命令行，中文参数会
    // 变乱码；args_os 保留原生 UTF-16，转 String 时对无法映射的字符有损降级，
    // 正常 UTF-8 内容不会受损。参数最终仍以 UTF-8 进入 clap。
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    std::process::exit(kxtodo_core::cli::main_entry(&args));
}
