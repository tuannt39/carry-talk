use cpal::traits::{DeviceTrait, HostTrait};

use crate::error::AppResult;

pub fn selected_input_host() -> AppResult<cpal::Host> {
    best_input_host_by(score_generic_input_host)
}

pub fn best_input_host_by<F>(mut scorer: F) -> AppResult<cpal::Host>
where
    F: FnMut(&cpal::Host, &str) -> i32,
{
    let available_hosts = cpal::available_hosts();
    tracing::debug!(hosts = ?available_hosts, "Enumerating CPAL host candidates.");

    let mut best: Option<(i32, String, cpal::Host)> = None;

    for host_id in available_hosts {
        let host_label = format!("{host_id:?}");
        let host = match cpal::host_from_id(host_id) {
            Ok(host) => host,
            Err(err) => {
                tracing::debug!(host = %host_label, error = %err, "Skipping unavailable CPAL host.");
                continue;
            }
        };

        let score = scorer(&host, &host_label);
        tracing::debug!(host = %host_label, score, "Scored CPAL host candidate.");

        match &best {
            Some((best_score, _, _)) if *best_score >= score => {}
            _ => best = Some((score, host_label, host)),
        }
    }

    if let Some((score, host_label, host)) = best {
        tracing::debug!(host = %host_label, score, "Selected CPAL host candidate.");
        return Ok(host);
    }

    let fallback = cpal::default_host();
    tracing::debug!("Falling back to CPAL default host because no scored host was selected.");
    Ok(fallback)
}

fn score_generic_input_host(host: &cpal::Host, host_label: &str) -> i32 {
    let mut score = linux_host_bias(host_label);
    let Ok(devices) = host.input_devices() else {
        return score - 10_000;
    };

    score += 100;
    let mut device_count = 0;
    let mut usable_count = 0;
    for device in devices {
        device_count += 1;
        if device.default_input_config().is_ok() {
            usable_count += 1;
        }
    }

    score += device_count.min(8);
    score += usable_count.min(8) * 20;

    if let Some(default_device) = host.default_input_device() {
        score += 10;
        if default_device.default_input_config().is_ok() {
            score += 30;
        }
    }

    score
}

#[cfg(target_os = "linux")]
fn linux_host_bias(host_label: &str) -> i32 {
    let normalized = host_label.to_ascii_lowercase();
    if normalized.contains("jack") {
        return -1_000;
    }
    if normalized.contains("alsa") {
        return 25;
    }
    if normalized.contains("pulse") || normalized.contains("pipewire") {
        return 40;
    }

    0
}

#[cfg(not(target_os = "linux"))]
fn linux_host_bias(_host_label: &str) -> i32 {
    0
}
