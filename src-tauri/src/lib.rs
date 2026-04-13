mod audio_backend;
mod audio_capabilities;
mod audio_capture;
mod audio_combine;
mod audio_resampler;
mod commands;
mod error;
mod secrets;
mod session_manager;
mod settings;
mod soniox_runtime;
mod storage;
mod transcript_buffer;
mod types;
mod websocket_client;

use session_manager::SessionManager;
use settings::Settings;
use std::sync::Arc;
use tauri::Manager;
use tokio::sync::Mutex;
use tracing_subscriber::EnvFilter;

pub struct AppState {
    pub session_manager: Arc<Mutex<SessionManager>>,
    pub settings: Arc<Mutex<Settings>>,
    pub secret_store: Arc<Mutex<secrets::SecretStore>>,
}

fn init_logging() {
    if !cfg!(debug_assertions) {
        return;
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
}

pub fn run() {
    // Structured logging is enabled in debug/dev builds only.
    init_logging();

    let settings = Settings::load_or_default();
    let session_manager = SessionManager::new();

    let secret_store = secrets::SecretStore::new();

    let app_state = AppState {
        session_manager: Arc::new(Mutex::new(session_manager)),
        settings: Arc::new(Mutex::new(settings)),
        secret_store: Arc::new(Mutex::new(secret_store)),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(app_state)
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::CloseRequested { api, .. } => {
                // Prevent immediate OS termination
                api.prevent_close();
                let app_handle = window.app_handle().clone();

                tauri::async_runtime::spawn(async move {
                    tracing::info!("Received close request, executing safe teardown...");
                    let session_manager = {
                        let state: tauri::State<AppState> = app_handle.state();
                        state.session_manager.clone()
                    };
                    {
                        let mut sm = session_manager.lock().await;
                        if let Err(e) = sm.stop(&app_handle).await {
                            tracing::warn!("Session cleanup warning during shutdown: {e}");
                        }
                    }
                    tracing::info!("Safe teardown complete. Exiting process.");
                    app_handle.exit(0);
                });
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::save_settings,
            commands::save_settings_and_api_key,
            commands::start_session,
            commands::stop_session,
            commands::pause_session,
            commands::resume_session,
            commands::get_session_state,
            commands::get_audio_runtime_capabilities,
            commands::list_audio_devices,
            commands::set_api_key,
            commands::has_api_key,
            commands::list_sessions,
        ])
        .setup(|app| {
            let handle = app.handle().clone();

            // Validate basic I/O capabilities for true portable-mode execution
            let data_path = crate::settings::data_dir();
            if let Err(e) = std::fs::create_dir_all(&data_path) {
                tracing::error!("FATAL: Unable to create data directory. Permission Denied. ({e})");
            } else {
                let test_file = data_path.join(".write_test");
                if std::fs::write(&test_file, b"OK").is_ok() {
                    let _ = std::fs::remove_file(test_file);
                } else {
                    tracing::error!("WARNING: Cannot write to portable `./carrytalk-data/` directory. Check USB properties or Administrator bounds.");
                }
            }

            // Run crash recovery check on startup
            tauri::async_runtime::spawn(async move {
                if let Err(e) = storage::check_interrupted_sessions(&handle).await {
                    tracing::warn!("Crash recovery check failed: {e}");
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run CarryTalk");
}
