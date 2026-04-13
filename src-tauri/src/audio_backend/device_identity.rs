use crate::types::AudioBackendKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputFingerprint {
    pub normalized_label: String,
    pub default_sample_rate: u32,
    pub default_channels: u16,
    pub default_sample_format: String,
    pub capability_signature: String,
}

pub fn backend_namespace(backend: AudioBackendKind) -> &'static str {
    match backend {
        AudioBackendKind::Cpal => "cpal",
        AudioBackendKind::WindowsSystem => "windows-system",
        AudioBackendKind::MacosSystem => "macos-system",
        AudioBackendKind::LinuxSystem => "linux-system",
    }
}

pub fn normalize_label(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn fingerprint_payload(snapshot: &InputFingerprint) -> String {
    format!(
        "{}|{}|{}|{}|{}",
        snapshot.normalized_label,
        snapshot.default_sample_rate,
        snapshot.default_channels,
        snapshot.default_sample_format,
        snapshot.capability_signature,
    )
}

pub fn versioned_id(namespace: &str, version: &str, payload: &str) -> String {
    namespaced_id(namespace, &format!("{version}:{payload}"))
}

pub fn split_versioned_payload(value: &str) -> Option<(&str, &str)> {
    let (version, payload) = value.split_once(':')?;
    if version.is_empty() || payload.is_empty() {
        return None;
    }

    Some((version, payload))
}

pub fn parse_v2_fingerprint_payload(value: &str) -> Option<&str> {
    let (version, payload) = split_versioned_payload(value)?;
    (version == "v2").then_some(payload)
}

pub fn parse_v2_fingerprint(value: &str) -> Option<InputFingerprint> {
    let payload = parse_v2_fingerprint_payload(value)?;
    let mut parts = payload.splitn(5, '|');
    let normalized_label = normalize_label(parts.next()?);
    let default_sample_rate = parts.next()?.parse().ok()?;
    let default_channels = parts.next()?.parse().ok()?;
    let default_sample_format = parts.next()?.to_string();
    let capability_signature = parts.next()?.to_string();

    if normalized_label.is_empty() {
        return None;
    }

    Some(InputFingerprint {
        normalized_label,
        default_sample_rate,
        default_channels,
        default_sample_format,
        capability_signature,
    })
}

pub fn microphone_rematch_score(saved: &InputFingerprint, candidate: &InputFingerprint) -> u32 {
    let mut score = 0;
    if saved.normalized_label == candidate.normalized_label {
        score += 60;
    }
    if saved.default_sample_rate == candidate.default_sample_rate {
        score += 15;
    }
    if saved.default_channels == candidate.default_channels {
        score += 10;
    }
    if saved.default_sample_format == candidate.default_sample_format {
        score += 5;
    }
    if saved.capability_signature == candidate.capability_signature {
        score += 10;
    }
    score
}

pub fn namespaced_id(namespace: &str, device_id: &str) -> String {
    format!("{namespace}:{device_id}")
}

pub fn split_namespaced_id(id: &str) -> Option<(&str, &str)> {
    let (namespace, device_id) = id.split_once(':')?;
    if namespace.is_empty() || device_id.is_empty() {
        return None;
    }

    Some((namespace, device_id))
}

