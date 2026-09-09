pub mod asr;
pub mod benchmark;
pub mod cache;
mod commands;
pub mod knowledge;
pub mod media;
pub mod process;
pub mod profiler;
pub mod service;
pub mod settings;
pub mod source;
pub mod web_source;

use service::Runtime;
use std::sync::Arc;
use tauri::{Emitter, Manager};

pub fn worker_path() -> std::path::PathBuf {
    if cfg!(debug_assertions) {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../workers/asr/worker.py")
    } else {
        std::env::current_exe()
            .unwrap_or_default()
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("workers/asr/worker.py")
    }
}

pub fn run() {
    let application = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let worker = if cfg!(debug_assertions) {
                worker_path()
            } else {
                app.path().resource_dir()?.join("workers/asr/worker.py")
            };
            let runtime = Runtime::new(settings::default_config_path(), worker)?;
            let handle = app.handle().clone();
            runtime.set_event_sink(Arc::new(move |job| {
                let _ = handle.emit("job-updated", job);
            }));
            app.manage(runtime);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::bootstrap,
            commands::probe_source,
            commands::probe_part,
            commands::create_jobs,
            commands::cancel_job,
            commands::retry_job,
            commands::get_asset_detail,
            commands::save_transcript_edit,
            commands::activate_version,
            commands::ensure_audio,
            commands::export_asset,
            commands::search_library,
            commands::generate_knowledge,
            commands::save_manual_note,
            commands::save_settings,
            commands::set_api_key,
            commands::probe_resources,
            commands::benchmark_profile,
            commands::list_benchmarks,
            commands::download_model,
            commands::cache_inventory,
            commands::clear_cache_category,
            commands::open_external,
            commands::open_data_folder
        ])
        .build(tauri::generate_context!())
        .expect("无法启动课程工作台，请检查设置文件与数据目录权限");
    application.run(|app, event| {
        if matches!(event, tauri::RunEvent::ExitRequested { .. }) {
            if let Some(state) = app.try_state::<Arc<Runtime>>() {
                state.shutdown();
            }
        }
    });
}
