use rubato::{
    audioadapter_buffers::owned::InterleavedOwned, Async, FixedAsync, Indexing, Resampler,
    SincInterpolationParameters, SincInterpolationType, WindowFunction,
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use std::collections::{HashMap, VecDeque};

use crate::audio_backend::CaptureRuntime;
use crate::audio_combine::combine_mixed_tick;
use crate::error::{AppError, AppResult};
use crate::types::{
    AudioSource, CapturedAudioFrame, NormalizedAudioFrame, PhysicalAudioSource, ResampledAudioFrame,
    SourceActivity,
};
use crate::websocket_client::WsEvent;

#[derive(Debug, Clone, Copy)]
pub struct AudioProcessingConfig {
    pub mic_gain: f32,
    pub system_gain: f32,
    pub mic_silence_threshold: i16,
    pub system_silence_threshold: i16,
}

fn apply_gain(samples: &[f32], gain: f32) -> Vec<f32> {
    samples
        .iter()
        .map(|sample| (sample * gain).clamp(-1.0, 1.0))
        .collect()
}

pub struct AudioResampler {
    channels: u16,
    processing: AudioProcessingConfig,
    rubato_mono: Option<Async<f32>>,
    mono_buffer: Vec<f32>,
    tail_captured_at: Option<chrono::DateTime<chrono::Utc>>,
    tail_source: Option<AudioSource>,
}

impl AudioResampler {
    pub fn new(
        in_rate: u32,
        out_rate: u32,
        channels: u16,
        processing: AudioProcessingConfig,
    ) -> AppResult<Self> {
        let rubato_mono = if in_rate != out_rate {
            let params = SincInterpolationParameters {
                sinc_len: 256,
                f_cutoff: 0.95,
                interpolation: SincInterpolationType::Linear,
                oversampling_factor: 128,
                window: WindowFunction::BlackmanHarris2,
            };

            let resampler = Async::<f32>::new_sinc(
                out_rate as f64 / in_rate as f64,
                2.0,
                &params,
                1024,
                1,
                FixedAsync::Input,
            )
            .map_err(|e| AppError::Resampler(e.to_string()))?;
            Some(resampler)
        } else {
            None
        };

        Ok(Self {
            channels,
            processing,
            rubato_mono,
            mono_buffer: Vec::with_capacity(4096),
            tail_captured_at: None,
            tail_source: None,
        })
    }

    pub fn process(&mut self, frame: CapturedAudioFrame) -> AppResult<Vec<ResampledAudioFrame>> {
        let mono = self.downmix_to_mono(&frame.samples);
        let gain = match frame.source {
            AudioSource::Mic => self.processing.mic_gain,
            AudioSource::System => self.processing.system_gain,
            AudioSource::Mixed => 1.0,
        };
        let mono = apply_gain(&mono, gain);
        let mut output_frames = Vec::new();

        if let Some(resampler) = &mut self.rubato_mono {
            if self.mono_buffer.is_empty() {
                self.tail_captured_at = Some(frame.captured_at);
                self.tail_source = Some(frame.source);
            }

            self.mono_buffer.extend_from_slice(&mono);

            while self.mono_buffer.len() >= resampler.input_frames_next() {
                let required = resampler.input_frames_next();
                let chunk: Vec<f32> = self.mono_buffer.drain(..required).collect();
                let captured_at = self.tail_captured_at.unwrap_or(frame.captured_at);
                let source = self.tail_source.unwrap_or(frame.source);

                let out = Self::resample_chunk(resampler, &chunk, None)?;
                output_frames.push(Self::build_frame_with_processing(
                    self.processing,
                    captured_at,
                    source,
                    &out,
                ));

                if self.mono_buffer.is_empty() {
                    self.tail_captured_at = None;
                    self.tail_source = None;
                } else {
                    self.tail_captured_at = Some(frame.captured_at);
                    self.tail_source = Some(frame.source);
                }
            }
        } else {
            output_frames.push(Self::build_frame_with_processing(self.processing,frame.captured_at, frame.source, &mono));
        }

        Ok(output_frames)
    }

    pub fn flush_tail(&mut self) -> AppResult<Option<ResampledAudioFrame>> {
        if self.mono_buffer.is_empty() {
            self.tail_captured_at = None;
            self.tail_source = None;
            return Ok(None);
        }

        let Some(captured_at) = self.tail_captured_at.take() else {
            self.mono_buffer.clear();
            self.tail_source = None;
            return Ok(None);
        };
        let Some(source) = self.tail_source.take() else {
            self.mono_buffer.clear();
            return Ok(None);
        };
        let remaining = std::mem::take(&mut self.mono_buffer);

        if let Some(resampler) = &mut self.rubato_mono {
            let out = Self::resample_chunk(resampler, &remaining, Some(remaining.len()))?;
            return Ok(Some(Self::build_frame_with_processing(self.processing,captured_at, source, &out)));
        }

        Ok(Some(Self::build_frame_with_processing(
            self.processing,
            captured_at,
            source,
            &remaining,
        )))
    }

    fn build_frame_with_processing(
        processing: AudioProcessingConfig,
        captured_at: chrono::DateTime<chrono::Utc>,
        source: crate::types::AudioSource,
        samples: &[f32],
    ) -> ResampledAudioFrame {
        let duration_ms = ((samples.len() as u64) * 1000 / 16000) as u32;
        let pcm_bytes = Self::f32_to_pcm16_bytes(samples);
        let threshold = match source {
            AudioSource::Mic => processing.mic_silence_threshold,
            AudioSource::System => processing.system_silence_threshold,
            AudioSource::Mixed => processing
                .mic_silence_threshold
                .min(processing.system_silence_threshold),
        };
        let active = pcm_has_signal(&pcm_bytes, threshold);
        ResampledAudioFrame {
            captured_at,
            duration_ms,
            source,
            pcm_bytes,
            activity: SourceActivity::from_source(source, active),
        }
    }

    fn resample_chunk(
        resampler: &mut Async<f32>,
        chunk: &[f32],
        partial_len: Option<usize>,
    ) -> AppResult<Vec<f32>> {
        let input = InterleavedOwned::new_from(chunk.to_vec(), 1, chunk.len()).map_err(
            |e: rubato::audioadapter_buffers::SizeError| AppError::Resampler(e.to_string()),
        )?;
        let mut output = InterleavedOwned::new(0.0_f32, 1, resampler.output_frames_next());
        let indexing = Indexing {
            input_offset: 0,
            output_offset: 0,
            partial_len,
            active_channels_mask: None,
        };

        let (_nbr_in, nbr_out) = resampler
            .process_into_buffer(&input, &mut output, Some(&indexing))
            .map_err(|e| AppError::Resampler(e.to_string()))?;

        let data = output.take_data();
        Ok(data.into_iter().take(nbr_out).collect())
    }

    fn downmix_to_mono(&self, samples: &[f32]) -> Vec<f32> {
        if self.channels <= 1 {
            return samples.to_vec();
        }
        let ch = self.channels as usize;
        samples
            .chunks_exact(ch)
            .map(|frame| frame.iter().sum::<f32>() / ch as f32)
            .collect()
    }

    fn f32_to_pcm16_bytes(samples: &[f32]) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(samples.len() * 2);
        for &s in samples {
            let clamped = s.clamp(-1.0, 1.0);
            let pcm16 = (clamped * 32767.0) as i16;
            bytes.extend_from_slice(&pcm16.to_le_bytes());
        }
        bytes
    }
}

/// Spawns a dedicated OS Thread to process the CPU-heavy re-sampling computations.
/// Pushes strict 16kHz PCM16 frames with metadata across the bridge for SessionManager-controlled sending.
pub fn spawn_resampler_pipeline(
    runtime: CaptureRuntime,
    processing: AudioProcessingConfig,
    audio_rx: crossbeam_channel::Receiver<CapturedAudioFrame>,
    ws_tx: mpsc::Sender<ResampledAudioFrame>,
    event_tx: mpsc::Sender<WsEvent>,
    cancel_token: CancellationToken,
) -> AppResult<()> {
    let mut resamplers: HashMap<AudioSource, AudioResampler> = HashMap::new();
    for format in &runtime.formats {
        let source = match format.source {
            PhysicalAudioSource::Microphone => AudioSource::Mic,
            PhysicalAudioSource::SystemOutput => AudioSource::System,
        };
        resamplers.insert(
            source,
            AudioResampler::new(format.sample_rate, 16000, format.channels, processing)?,
        );
    }

    std::thread::spawn(move || {
        tracing::debug!(mixed = runtime.mixed, sources = runtime.formats.len(), "Resampler OS thread booted");
        let mut pending_mic: VecDeque<NormalizedAudioFrame> = VecDeque::new();
        let mut pending_system: VecDeque<NormalizedAudioFrame> = VecDeque::new();

        loop {
            if cancel_token.is_cancelled() {
                flush_all_resamplers(
                    &mut resamplers,
                    runtime.mixed,
                    &mut pending_mic,
                    &mut pending_system,
                    &ws_tx,
                    &event_tx,
                    &cancel_token,
                );
                break;
            }

            match audio_rx.recv() {
                Ok(samples) => {
                    let Some(resampler) = resamplers.get_mut(&samples.source) else {
                        let message = format!("Missing resampler for source {:?}", samples.source);
                        tracing::error!("{message}");
                        if !cancel_token.is_cancelled() {
                            let _ = event_tx.blocking_send(WsEvent::Error(message));
                        }
                        continue;
                    };

                    match resampler.process(samples) {
                        Ok(frames) => {
                            for frame in frames {
                                if emit_frame(
                                    frame,
                                    runtime.mixed,
                                    &mut pending_mic,
                                    &mut pending_system,
                                    &ws_tx,
                                )
                                .is_err()
                                {
                                    return;
                                }
                            }
                        }
                        Err(e) => {
                            let message = format!("Resampler failure dropping chunk: {e}");
                            tracing::error!("{message}");
                            if !cancel_token.is_cancelled() {
                                let _ = event_tx.blocking_send(WsEvent::Error(message));
                            }
                        }
                    }
                }
                Err(_) => {
                    flush_all_resamplers(
                        &mut resamplers,
                        runtime.mixed,
                        &mut pending_mic,
                        &mut pending_system,
                        &ws_tx,
                        &event_tx,
                        &cancel_token,
                    );
                    break;
                }
            }
        }
    });

    Ok(())
}

fn emit_frame(
    frame: ResampledAudioFrame,
    mixed: bool,
    pending_mic: &mut VecDeque<NormalizedAudioFrame>,
    pending_system: &mut VecDeque<NormalizedAudioFrame>,
    ws_tx: &mpsc::Sender<ResampledAudioFrame>,
) -> Result<(), ()> {
    if !mixed {
        return ws_tx.blocking_send(frame).map_err(|_| ());
    }

    let normalized = normalized_from_resampled(frame);
    match normalized.source {
        AudioSource::Mic => pending_mic.push_back(normalized),
        AudioSource::System => pending_system.push_back(normalized),
        AudioSource::Mixed => {
            return ws_tx.blocking_send(resampled_from_normalized(normalized)).map_err(|_| ());
        }
    }

    while !pending_mic.is_empty() && !pending_system.is_empty() {
        let mic_frame = pending_mic.pop_front();
        let system_frame = pending_system.pop_front();
        let Some(combined) = combine_mixed_tick(mic_frame, system_frame) else {
            break;
        };
        ws_tx
            .blocking_send(resampled_from_normalized(combined))
            .map_err(|_| ())?;
    }

    Ok(())
}

fn flush_all_resamplers(
    resamplers: &mut HashMap<AudioSource, AudioResampler>,
    mixed: bool,
    pending_mic: &mut VecDeque<NormalizedAudioFrame>,
    pending_system: &mut VecDeque<NormalizedAudioFrame>,
    ws_tx: &mpsc::Sender<ResampledAudioFrame>,
    event_tx: &mpsc::Sender<WsEvent>,
    cancel_token: &CancellationToken,
) {
    for resampler in resamplers.values_mut() {
        match resampler.flush_tail() {
            Ok(Some(frame)) => {
                let _ = emit_frame(frame, mixed, pending_mic, pending_system, ws_tx);
            }
            Ok(None) => {}
            Err(e) => {
                let message = format!("Resampler tail flush failed: {e}");
                tracing::error!("{message}");
                if !cancel_token.is_cancelled() {
                    let _ = event_tx.blocking_send(WsEvent::Error(message));
                }
            }
        }
    }

    if mixed {
        while !pending_mic.is_empty() && !pending_system.is_empty() {
            let mic_frame = pending_mic.pop_front();
            let system_frame = pending_system.pop_front();
            let Some(combined) = combine_mixed_tick(mic_frame, system_frame) else {
                break;
            };
            let _ = ws_tx.blocking_send(resampled_from_normalized(combined));
        }

        while let Some(mic_frame) = pending_mic.pop_front() {
            let _ = ws_tx.blocking_send(resampled_from_normalized(mic_frame));
        }
        while let Some(system_frame) = pending_system.pop_front() {
            let _ = ws_tx.blocking_send(resampled_from_normalized(system_frame));
        }
    }
}

fn normalized_from_resampled(frame: ResampledAudioFrame) -> NormalizedAudioFrame {
    NormalizedAudioFrame {
        captured_at: frame.captured_at,
        duration_ms: frame.duration_ms,
        source: frame.source,
        pcm_bytes: frame.pcm_bytes,
        activity: frame.activity,
    }
}

fn resampled_from_normalized(frame: NormalizedAudioFrame) -> ResampledAudioFrame {
    ResampledAudioFrame {
        captured_at: frame.captured_at,
        duration_ms: frame.duration_ms,
        source: frame.source,
        pcm_bytes: frame.pcm_bytes,
        activity: frame.activity,
    }
}

fn pcm_has_signal(bytes: &[u8], threshold: i16) -> bool {
    for sample in bytes.chunks_exact(2) {
        let value = i16::from_le_bytes([sample[0], sample[1]]).abs();
        if value >= threshold {
            return true;
        }
    }
    false
}
