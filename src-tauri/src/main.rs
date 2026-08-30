// GUI 子系统：Windows 下不附加控制台，启动即干净的应用窗口（无控制台一闪）。
// CLI 是独立的 kxtodo-cli binary；本程序只负责 GUI / 隐藏 Host 两种模式。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    todo_note_lib::run();
}
