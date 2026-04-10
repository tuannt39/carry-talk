use std::sync::mpsc;

use chrono::Utc;
use crossbeam_channel::Sender;
use tokio_util::sync::CancellationToken;
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::{
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
    AUDCLNT_STREAMFLAGS_LOOPBACK, DEVICE_STATE_ACTIVE, IAudioCaptureClient, IAudioClient,
    IMMDevice, IMMDeviceCollection, IMMDeviceEnumerator, MMDeviceEnumerator, WAVEFORMATEX,
    WAVEFORMATEXTENSIBLE, eConsole, eRender,
};
use windows::Win32::Media::KernelStreaming::WAVE_FORMAT_EXTENSIBLE;
use windows::Win32::Media::Multimedia::{KSDATAFORMAT_SUBTYPE_IEEE_FLOAT, WAVE_FORMAT_IEEE_FLOAT};
use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize};
use windows::Win32::System::Variant::VT_LPWSTR;
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::core::{Error as WindowsError, PWSTR};

use crate::audio_backend::device_identity::{backend_namespace, namespaced_id, split_namespaced_id};
use crate::audio_backend::{SourceStreamFormat, SystemAudioBackend};
use crate::error::{AppError, AppResult};
use crate::types::{
    AudioBackendKind, AudioSource, AudioSourceDevice, CapturedAudioFrame, PhysicalAudioSource,
};

const REFTIMES_PER_MS: i64 = 10_000;
const LOOPBACK_POLL_MS: u64 = 200;

pub struct WindowsSystemBackend;

impl SystemAudioBackend for WindowsSystemBackend {
    fn enumerate_system_devices(&self) -> AppResult<Vec<AudioSourceDevice>> {
        enumerate_loopback_devices()
    }

    fn preflight_system_capture(&self, device_id: Option<&str>) -> AppResult<()> {
        preflight_loopback_capture(device_id)
    }

    fn start_system_capture(
        &self,
        device_id: Option<&str>,
        tx: Sender<CapturedAudioFrame>,
        cancel_token: CancellationToken,
    ) -> AppResult<SourceStreamFormat> {
        start_loopback_capture(device_id, tx, cancel_token)
    }
}

fn enumerate_loopback_devices() -> AppResult<Vec<AudioSourceDevice>> {
    run_mta_task(|| {
        let enumerator = create_device_enumerator()?;
        let default_id = default_render_device_id(&enumerator)?;
        let devices = enum_active_render_devices(&enumerator)?;

        let mut result = Vec::with_capacity(devices.len());
        for device in devices {
            let raw_id = device_id(&device)?;
            let label = device_friendly_name(&device).unwrap_or_else(|_| raw_id.clone());
            let id = namespaced_id(backend_namespace(AudioBackendKind::WindowsSystem), &raw_id);
            result.push(AudioSourceDevice {
                id,
                label,
                source: PhysicalAudioSource::SystemOutput,
                backend: AudioBackendKind::WindowsSystem,
                is_default: default_id.as_deref() == Some(raw_id.as_str()),
                usable: true,
            });
        }

        Ok(result)
    })
}

fn start_loopback_capture(
    requested_device_id: Option<&str>,
    tx: Sender<CapturedAudioFrame>,
    cancel_token: CancellationToken,
) -> AppResult<SourceStreamFormat> {
    let requested_device_id = requested_device_id.map(str::to_owned);
    let (startup_tx, startup_rx) = mpsc::sync_channel(1);

    std::thread::spawn(move || {
        let mut startup_sent = false;
        let thread_result = run_loopback_capture_thread(
            requested_device_id,
            tx,
            cancel_token,
            &startup_tx,
            &mut startup_sent,
        );
        if let Err(err) = thread_result {
            if !startup_sent {
                let _ = startup_tx.send(Err(err));
            } else {
                tracing::error!("Windows loopback capture thread exited: {err}");
            }
        }
    });

    let thread_format = startup_rx.recv().map_err(|_| {
        AppError::AudioCapture("Windows system capture thread exited before startup completed".into())
    })??;

    Ok(thread_format)
}

fn run_loopback_capture_thread(
    requested_device_id: Option<String>,
    tx: Sender<CapturedAudioFrame>,
    cancel_token: CancellationToken,
    startup_tx: &mpsc::SyncSender<AppResult<SourceStreamFormat>>,
    startup_sent: &mut bool,
) -> AppResult<()> {
    let _com = ComScope::new()?;
    let enumerator = create_device_enumerator()?;
    let device = resolve_render_device(&enumerator, requested_device_id.as_deref())?;
    let selected_device_id = device_id(&device)?;
    let selected_device_label = device_friendly_name(&device).unwrap_or_else(|_| selected_device_id.clone());

    let audio_client: IAudioClient = unsafe {
        device
            .Activate(CLSCTX_ALL, None)
            .map_err(windows_audio_error)?
    };

    let mix_format = get_mix_format(&audio_client)?;
    validate_loopback_format(&mix_format)?;
    let wave = mix_format.wave();

    let block_align = wave.nBlockAlign as usize;
    let channels = wave.nChannels;
    let sample_rate = wave.nSamplesPerSec;
    let buffer_duration = REFTIMES_PER_MS * 200;

    let event_handle = unsafe { windows::Win32::System::Threading::CreateEventW(None, false, false, None) }
        .map_err(windows_audio_error)?;
    let event_handle = EventHandle::new(event_handle);

    unsafe {
        audio_client
            .Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                buffer_duration,
                0,
                mix_format.as_ptr(),
                None,
            )
            .map_err(windows_audio_error)?;

        audio_client
            .SetEventHandle(event_handle.raw())
            .map_err(windows_audio_error)?;
    }

    let capture_client: IAudioCaptureClient = unsafe {
        audio_client
            .GetService()
            .map_err(windows_audio_error)?
    };

    unsafe {
        audio_client.Start().map_err(windows_audio_error)?;
    }

    let started_format = SourceStreamFormat {
        source: PhysicalAudioSource::SystemOutput,
        sample_rate,
        channels,
    };

    startup_tx
        .send(Ok(started_format.clone()))
        .map_err(|_| AppError::AudioCapture("Windows loopback startup receiver closed".into()))?;
    *startup_sent = true;

    tracing::info!(
        device = %selected_device_label,
        sample_rate,
        channels,
        "Windows WASAPI loopback capture started."
    );

    loop {
        if cancel_token.is_cancelled() {
            break;
        }

        let wait_result = unsafe {
            windows::Win32::System::Threading::WaitForSingleObject(event_handle.raw(), LOOPBACK_POLL_MS as u32)
        };

        if wait_result != WAIT_OBJECT_0 {
            continue;
        }

        drain_available_packets(&capture_client, sample_rate, channels, block_align, tx.clone())?;
    }

    unsafe {
        let _ = audio_client.Stop();
    }
    tracing::debug!(device = %selected_device_label, "Windows WASAPI loopback capture stopped.");

    Ok(())
}

fn drain_available_packets(
    capture_client: &IAudioCaptureClient,
    sample_rate: u32,
    channels: u16,
    block_align: usize,
    tx: Sender<CapturedAudioFrame>,
) -> AppResult<()> {
    loop {
        let next_packet_size = unsafe { capture_client.GetNextPacketSize().map_err(windows_audio_error)? };

        if next_packet_size == 0 {
            break;
        }

        let mut data_ptr = std::ptr::null_mut();
        let mut frames_available = 0u32;
        let mut flags = 0u32;

        unsafe {
            capture_client
                .GetBuffer(
                    &mut data_ptr,
                    &mut frames_available,
                    &mut flags,
                    None,
                    None,
                )
                .map_err(windows_audio_error)?;
        }

        let samples = if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 {
            vec![0.0_f32; frames_available as usize * channels as usize]
        } else {
            let byte_len = frames_available as usize * block_align;
            let bytes = unsafe { std::slice::from_raw_parts(data_ptr.cast::<u8>(), byte_len) };
            pcm_f32_from_wave_bytes(bytes, channels)?
        };

        unsafe {
            capture_client
                .ReleaseBuffer(frames_available)
                .map_err(windows_audio_error)?;
        }

        if samples.is_empty() {
            continue;
        }

        tx.send(CapturedAudioFrame {
            captured_at: Utc::now(),
            source: AudioSource::System,
            samples,
        })
        .map_err(|err| AppError::AudioCapture(format!("Windows loopback receiver closed: {err}")))?;

        let _ = sample_rate;
    }

    Ok(())
}

fn pcm_f32_from_wave_bytes(bytes: &[u8], channels: u16) -> AppResult<Vec<f32>> {
    if !bytes.len().is_multiple_of(4) {
        return Err(AppError::AudioCapture(
            "Windows loopback delivered non-f32-aligned audio buffer".into(),
        ));
    }

    let _ = channels;
    let mut samples = Vec::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        samples.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    Ok(samples)
}

fn preflight_loopback_capture(device_id: Option<&str>) -> AppResult<()> {
    let requested_device_id = device_id.map(str::to_owned);
    run_mta_task(move || {
        let enumerator = create_device_enumerator()?;
        let _ = resolve_render_device(&enumerator, requested_device_id.as_deref())?;
        Ok(())
    })
}

fn run_mta_task<T, F>(task: F) -> AppResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> AppResult<T> + Send + 'static,
{
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let result = (|| {
            let _com = ComScope::new()?;
            task()
        })();
        let _ = tx.send(result);
    });

    rx.recv().map_err(|_| {
        AppError::AudioCapture("Windows worker thread exited before completing audio task".into())
    })?
}

fn resolve_render_device(enumerator: &IMMDeviceEnumerator, device_id: Option<&str>) -> AppResult<IMMDevice> {
    if let Some(device_id) = device_id.filter(|value| *value != "default") {
        let raw_device_id = normalize_windows_system_device_id(device_id).ok_or_else(|| {
            AppError::AudioCapture(format!(
                "Windows system output device id is invalid for this backend: {device_id}"
            ))
        })?;

        unsafe {
            enumerator
                .GetDevice(to_pcwstr(&raw_device_id).raw())
                .map_err(|err| {
                    AppError::AudioCapture(format!("Windows system output device not found `{device_id}`: {err}"))
                })
        }
    } else {
        default_render_device(enumerator)
    }
}

fn normalize_windows_system_device_id(device_id: &str) -> Option<String> {
    match split_namespaced_id(device_id) {
        Some((namespace, raw_device_id)) if namespace == backend_namespace(AudioBackendKind::WindowsSystem) => {
            Some(raw_device_id.to_string())
        }
        Some(_) => None,
        None => Some(device_id.to_string()),
    }
}

fn validate_loopback_format(format: &WaveFormat) -> AppResult<()> {
    let wave = format.wave();
    let bits_per_sample = wave.wBitsPerSample;
    let tag = wave.wFormatTag as u32;
    let channels = wave.nChannels;
    let sample_rate = wave.nSamplesPerSec;
    let block_align = wave.nBlockAlign;

    if bits_per_sample != 32 {
        return Err(AppError::AudioCapture(format!(
            "Windows loopback currently expects 32-bit float mix format, got {} bits",
            bits_per_sample
        )));
    }

    let is_float = if tag == WAVE_FORMAT_IEEE_FLOAT {
        true
    } else if tag == WAVE_FORMAT_EXTENSIBLE {
        format
            .extensible_sub_format()
            .is_some_and(|sub| sub == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT)
    } else {
        false
    };

    if !is_float {
        return Err(AppError::AudioCapture(
            "Windows loopback currently supports only float mix formats".into(),
        ));
    }

    if channels == 0 || sample_rate == 0 || block_align == 0 {
        return Err(AppError::AudioCapture(
            "Windows loopback reported invalid stream format metadata".into(),
        ));
    }

    Ok(())
}

fn create_device_enumerator() -> AppResult<IMMDeviceEnumerator> {
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.map_err(windows_audio_error)
}

fn default_render_device(enumerator: &IMMDeviceEnumerator) -> AppResult<IMMDevice> {
    unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eConsole) }.map_err(windows_audio_error)
}

fn default_render_device_id(enumerator: &IMMDeviceEnumerator) -> AppResult<Option<String>> {
    default_render_device(enumerator)
        .and_then(|device| device_id(&device).map(Some))
        .or_else(|err| {
            tracing::debug!("Unable to resolve default Windows render device id: {err}");
            Ok(None)
        })
}

fn enum_active_render_devices(enumerator: &IMMDeviceEnumerator) -> AppResult<Vec<IMMDevice>> {
    let collection: IMMDeviceCollection = unsafe {
        enumerator
            .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)
            .map_err(windows_audio_error)?
    };

    let count = unsafe { collection.GetCount().map_err(windows_audio_error)? };

    let mut devices = Vec::with_capacity(count as usize);
    for index in 0..count {
        let device = unsafe { collection.Item(index).map_err(windows_audio_error)? };
        devices.push(device);
    }
    Ok(devices)
}

fn device_id(device: &IMMDevice) -> AppResult<String> {
    let raw = unsafe { device.GetId().map_err(windows_audio_error)? };
    pwstr_to_string(raw)
}

fn device_friendly_name(device: &IMMDevice) -> AppResult<String> {
    use windows::Win32::System::Com::StructuredStorage::PropVariantClear;

    let store: IPropertyStore = unsafe {
        device
            .OpenPropertyStore(windows::Win32::System::Com::STGM_READ)
            .map_err(windows_audio_error)?
    };
    let mut value = unsafe {
        store
            .GetValue(&PKEY_Device_FriendlyName)
            .map_err(windows_audio_error)?
    };

    let result = propvariant_to_string(&value);

    unsafe {
        let _ = PropVariantClear(&mut value);
    }

    result
}

fn propvariant_to_string(value: &windows::Win32::System::Com::StructuredStorage::PROPVARIANT) -> AppResult<String> {
    unsafe {
        let prop_variant = &value.Anonymous.Anonymous;
        if prop_variant.vt != VT_LPWSTR {
            return Err(AppError::AudioCapture(
                "Windows device property was not a UTF-16 string".into(),
            ));
        }

        let raw_ptr = *(&prop_variant.Anonymous as *const _ as *const *mut u16);
        if raw_ptr.is_null() {
            return Err(AppError::AudioCapture(
                "Windows device property string was null".into(),
            ));
        }

        pwstr_to_string(PWSTR(raw_ptr))
    }
}

fn get_mix_format(audio_client: &IAudioClient) -> AppResult<WaveFormat> {
    let format_ptr = unsafe { audio_client.GetMixFormat().map_err(windows_audio_error)? };
    WaveFormat::new(format_ptr)
}

fn pwstr_to_string(raw: PWSTR) -> AppResult<String> {
    unsafe { raw.to_string() }
        .map_err(|err| AppError::AudioCapture(format!("Windows string conversion failed: {err}")))
}

fn to_pcwstr(value: &str) -> WideString {
    let wide = value.encode_utf16().chain(std::iter::once(0)).collect();
    WideString(wide)
}

fn windows_audio_error(err: WindowsError) -> AppError {
    AppError::AudioCapture(format!("Windows audio backend error: {err}"))
}

struct WideString(Vec<u16>);
impl WideString {
    fn raw(&self) -> windows::core::PCWSTR {
        windows::core::PCWSTR(self.0.as_ptr())
    }
}

struct ComScope;
impl ComScope {
    fn new() -> AppResult<Self> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED)
                .ok()
                .map_err(windows_audio_error)?;
        }
        Ok(Self)
    }
}
impl Drop for ComScope {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}

struct EventHandle(HANDLE);
impl EventHandle {
    fn new(handle: HANDLE) -> Self {
        Self(handle)
    }

    fn raw(&self) -> HANDLE {
        self.0
    }
}
impl Drop for EventHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

struct WaveFormat {
    ptr: *mut WAVEFORMATEX,
}
impl WaveFormat {
    fn new(ptr: *mut WAVEFORMATEX) -> AppResult<Self> {
        if ptr.is_null() {
            return Err(AppError::AudioCapture(
                "Windows audio client returned null mix format".into(),
            ));
        }
        Ok(Self { ptr })
    }

    fn as_ptr(&self) -> *const WAVEFORMATEX {
        self.ptr.cast_const()
    }

    fn wave(&self) -> &WAVEFORMATEX {
        unsafe { &*self.ptr }
    }

    fn extensible_sub_format(&self) -> Option<windows::core::GUID> {
        if self.wave().wFormatTag as u32 != WAVE_FORMAT_EXTENSIBLE {
            return None;
        }
        let ext = unsafe { &*(self.ptr as *const WAVEFORMATEXTENSIBLE) };
        Some(ext.SubFormat)
    }
}
impl Drop for WaveFormat {
    fn drop(&mut self) {
        unsafe {
            CoTaskMemFree(Some(self.ptr.cast()));
        }
    }
}
