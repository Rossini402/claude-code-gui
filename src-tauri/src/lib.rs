mod cli;
mod commands;
mod scanner;
mod search;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 无头 CLI 模式：直接跑数据层输出 JSON（调试 / 对照验证用），不启动 GUI
    if cli::maybe_handle_cli() {
        return
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .invoke_handler(tauri::generate_handler![
            commands::list_projects,
            commands::get_sessions,
            commands::get_recent_sessions,
            commands::search_sessions,
            commands::get_messages,
            commands::get_archived_sessions,
            commands::archive_session,
            commands::restore_session,
            commands::open_terminal,
        ])
        .run(tauri::generate_context!())
        .expect("error while building tauri application");
}
