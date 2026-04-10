use std::ffi::CStr;
use std::os::raw::{c_char, c_void};
use std::time::Duration;

use chrono::Utc;
use crossbeam_channel::Sender;
use tokio_util::sync::CancellationToken;

use crate::audio_backend::device_identity::{backend_namespace, namespaced_id, split_namespaced_id};
use crate::audio_backend::{SourceStreamFormat, SystemAudioBackend};
use crate::error::{AppError, AppResult};
use crate::types::{AudioBackendKind, AudioSource, AudioSourceDevice, CapturedAudioFrame, PhysicalAudioSource};

const DISPLAY_ID_PREFIX: &str = "display:";
const SCREEN_CAPTUREKIT_OUTPUT_SAMPLE_RATE: u32 = 48_000;
const SCREEN_CAPTUREKIT_OUTPUT_CHANNELS: u16 = 2;

type MacosScStreamHandle = c_void;

unsafe extern "C" {
    fn carrytalk_macos_sc_create_stream(
        display_id: u32,
        callback: extern "C" fn(*const f32, usize, u32, u16, *mut c_void),
        user_data: *mut c_void,
    ) -> *mut MacosScStreamHandle;
    fn carrytalk_macos_sc_start_stream(handle: *mut MacosScStreamHandle) -> bool;
    fn carrytalk_macos_sc_stop_stream(handle: *mut MacosScStreamHandle);
    fn carrytalk_macos_sc_destroy_stream(handle: *mut MacosScStreamHandle);
    fn carrytalk_macos_sc_stream_running(handle: *const MacosScStreamHandle) -> bool;
    fn carrytalk_macos_sc_stream_sample_rate(handle: *const MacosScStreamHandle) -> u32;
    fn carrytalk_macos_sc_stream_channels(handle: *const MacosScStreamHandle) -> u16;
    fn carrytalk_macos_sc_last_error(handle: *const MacosScStreamHandle) -> *const c_char;
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGMainDisplayID() -> u32;
    fn CGGetActiveDisplayList(max_displays: u32, active_displays: *mut u32, display_count: *mut u32) -> i32;
}

#[derive(Debug, Clone, Copy)]
struct DisplayInfo {
    id: u32,
    is_main: bool,
}

pub struct MacosSystemBackend;

impl SystemAudioBackend for MacosSystemBackend {
    fn enumerate_system_devices(&self) -> AppResult<Vec<AudioSourceDevice>> {
        let displays = active_displays()?;
        let screen_access = has_screen_capture_access();

        Ok(displays
            .into_iter()
            .enumerate()
            .map(|(index, display)| AudioSourceDevice {
                id: encode_display_id(display.id),
                label: display_label(index, display),
                source: PhysicalAudioSource::SystemOutput,
                backend: AudioBackendKind::MacosSystem,
                is_default: display.is_main,
                usable: screen_access,
            })
            .collect())
    }

    fn preflight_system_capture(&self, device_id: Option<&str>) -> AppResult<()> {
        let displays = active_displays()?;
        let selected = resolve_display(device_id, &displays)?;

        if !has_screen_capture_access() {
            return Err(AppError::AudioCapture(
                "macOS system audio capture requires Screen Recording permission. Grant access in System Settings > Privacy & Security > Screen & System Audio Recording, then retry.".into(),
            ));
        }

        tracing::debug!(display = %encode_display_id(selected.id), "macOS system audio preflight passed");
        Ok(())
    }

    fn start_system_capture(
        &self,
        device_id: Option<&str>,
        tx: Sender<CapturedAudioFrame>,
        cancel_token: CancellationToken,
    ) -> AppResult<SourceStreamFormat> {
        let displays = active_displays()?;
        let selected = resolve_display(device_id, &displays)?;

        if !has_screen_capture_access() {
            return Err(AppError::AudioCapture(
                "macOS system audio capture requires Screen Recording permission. Grant access in System Settings > Privacy & Security > Screen & System Audio Recording, then retry.".into(),
            ));
        }

        let sender = Box::new(tx);
        let sender_ptr = Box::into_raw(sender) as *mut c_void;
        let handle = unsafe {
            carrytalk_macos_sc_create_stream(selected.id, macos_audio_callback, sender_ptr)
        };
        if handle.is_null() {
            unsafe {
                drop(Box::from_raw(sender_ptr as *mut Sender<CapturedAudioFrame>));
            }
            return Err(AppError::AudioCapture(
                "Failed to allocate macOS ScreenCaptureKit stream handle".into(),
            ));
        }

        let started = unsafe { carrytalk_macos_sc_start_stream(handle) };
        if !started {
            let error = unsafe { last_error_message(handle) }
                .unwrap_or_else(|| "Failed to start macOS ScreenCaptureKit stream".into());
            unsafe {
                carrytalk_macos_sc_destroy_stream(handle);
                drop(Box::from_raw(sender_ptr as *mut Sender<CapturedAudioFrame>));
            }
            return Err(AppError::AudioCapture(error));
        }

        let sample_rate = unsafe { carrytalk_macos_sc_stream_sample_rate(handle) };
        let channels = unsafe { carrytalk_macos_sc_stream_channels(handle) };

        std::thread::spawn(move || {
            while !cancel_token.is_cancelled() && unsafe { carrytalk_macos_sc_stream_running(handle) } {
                std::thread::sleep(Duration::from_millis(20));
            }

            unsafe {
                carrytalk_macos_sc_stop_stream(handle);
                carrytalk_macos_sc_destroy_stream(handle);
                drop(Box::from_raw(sender_ptr as *mut Sender<CapturedAudioFrame>));
            }
            tracing::debug!(display = %encode_display_id(selected.id), "macOS ScreenCaptureKit stream stopped.");
        });

        Ok(SourceStreamFormat {
            source: PhysicalAudioSource::SystemOutput,
            sample_rate: if sample_rate == 0 {
                SCREEN_CAPTUREKIT_OUTPUT_SAMPLE_RATE
            } else {
                sample_rate
            },
            channels: if channels == 0 {
                SCREEN_CAPTUREKIT_OUTPUT_CHANNELS
            } else {
                channels
            },
        })
    }
}

fn active_displays() -> AppResult<Vec<DisplayInfo>> {
    let mut display_count = 0_u32;
    let status = unsafe { CGGetActiveDisplayList(0, std::ptr::null_mut(), &mut display_count) };
    if status != 0 {
        return Err(AppError::AudioCapture(format!(
            "Failed to query active macOS displays: CoreGraphics status {}",
            status
        )));
    }

    if display_count == 0 {
        return Ok(Vec::new());
    }

    let mut display_ids = vec![0_u32; display_count as usize];
    let status = unsafe {
        CGGetActiveDisplayList(
            display_count,
            display_ids.as_mut_ptr(),
            &mut display_count,
        )
    };
    if status != 0 {
        return Err(AppError::AudioCapture(format!(
            "Failed to enumerate active macOS displays: CoreGraphics status {}",
            status
        )));
    }

    let main_display = unsafe { CGMainDisplayID() };
    Ok(display_ids
        .into_iter()
        .take(display_count as usize)
        .map(|id| DisplayInfo {
            id,
            is_main: id == main_display,
        })
        .collect())
}

fn has_screen_capture_access() -> bool {
    unsafe { CGPreflightScreenCaptureAccess() }
}

fn resolve_display(device_id: Option<&str>, displays: &[DisplayInfo]) -> AppResult<DisplayInfo> {
    if displays.is_empty() {
        return Err(AppError::AudioCapture(
            "No active macOS displays available for ScreenCaptureKit system audio capture".into(),
        ));
    }

    if let Some(device_id) = device_id.filter(|value| *value != "default") {
        let requested_id = decode_display_id(device_id)?;
        return displays
            .iter()
            .copied()
            .find(|display| display.id == requested_id)
            .ok_or_else(|| {
                AppError::AudioCapture(format!(
                    "Requested macOS system audio display not found: {}",
                    device_id
                ))
            });
    }

    displays
        .iter()
        .copied()
        .find(|display| display.is_main)
        .or_else(|| displays.first().copied())
        .ok_or_else(|| {
            AppError::AudioCapture(
                "No default macOS display available for system audio capture".into(),
            )
        })
}

fn encode_display_id(display_id: u32) -> String {
    let raw_id = format!("{DISPLAY_ID_PREFIX}{display_id}");
    namespaced_id(backend_namespace(AudioBackendKind::MacosSystem), &raw_id)
}

fn decode_display_id(value: &str) -> AppResult<u32> {
    let raw_value = normalize_macos_system_device_id(value).ok_or_else(|| {
        AppError::AudioCapture(format!(
            "Invalid macOS system audio device id `{value}`; device id namespace does not match macOS system audio"
        ))
    })?;

    let raw = raw_value.strip_prefix(DISPLAY_ID_PREFIX).ok_or_else(|| {
        AppError::AudioCapture(format!(
            "Invalid macOS system audio device id `{value}`; expected `macos-system:{DISPLAY_ID_PREFIX}<cg_display_id>`"
        ))
    })?;

    raw.parse::<u32>().map_err(|_| {
        AppError::AudioCapture(format!(
            "Invalid macOS system audio display id `{value}`; numeric CGDisplayID expected"
        ))
    })
}

fn normalize_macos_system_device_id(device_id: &str) -> Option<String> {
    match split_namespaced_id(device_id) {
        Some((namespace, raw_device_id)) if namespace == backend_namespace(AudioBackendKind::MacosSystem) => {
            Some(raw_device_id.to_string())
        }
        Some(_) => None,
        None => Some(device_id.to_string()),
    }
}

fn display_label(index: usize, display: DisplayInfo) -> String {
    display_label_parts(index, display.id, display.is_main)
}

extern "C" fn macos_audio_callback(
    samples: *const f32,
    sample_count: usize,
    _sample_rate: u32,
    _channels: u16,
    user_data: *mut c_void,
) {
    if samples.is_null() || sample_count == 0 || user_data.is_null() {
        return;
    }

    let sender = unsafe { &*(user_data as *const Sender<CapturedAudioFrame>) };
    let slice = unsafe { std::slice::from_raw_parts(samples, sample_count) };
    let frame = CapturedAudioFrame {
        captured_at: Utc::now(),
        source: AudioSource::System,
        samples: slice.to_vec(),
    };
    if let Err(error) = sender.send(frame) {
        tracing::debug!("Stopping macOS system audio callback because receiver closed: {error}");
    }
}

unsafe fn last_error_message(handle: *const MacosScStreamHandle) -> Option<String> {
    let raw = carrytalk_macos_sc_last_error(handle);
    if raw.is_null() {
        return None;
    }
    Some(CStr::from_ptr(raw).to_string_lossy().into_owned())
}

fn display_label_from_info(display: DisplayInfo) -> String {
    display_label_parts(0, display.id, display.is_main)
}

fn display_label_parts(index: usize, display_id: u32, is_main: bool) -> String {
    if is_main {
        format!("Main Display ({})", encode_display_id(display_id))
    } else {
        format!("Display {} ({})", index + 1, encode_display_id(display_id))
    }
}

#[allow(dead_code)]
fn screen_capturekit_output_format() -> SourceStreamFormat {
    SourceStreamFormat {
        source: PhysicalAudioSource::SystemOutput,
        sample_rate: SCREEN_CAPTUREKIT_OUTPUT_SAMPLE_RATE,
        channels: SCREEN_CAPTUREKIT_OUTPUT_CHANNELS,
    }
}
