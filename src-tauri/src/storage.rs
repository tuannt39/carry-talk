use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use chrono::Utc;
use tauri::{AppHandle, Emitter};

use crate::error::AppResult;
use crate::settings::data_dir;
use crate::types::{
    JsonlLine, SessionManifest, SessionPartMeta, SessionStatus, SessionSummary, TranscriptSegment,
};

/// Directory for session JSONL folders.
pub fn sessions_dir() -> PathBuf {
    data_dir().join("sessions")
}

/// Atomically overwrite the manifest.json file inside a session folder.
pub fn atomic_write_manifest(folder: &Path, manifest: &SessionManifest) -> AppResult<()> {
    let manifest_path = folder.join("manifest.json");
    let tmp_path = folder.join("manifest.json.tmp");

    let content = serde_json::to_string_pretty(manifest)?;
    fs::write(&tmp_path, content)?;
    fs::rename(&tmp_path, &manifest_path)?;
    Ok(())
}

/// Create a new session folder containing the initial manifest and part-0001.jsonl.
pub fn create_session_folder(
    provider: &str,
    source_language: &str,
    target_language: &str,
) -> AppResult<(String, PathBuf, SessionManifest)> {
    let base_dir = sessions_dir();
    fs::create_dir_all(&base_dir)?;

    let now = Utc::now();
    let short_id = &uuid::Uuid::new_v4().as_simple().to_string()[..6];
    let session_id = format!("{}_{}", now.format("%Y%m%d_%H%M%S"), short_id);
    let folder_path = base_dir.join(&session_id);

    fs::create_dir_all(&folder_path)?;

    let part_file = format!("part-0001.jsonl");
    let part_path = folder_path.join(&part_file);

    // Write part-0001 header
    let header_line = JsonlLine::Header {
        part_index: 1,
        session_id: session_id.clone(),
        created_at: now,
    };
    let mut file = File::create(&part_path)?;
    writeln!(file, "{}", serde_json::to_string(&header_line)?)?;
    file.flush()?;

    let manifest = SessionManifest {
        session_id: session_id.clone(),
        status: SessionStatus::Active,
        started_at: now,
        provider: provider.to_string(),
        source_language: source_language.to_string(),
        target_language: target_language.to_string(),
        parts: vec![SessionPartMeta {
            file: part_file,
            status: SessionStatus::Active,
            segments: 0,
        }],
    };

    atomic_write_manifest(&folder_path, &manifest)?;

    tracing::info!("Session folder created: {}", folder_path.display());
    Ok((session_id, folder_path, manifest))
}

/// Append finalized transcript segments to the specified part file.
pub fn append_segments(file_path: &Path, segments: &[TranscriptSegment]) -> AppResult<()> {
    if segments.is_empty() {
        return Ok(());
    }

    let mut file = OpenOptions::new().append(true).open(file_path)?;
    for segment in segments {
        let line = JsonlLine::Segment(segment.clone());
        writeln!(file, "{}", serde_json::to_string(&line)?)?;
    }
    file.flush()?;

    tracing::debug!(
        "Appended {} segments to {}",
        segments.len(),
        file_path.display()
    );
    Ok(())
}

/// Write a footer line to seal a part file.
pub fn finalize_part(file_path: &Path, segment_count: u32, status: SessionStatus) -> AppResult<()> {
    let footer_line = JsonlLine::Footer {
        status,
        segment_count,
        closed_at: Some(Utc::now()),
    };

    let mut file = OpenOptions::new().append(true).open(file_path)?;
    writeln!(file, "{}", serde_json::to_string(&footer_line)?)?;
    file.flush()?;
    Ok(())
}

/// List all saved sessions with summary info parsed natively from subfolder manifests.
pub fn list_sessions() -> AppResult<Vec<SessionSummary>> {
    let dir = sessions_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut summaries = Vec::new();

    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            let manifest_path = path.join("manifest.json");
            if manifest_path.exists() {
                if let Ok(content) = fs::read_to_string(&manifest_path) {
                    if let Ok(manifest) = serde_json::from_str::<SessionManifest>(&content) {
                        let is_complete = manifest.status != SessionStatus::Active;
                        let total_segments = manifest.parts.iter().map(|p| p.segments).sum();

                        summaries.push(SessionSummary {
                            session_id: manifest.session_id.clone(),
                            file_path: path.to_string_lossy().to_string(),
                            started_at: Some(manifest.started_at),
                            segment_count: total_segments,
                            is_complete,
                        });
                    }
                }
            }
        }
    }

    summaries.sort_by(|a, b| b.session_id.cmp(&a.session_id));
    Ok(summaries)
}

/// Check for interrupted sessions on app startup.
/// Scans manifests for "Active" states, seals the broken part with a Recovered footer, and repairs the manifest.
pub async fn check_interrupted_sessions(handle: &AppHandle) -> AppResult<()> {
    let dir = sessions_dir();
    if !dir.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            let manifest_path = path.join("manifest.json");
            if !manifest_path.exists() {
                continue;
            }

            if let Ok(content) = fs::read_to_string(&manifest_path) {
                if let Ok(mut manifest) = serde_json::from_str::<SessionManifest>(&content) {
                    if manifest.status == SessionStatus::Active {
                        // Locate active part
                        let active_idx = manifest
                            .parts
                            .iter()
                            .position(|p| p.status == SessionStatus::Active);

                        if let Some(idx) = active_idx {
                            let part_file = &manifest.parts[idx].file;
                            let part_path = path.join(part_file);

                            // Re-count segments strictly to ensure exact precision
                            let mut accurate_count = 0;
                            if let Ok(f) = File::open(&part_path) {
                                let reader = BufReader::new(f);
                                for line in reader.lines() {
                                    if let Ok(l) = line {
                                        if l.contains(r#""type":"segment""#)
                                            || l.contains(r#""type": "segment""#)
                                        {
                                            accurate_count += 1;
                                        }
                                    }
                                }
                            }

                            // Seal Part
                            let _ =
                                finalize_part(&part_path, accurate_count, SessionStatus::Recovered);

                            // Repair Manifest
                            manifest.parts[idx].status = SessionStatus::Recovered;
                            manifest.parts[idx].segments = accurate_count;
                        }

                        manifest.status = SessionStatus::Recovered;
                        if let Err(e) = atomic_write_manifest(&path, &manifest) {
                            tracing::error!(
                                "Failed to repair manifest for {}: {e}",
                                manifest.session_id
                            );
                        } else {
                            tracing::warn!(
                                "Recovered interrupted session: {}",
                                manifest.session_id
                            );

                            let total_segments = manifest.parts.iter().map(|p| p.segments).sum();
                            let summary = SessionSummary {
                                session_id: manifest.session_id.clone(),
                                file_path: path.to_string_lossy().to_string(),
                                started_at: Some(manifest.started_at),
                                segment_count: total_segments,
                                is_complete: true,
                            };

                            let _ = handle.emit("session_recovered", &summary);
                        }
                    }
                }
            }
        }
    }

    Ok(())
}
