mod document_processor;
use document_processor::selector::*;
use tauri_plugin_log::{Target, TargetKind};

#[tauri::command]
fn log_trace(message: String) -> String {
    log::trace!("{}", message);
    "Logged".to_string()
}

#[tauri::command]
fn log_info(message: String) -> String {
    log::info!("{}", message);
    "Logged".to_string()
}

#[tauri::command]
fn log_error(message: String) -> String {
    log::error!("{}", message);
    "Logged".to_string()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .targets([
                    Target::new(TargetKind::Stdout),
                    Target::new(TargetKind::LogDir { file_name: None }),
                    Target::new(TargetKind::Webview),
                ])
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            log_trace,
            log_info,
            log_error,
            greet,
            select_document,
            prepare_document
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
