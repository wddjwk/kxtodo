//! kxtodo-cli：标准控制台程序，操作 KXToDo 用户数据。
//! 数据目录解析：--data-dir > KXTODO_HOME > 当前目录；找不到数据直接报错。

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(kxtodo_core::cli::main_entry(&args));
}
