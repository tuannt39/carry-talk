use crossbeam_channel::Sender;
use tokio_util::sync::CancellationToken;

use crate::error::AppResult;
use crate::types::{AudioBackendKind, AudioSourceDevice, CapturedAudioFrame, PhysicalAudioSource};

#[derive(Debug, Clone)]
pub struct SourceStreamFormat {
    pub source: PhysicalAudioSource,
    pub sample_rate: u32,
    pub channels: u16,
}

#[derive(Debug, Clone)]
pub struct CaptureRuntime {
    pub formats: Vec<SourceStreamFormat>,
    pub mixed: bool,
    pub warnings: Vec<String>,
}

pub trait SystemAudioBackend {
    fn enumerate_system_devices(&self) -> AppResult<Vec<AudioSourceDevice>>;
    fn preflight_system_capture(&self, device_id: Option<&str>) -> AppResult<()>;
    fn start_system_capture(
        &self,
        device_id: Option<&str>,
        tx: Sender<CapturedAudioFrame>,
        cancel_token: CancellationToken,
    ) -> AppResult<SourceStreamFormat>;
}

pub mod device_identity;
pub mod host_selection;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
use linux::LinuxSystemBackend as CurrentPlatformSystemBackend;
#[cfg(target_os = "macos")]
use macos::MacosSystemBackend as CurrentPlatformSystemBackend;
#[cfg(target_os = "windows")]
use windows::WindowsSystemBackend as CurrentPlatformSystemBackend;

pub fn current_platform_system_backend() -> AudioBackendKind {
    #[cfg(target_os = "windows")]
    {
        AudioBackendKind::WindowsSystem
    }
    #[cfg(target_os = "macos")]
    {
        AudioBackendKind::MacosSystem
    }
    #[cfg(target_os = "linux")]
    {
        AudioBackendKind::LinuxSystem
    }
}

pub fn enumerate_system_devices() -> AppResult<Vec<AudioSourceDevice>> {
    CurrentPlatformSystemBackend.enumerate_system_devices()
}

pub fn preflight_system_capture(device_id: Option<&str>) -> AppResult<()> {
    CurrentPlatformSystemBackend.preflight_system_capture(device_id)
}

pub fn start_system_capture(
    device_id: Option<&str>,
    tx: Sender<CapturedAudioFrame>,
    cancel_token: CancellationToken,
) -> AppResult<SourceStreamFormat> {
    CurrentPlatformSystemBackend.start_system_capture(device_id, tx, cancel_token)
}

