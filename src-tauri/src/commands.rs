use tauri::State;

use crate::AppState;
use crate::audio_capabilities;
use crate::audio_capture;
use crate::error::{AppError, AppResult};
use crate::secrets::SONIOX_API_KEY_SECRET_ID;
use crate::session_manager;
use crate::storage;
use crate::types::{
    AppSettings, AudioCapabilityReason, AudioCaptureMode, AudioDevice,
    AudioRuntimeCapabilities, ProviderSettings, SessionState, SessionSummary,
};

// ── Settings ────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> AppResult<AppSettings> {
    let settings = state.settings.lock().await;
    Ok(settings.get().clone())
}

#[tauri::command]
pub async fn save_settings(state: State<'_, AppState>, settings: AppSettings) -> AppResult<()> {
    let mut s = state.settings.lock().await;
    s.update(settings)
}

#[tauri::command]
pub async fn save_settings_and_api_key(
    state: State<'_, AppState>,
    settings: AppSettings,
    api_key: Option<String>,
) -> AppResult<()> {
    tracing::info!("save_settings_and_api_key called");

    let next_api_key = api_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);

    let previous_api_key = {
        let store = state.secret_store.lock().await;
        if store.has_runtime_api_key()? {
            Some(store.runtime_api_key()?)
        } else {
            None
        }
    };

    if let Some(key) = &next_api_key {
        let mut store = state.secret_store.lock().await;
        store.upsert_api_key(key)?;
    }

    let save_result = {
        let mut s = state.settings.lock().await;
        s.update(settings)
    };

    if let Err(err) = save_result {
        tracing::warn!(error = %err, "Saving settings failed after API key handling");
        if next_api_key.is_some() {
            let mut store = state.secret_store.lock().await;
            match previous_api_key {
                Some(previous_key) => {
                    let _ = store.upsert_api_key(&previous_key);
                }
                None => {
                    let _ = store.clear_api_key();
                }
            }
        }

        return Err(err);
    }

    Ok(())
}

// ── Session Lifecycle ───────────────────────────────────────────────────────

fn selected_target_lang(
    provider_settings: &ProviderSettings,
    override_target: Option<String>,
) -> String {
    if provider_settings.translation_enabled {
        override_target.unwrap_or_else(|| provider_settings.translation_target_language.clone())
    } else {
        String::new()
    }
}

fn api_key_secret_id_for_provider(provider: &str) -> AppResult<&'static str> {
    match provider {
        "soniox" => Ok(SONIOX_API_KEY_SECRET_ID),
        _ => Err(AppError::Auth(format!(
            "API key mapping not configured for provider `{provider}`"
        ))),
    }
}

fn validate_audio_mode(
    mode: &AudioCaptureMode,
    capabilities: &AudioRuntimeCapabilities,
) -> AppResult<()> {
    match mode {
        AudioCaptureMode::Mic if capabilities.microphone.usable => Ok(()),
        AudioCaptureMode::Mic => Err(AppError::AudioCapture(format_audio_reason(
            capabilities.microphone.reason.as_ref(),
            "Microphone capture is unavailable on this runtime",
        ))),
        AudioCaptureMode::System if capabilities.system_output.usable => Ok(()),
        AudioCaptureMode::System => {
            #[cfg(not(target_os = "linux"))]
            if capabilities.microphone.usable {
                return Ok(());
            }

            Err(AppError::AudioCapture(format_audio_reason(
                capabilities.system_output.reason.as_ref(),
                "System audio is unavailable on this runtime",
            )))
        }
        AudioCaptureMode::Mixed if capabilities.mixed_supported => Ok(()),
        AudioCaptureMode::Mixed => {
            #[cfg(not(target_os = "linux"))]
            if capabilities.microphone.usable {
                return Ok(());
            }

            Err(AppError::AudioCapture(format_audio_reason(
                capabilities.mixed_reason.as_ref(),
                "Mixed audio is unavailable on this runtime",
            )))
        }
    }
}

fn format_audio_reason(reason: Option<&AudioCapabilityReason>, fallback: &str) -> String {
    match reason {
        Some(reason) => match reason.detail.as_deref() {
            Some(detail) if !detail.trim().is_empty() => detail.to_string(),
            _ => fallback.into(),
        },
        None => fallback.into(),
    }
}

#[tauri::command]
pub async fn start_session(
    state: State<'_, AppState>,
    handle: tauri::AppHandle,
    source_lang: Option<String>,
    target_lang: Option<String>,
) -> AppResult<String> {
    let settings = state.settings.lock().await;
    let app_settings = settings.get().clone();
    drop(settings);
    let provider_settings = &app_settings.provider;
    let audio_settings = app_settings.audio.clone();
    let capabilities = audio_capabilities::query_audio_runtime_capabilities()?;

    validate_audio_mode(&audio_settings.capture_mode, &capabilities)?;

    let provider_name = provider_settings.name.clone();
    let source_lang = source_lang.unwrap_or_else(|| provider_settings.source_language.clone());
    let target_lang = selected_target_lang(provider_settings, target_lang);
    let secret_id = api_key_secret_id_for_provider(&provider_name)?;

    let capture_config = audio_capture::CaptureConfig {
        mode: audio_settings.capture_mode.clone(),
        mic_device_id: Some(audio_settings.mic_device_id.clone()),
        system_device_id: Some(audio_settings.system_device_id.clone()),
    };
    audio_capture::preflight_capture(&capture_config)?;

    let secret_store = state.secret_store.lock().await;
    let api_key = secret_store.get_secret(secret_id)?;
    drop(secret_store);

    let start_context = {
        let mut sm = state.session_manager.lock().await;
        sm.prepare_start(&handle, &source_lang, &target_lang)?
    };

    let pipeline = {
        let mut sm = state.session_manager.lock().await;
        sm.prepare_streaming_pipeline(&handle, &audio_settings)?
    };

    let start_result = session_manager::SessionManager::start_streaming_pipeline(
        &handle,
        state.session_manager.clone(),
        &api_key,
        &source_lang,
        &target_lang,
        pipeline,
        &start_context,
    )
    .await;

    match start_result {
        Ok(()) => Ok(start_context.session_id.clone()),
        Err(err) => {
            let mut sm = state.session_manager.lock().await;
            sm.rollback_prepared_start(&handle, &start_context);
            Err(err)
        }
    }
}

#[tauri::command]
pub async fn stop_session(state: State<'_, AppState>, handle: tauri::AppHandle) -> AppResult<()> {
    let mut sm = state.session_manager.lock().await;
    sm.stop(&handle).await
}

#[tauri::command]
pub async fn pause_session(state: State<'_, AppState>, handle: tauri::AppHandle) -> AppResult<()> {
    let mut sm = state.session_manager.lock().await;
    sm.pause(&handle)
}

#[tauri::command]
pub async fn resume_session(state: State<'_, AppState>, handle: tauri::AppHandle) -> AppResult<()> {
    let settings = state.settings.lock().await;
    let app_settings = settings.get().clone();
    drop(settings);
    let audio_settings = app_settings.audio.clone();

    let (provider_name, source_lang, target_lang, start_context, pipeline) = {
        let mut sm = state.session_manager.lock().await;
        let (provider_name, source_lang, target_lang) = sm.resume_context()?;
        let (start_context, pipeline) = sm.prepare_resume(&handle, &audio_settings)?;
        (provider_name, source_lang, target_lang, start_context, pipeline)
    };

    let runtime_target_lang = if app_settings.provider.translation_enabled {
        target_lang
    } else {
        String::new()
    };
    let secret_id = api_key_secret_id_for_provider(&provider_name)?;

    let secret_store = state.secret_store.lock().await;
    let api_key = secret_store.get_secret(secret_id)?;
    drop(secret_store);

    session_manager::SessionManager::start_streaming_pipeline(
        &handle,
        state.session_manager.clone(),
        &api_key,
        &source_lang,
        &runtime_target_lang,
        pipeline,
        &start_context,
    )
    .await
}

#[tauri::command]
pub async fn get_session_state(state: State<'_, AppState>) -> AppResult<SessionState> {
    let sm = state.session_manager.lock().await;
    Ok(sm.state().clone())
}

#[tauri::command]
pub async fn get_audio_runtime_capabilities() -> AppResult<AudioRuntimeCapabilities> {
    audio_capabilities::query_audio_runtime_capabilities()
}

// ── Audio Devices ───────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_audio_devices() -> AppResult<Vec<AudioDevice>> {
    audio_capture::list_devices()
}

// ── Secrets ─────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn set_api_key(state: State<'_, AppState>, key: String) -> AppResult<()> {
    let mut store = state.secret_store.lock().await;
    store.upsert_api_key(&key)
}

#[tauri::command]
pub async fn has_api_key(state: State<'_, AppState>) -> AppResult<bool> {
    let store = state.secret_store.lock().await;
    store.has_runtime_api_key()
}

// ── Sessions History ────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_sessions() -> AppResult<Vec<SessionSummary>> {
    storage::list_sessions()
}
