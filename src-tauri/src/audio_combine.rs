use chrono::Utc;

use crate::types::{AudioSource, NormalizedAudioFrame, SourceActivity};

pub fn combine_mixed_tick(
    mic_frame: Option<NormalizedAudioFrame>,
    system_frame: Option<NormalizedAudioFrame>,
) -> Option<NormalizedAudioFrame> {
    let activity = SourceActivity {
        mic_active: mic_frame
            .as_ref()
            .is_some_and(|frame| frame.activity.mic_active),
        system_active: system_frame
            .as_ref()
            .is_some_and(|frame| frame.activity.system_active),
    };

    let captured_at = match (mic_frame.as_ref(), system_frame.as_ref()) {
        (Some(mic), Some(system)) => mic.captured_at.min(system.captured_at),
        (Some(mic), None) => mic.captured_at,
        (None, Some(system)) => system.captured_at,
        (None, None) => Utc::now(),
    };

    let duration_ms = match (mic_frame.as_ref(), system_frame.as_ref()) {
        (Some(mic), Some(system)) => mic.duration_ms.max(system.duration_ms),
        (Some(mic), None) => mic.duration_ms,
        (None, Some(system)) => system.duration_ms,
        (None, None) => return None,
    };

    let mic_pcm = mic_frame.as_ref().map(|frame| frame.pcm_bytes.as_slice());
    let system_pcm = system_frame.as_ref().map(|frame| frame.pcm_bytes.as_slice());
    let mixed_pcm = mix_pcm16_le(mic_pcm, system_pcm);

    Some(NormalizedAudioFrame {
        captured_at,
        duration_ms,
        source: AudioSource::Mixed,
        pcm_bytes: mixed_pcm,
        activity,
    })
}

pub fn mixed_is_active(activity: &SourceActivity) -> bool {
    activity.mic_active || activity.system_active
}

fn mix_pcm16_le(left: Option<&[u8]>, right: Option<&[u8]>) -> Vec<u8> {
    let max_len = left
        .map(|bytes| bytes.len())
        .unwrap_or(0)
        .max(right.map(|bytes| bytes.len()).unwrap_or(0));

    let mut out = Vec::with_capacity(max_len);
    let sample_count = max_len / 2;

    for index in 0..sample_count {
        let offset = index * 2;
        let left_sample = sample_at(left, offset);
        let right_sample = sample_at(right, offset);
        let mixed = ((left_sample as i32 + right_sample as i32) / 2)
            .clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        out.extend_from_slice(&mixed.to_le_bytes());
    }

    out
}

fn sample_at(bytes: Option<&[u8]>, offset: usize) -> i16 {
    let Some(bytes) = bytes else {
        return 0;
    };
    if offset + 1 >= bytes.len() {
        return 0;
    }
    i16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}
