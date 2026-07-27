// Console subsystem for all builds: CLI output works natively in any terminal
// (pwsh / cmd / Git Bash / WSL interop). GUI mode calls FreeConsole() at startup
// to hide the console window (standard dual-mode approach, same as Deno/Bun).

fn main() {
    #[cfg(target_os = "windows")]
    {
        // GUI mode (no CLI args): detach console immediately so double-click
        // doesn't show a console window. CLI mode keeps the console for output.
        let has_cli_args = std::env::args().nth(1).is_some();
        if !has_cli_args {
            unsafe {
                windows_sys::Win32::System::Console::FreeConsole();
            }
        }
    }
    todo_note_lib::run();
}
