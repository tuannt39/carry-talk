use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use rand::RngExt;
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::settings::data_dir;

pub const SECRETS_FILE: &str = ".secrets";
pub const SONIOX_API_KEY_SECRET_ID: &str = "provider.soniox.api_key";
const LEGACY_RUNTIME_API_KEY_FILE: &str = ".runtime_api_key";
const SECRET_KEY_FILE: &str = ".secrets.key";
const ENCRYPTED_SECRET_PREFIX: &str = "enc:v1";
const SECRET_KEY_LEN: usize = 32;
const SECRET_NONCE_LEN: usize = 12;
const SECRETS_DOC_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SecretsDocument {
    #[serde(default = "default_secrets_doc_version")]
    version: u32,
    #[serde(default)]
    entries: BTreeMap<String, String>,
}

impl Default for SecretsDocument {
    fn default() -> Self {
        Self {
            version: SECRETS_DOC_VERSION,
            entries: BTreeMap::new(),
        }
    }
}

fn default_secrets_doc_version() -> u32 {
    SECRETS_DOC_VERSION
}

/// Runtime secret store.
///
/// Secrets are persisted locally in `carrytalk-data/.secrets` using best-effort local
/// encryption. New installs store a JSON document whose entries are encrypted
/// individually, for example `provider.soniox.api_key -> enc:v1:...`.
/// Legacy installs may still have plaintext data in the old portable storage files,
/// including `data/.secrets`, `data/.runtime_api_key`, or a single encrypted payload in `data/.secrets`;
/// reads migrate those values to the multi-entry format automatically.
pub struct SecretStore;

impl SecretStore {
    pub fn new() -> Self {
        Self
    }

    fn secrets_file_path() -> PathBuf {
        data_dir().join(SECRETS_FILE)
    }

    fn secret_key_file_path() -> PathBuf {
        data_dir().join(SECRET_KEY_FILE)
    }

    fn legacy_runtime_api_key_path() -> PathBuf {
        data_dir().join(LEGACY_RUNTIME_API_KEY_FILE)
    }

    fn ensure_parent_dir(path: &PathBuf) -> AppResult<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| AppError::Storage(format!("Cannot create secrets directory: {e}")))?;
        }
        Ok(())
    }

    fn normalize_secret_value(value: &str) -> Option<String> {
        let normalized = value.trim();
        if normalized.is_empty() {
            return None;
        }

        Some(normalized.to_string())
    }

    fn read_text_file(path: &PathBuf) -> AppResult<Option<String>> {
        match fs::read_to_string(path) {
            Ok(contents) => Ok(Some(contents)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(AppError::Storage(format!(
                "Cannot read file {}: {err}",
                path.display()
            ))),
        }
    }

    fn remove_file_if_exists(path: &PathBuf) -> AppResult<()> {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(AppError::Storage(format!(
                "Cannot remove file {}: {err}",
                path.display()
            ))),
        }
    }

    fn write_bytes_atomic(path: &PathBuf, bytes: &[u8]) -> AppResult<()> {
        Self::ensure_parent_dir(path)?;

        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| AppError::Storage("Invalid file path".into()))?;
        let tmp_path = path.with_file_name(format!("{file_name}.tmp"));

        let mut tmp_file = File::create(&tmp_path)
            .map_err(|e| AppError::Storage(format!("Cannot create temp file: {e}")))?;
        tmp_file
            .write_all(bytes)
            .map_err(|e| AppError::Storage(format!("Cannot write temp file: {e}")))?;
        tmp_file
            .sync_all()
            .map_err(|e| AppError::Storage(format!("Cannot sync temp file: {e}")))?;
        drop(tmp_file);

        fs::rename(&tmp_path, path)
            .map_err(|e| AppError::Storage(format!("Cannot replace file: {e}")))?;

        Ok(())
    }

    fn load_or_create_secret_key(&self) -> AppResult<[u8; SECRET_KEY_LEN]> {
        let path = Self::secret_key_file_path();

        if let Some(raw_key) = Self::read_text_file(&path)? {
            let decoded = STANDARD.decode(raw_key.trim()).map_err(|e| {
                AppError::Storage(format!("Cannot decode local secret key {}: {e}", path.display()))
            })?;

            let key: [u8; SECRET_KEY_LEN] = decoded.try_into().map_err(|_| {
                AppError::Storage(format!(
                    "Local secret key {} has invalid length",
                    path.display()
                ))
            })?;

            return Ok(key);
        }

        let mut key = [0u8; SECRET_KEY_LEN];
        let mut rng = rand::rng();
        rng.fill(&mut key);
        let encoded = STANDARD.encode(key);
        Self::write_bytes_atomic(&path, encoded.as_bytes())?;
        Ok(key)
    }

    fn encrypt_secret(&self, value: &str) -> AppResult<String> {
        let key = self.load_or_create_secret_key()?;
        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| AppError::Storage(format!("Cannot initialize secret cipher: {e}")))?;

        let mut nonce_bytes = [0u8; SECRET_NONCE_LEN];
        let mut rng = rand::rng();
        rng.fill(&mut nonce_bytes);
        let nonce = &Nonce::from(nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, value.as_bytes())
            .map_err(|e| AppError::Storage(format!("Cannot encrypt secret value: {e}")))?;

        Ok(format!(
            "{ENCRYPTED_SECRET_PREFIX}:{}:{}",
            STANDARD.encode(nonce_bytes),
            STANDARD.encode(ciphertext)
        ))
    }

    fn decrypt_secret(&self, value: &str) -> AppResult<String> {
        let payload = value.trim();
        let mut parts = payload.split(':');
        let prefix = parts.next();
        let version = parts.next();
        let nonce_b64 = parts.next();
        let ciphertext_b64 = parts.next();

        if prefix != Some("enc") || version != Some("v1") || parts.next().is_some() {
            return Err(AppError::Storage("Invalid encrypted secret payload".into()));
        }

        let nonce_bytes = STANDARD.decode(nonce_b64.ok_or_else(|| {
            AppError::Storage("Encrypted secret payload missing nonce".into())
        })?)
        .map_err(|e| AppError::Storage(format!("Cannot decode secret nonce: {e}")))?;
        let nonce_array: [u8; SECRET_NONCE_LEN] = nonce_bytes.try_into().map_err(|_| {
            AppError::Storage("Encrypted secret nonce has invalid length".into())
        })?;

        let ciphertext = STANDARD
            .decode(ciphertext_b64.ok_or_else(|| {
                AppError::Storage("Encrypted secret payload missing ciphertext".into())
            })?)
            .map_err(|e| AppError::Storage(format!("Cannot decode encrypted secret: {e}")))?;

        let key = self.load_or_create_secret_key()?;
        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| AppError::Storage(format!("Cannot initialize secret cipher: {e}")))?;
        let nonce = &Nonce::from(nonce_array);
        let plaintext = cipher
            .decrypt(nonce, ciphertext.as_ref())
            .map_err(|e| AppError::Storage(format!("Cannot decrypt secret: {e}")))?;
        let plaintext = String::from_utf8(plaintext)
            .map_err(|e| AppError::Storage(format!("Decrypted secret is not valid UTF-8: {e}")))?;

        Self::normalize_secret_value(&plaintext)
            .ok_or_else(|| AppError::Auth("Secret not configured".into()))
    }

    fn read_secret_file(path: &PathBuf) -> AppResult<Option<String>> {
        let Some(contents) = Self::read_text_file(path)? else {
            return Ok(None);
        };

        Ok(Self::normalize_secret_value(&contents))
    }

    fn read_secrets_document_if_present(&self) -> AppResult<Option<SecretsDocument>> {
        let path = Self::secrets_file_path();
        let Some(contents) = Self::read_text_file(&path)? else {
            return Ok(None);
        };

        let trimmed = contents.trim();
        if trimmed.is_empty() {
            return Ok(Some(SecretsDocument::default()));
        }

        match serde_json::from_str::<SecretsDocument>(trimmed) {
            Ok(document) => Ok(Some(document)),
            Err(_) => Ok(None),
        }
    }

    fn write_secrets_document(&self, document: &SecretsDocument) -> AppResult<()> {
        let path = Self::secrets_file_path();
        let bytes = serde_json::to_vec_pretty(document)
            .map_err(|e| AppError::Storage(format!("Cannot serialize secrets document: {e}")))?;
        Self::write_bytes_atomic(&path, &bytes)
    }

    fn load_or_default_document(&self) -> AppResult<SecretsDocument> {
        Ok(self.read_secrets_document_if_present()?.unwrap_or_default())
    }

    fn read_legacy_secret_from_secrets_file(&self) -> AppResult<Option<String>> {
        let path = Self::secrets_file_path();
        let Some(raw_secret) = Self::read_text_file(&path)? else {
            return Ok(None);
        };

        let trimmed = raw_secret.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }

        if serde_json::from_str::<SecretsDocument>(trimmed).is_ok() {
            return Ok(None);
        }

        if trimmed.starts_with(ENCRYPTED_SECRET_PREFIX) {
            return self.decrypt_secret(trimmed).map(Some);
        }

        Ok(Self::normalize_secret_value(trimmed))
    }

    fn read_legacy_runtime_api_key(&self) -> AppResult<Option<String>> {
        let legacy_path = Self::legacy_runtime_api_key_path();
        Self::read_secret_file(&legacy_path)
    }

    fn migrate_legacy_secret_to_entry(&self, secret_id: &str, value: &str) -> AppResult<String> {
        self.upsert_secret(secret_id, value)?;

        let legacy_path = Self::legacy_runtime_api_key_path();
        if legacy_path != Self::secrets_file_path() {
            let _ = Self::remove_file_if_exists(&legacy_path);
        }

        Ok(value.to_string())
    }

    pub fn has_secret(&self, secret_id: &str) -> AppResult<bool> {
        Ok(self.get_secret(secret_id).is_ok())
    }

    pub fn get_secret(&self, secret_id: &str) -> AppResult<String> {
        if let Some(document) = self.read_secrets_document_if_present()? {
            if let Some(encrypted) = document.entries.get(secret_id) {
                return self.decrypt_secret(encrypted);
            }
        }

        if let Some(legacy_secret) = self.read_legacy_secret_from_secrets_file()? {
            return self.migrate_legacy_secret_to_entry(secret_id, &legacy_secret);
        }

        if let Some(legacy_key) = self.read_legacy_runtime_api_key()? {
            return self.migrate_legacy_secret_to_entry(secret_id, &legacy_key);
        }

        Err(AppError::Auth("API key not configured".into()))
    }

    pub fn upsert_secret(&self, secret_id: &str, value: &str) -> AppResult<()> {
        let normalized = Self::normalize_secret_value(value)
            .ok_or_else(|| AppError::Auth("API key not configured".into()))?;
        let encrypted = self.encrypt_secret(&normalized)?;
        let mut document = self.load_or_default_document()?;
        document.version = SECRETS_DOC_VERSION;
        document.entries.insert(secret_id.to_string(), encrypted);
        self.write_secrets_document(&document)?;

        let legacy_path = Self::legacy_runtime_api_key_path();
        if legacy_path != Self::secrets_file_path() {
            let _ = Self::remove_file_if_exists(&legacy_path);
        }

        Ok(())
    }

    pub fn clear_secret(&self, secret_id: &str) -> AppResult<()> {
        let mut document = self.load_or_default_document()?;
        document.entries.remove(secret_id);

        let secrets_path = Self::secrets_file_path();
        if document.entries.is_empty() {
            Self::remove_file_if_exists(&secrets_path)?;
        } else {
            document.version = SECRETS_DOC_VERSION;
            self.write_secrets_document(&document)?;
        }

        if secret_id == SONIOX_API_KEY_SECRET_ID {
            let legacy_path = Self::legacy_runtime_api_key_path();
            if legacy_path != secrets_path {
                Self::remove_file_if_exists(&legacy_path)?;
            }
        }

        Ok(())
    }

    pub fn has_runtime_api_key(&self) -> AppResult<bool> {
        self.has_secret(SONIOX_API_KEY_SECRET_ID)
    }

    pub fn runtime_api_key(&self) -> AppResult<String> {
        self.get_secret(SONIOX_API_KEY_SECRET_ID)
    }

    pub fn upsert_api_key(&mut self, key: &str) -> AppResult<()> {
        self.upsert_secret(SONIOX_API_KEY_SECRET_ID, key)
    }

    pub fn clear_api_key(&mut self) -> AppResult<()> {
        self.clear_secret(SONIOX_API_KEY_SECRET_ID)
    }
}
