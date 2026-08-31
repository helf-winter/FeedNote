use std::sync::Mutex;

use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm, Nonce,
};
use argon2::{Algorithm, Argon2, Params, Version};
use chrono::Utc;
use rand::{rngs::OsRng, RngCore};
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

use crate::{
    db::Database,
    error::{AppError, AppResult},
    models::{
        EncryptedSecretRecord, SecretItem, SecretMetadataProposal, SecretPayload,
        UpdateSecretInput, VaultMeta, VaultStatus,
    },
};

const VERIFIER: &[u8] = b"feednote-vault-v1";
const VERIFIER_AAD: &[u8] = b"feednote-vault-verifier";
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;

pub struct Vault {
    key: Mutex<Option<Zeroizing<[u8; 32]>>>,
}

impl Vault {
    pub fn new() -> Self {
        Self {
            key: Mutex::new(None),
        }
    }

    pub fn status(&self, database: &Database) -> AppResult<VaultStatus> {
        Ok(VaultStatus {
            initialized: database.get_vault_meta()?.is_some(),
            unlocked: self.is_unlocked(),
            secret_count: database.count_secret_records()?,
        })
    }

    pub fn initialize(&self, database: &Database, password: &str) -> AppResult<()> {
        if database.get_vault_meta()?.is_some() {
            return Err(AppError::Vault("保险箱已经初始化".to_string()));
        }
        validate_master_password(password)?;
        let mut salt = vec![0_u8; SALT_LEN];
        OsRng.fill_bytes(&mut salt);
        let mut key = derive_key(password, &salt)?;
        let (verifier_nonce, verifier_ciphertext) = encrypt_bytes(&key, VERIFIER, VERIFIER_AAD)?;
        database.save_vault_meta(&VaultMeta {
            salt,
            verifier_nonce,
            verifier_ciphertext,
        })?;
        self.set_key(key);
        key.zeroize();
        Ok(())
    }

    pub fn unlock(&self, database: &Database, password: &str) -> AppResult<()> {
        let meta = database
            .get_vault_meta()?
            .ok_or_else(|| AppError::Vault("请先设置主密码".to_string()))?;
        let mut key = derive_key(password, &meta.salt)?;
        let verified = decrypt_bytes(
            &key,
            &meta.verifier_nonce,
            &meta.verifier_ciphertext,
            VERIFIER_AAD,
        )
        .is_ok_and(|value| value == VERIFIER);
        if !verified {
            key.zeroize();
            return Err(AppError::Vault("主密码错误".to_string()));
        }
        self.set_key(key);
        key.zeroize();
        Ok(())
    }

    pub fn lock(&self) {
        *self.key.lock().expect("vault key lock poisoned") = None;
    }

    pub fn stash(
        &self,
        database: &Database,
        secret_value: &str,
        source_title: &str,
    ) -> AppResult<SecretItem> {
        let secret_value = secret_value.trim();
        if secret_value.is_empty() || secret_value.chars().count() > 100_000 {
            return Err(AppError::Validation("秘密值为空或过长".to_string()));
        }
        let key = self.current_key()?;
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().timestamp_millis();
        let payload = SecretPayload {
            title: default_title(source_title),
            secret_type: detect_secret_type(secret_value),
            account: None,
            secret_value: secret_value.to_string(),
            website: None,
            notes: None,
            source_title: source_title.trim().chars().take(300).collect(),
        };
        let record = encrypt_record(&key, &id, &payload, now, now, None)?;
        database.insert_secret_record(&record)?;
        Ok(SecretItem {
            id,
            payload,
            created_at: now,
            updated_at: now,
            feishu_synced_at: None,
        })
    }

    pub fn list(&self, database: &Database) -> AppResult<Vec<SecretItem>> {
        let key = self.current_key()?;
        database
            .list_encrypted_secret_records()?
            .iter()
            .map(|record| decrypt_record(&key, record))
            .collect()
    }

    pub fn apply_metadata(
        &self,
        database: &Database,
        id: &str,
        metadata: &SecretMetadataProposal,
    ) -> AppResult<SecretItem> {
        let key = self.current_key()?;
        let record = database.get_encrypted_secret_record(id)?;
        let mut item = decrypt_record(&key, &record)?;
        item.payload.title = normalized(metadata.title.as_str(), 120)
            .filter(|value| !value.is_empty())
            .unwrap_or(item.payload.title);
        item.payload.secret_type = normalized(metadata.secret_type.as_str(), 40)
            .filter(|value| !value.is_empty())
            .unwrap_or(item.payload.secret_type);
        item.payload.account = normalize_optional(metadata.account.as_deref(), 300);
        item.payload.website = normalize_http_url(metadata.website.as_deref());
        item.payload.notes = normalize_optional(metadata.notes.as_deref(), 1_000);
        item.updated_at = Utc::now().timestamp_millis();
        item.feishu_synced_at = None;
        let encrypted = encrypt_record(
            &key,
            &item.id,
            &item.payload,
            item.created_at,
            item.updated_at,
            None,
        )?;
        database.update_secret_record(&encrypted)?;
        Ok(item)
    }

    pub fn update(
        &self,
        database: &Database,
        id: &str,
        input: &UpdateSecretInput,
    ) -> AppResult<SecretItem> {
        let key = self.current_key()?;
        let record = database.get_encrypted_secret_record(id)?;
        let mut item = decrypt_record(&key, &record)?;
        let title = input.title.trim();
        let secret_type = input.secret_type.trim();
        if title.is_empty() || title.chars().count() > 120 {
            return Err(AppError::Validation(
                "秘密名称不能为空且不能超过 120 个字符".to_string(),
            ));
        }
        if secret_type.is_empty() || secret_type.chars().count() > 40 {
            return Err(AppError::Validation(
                "秘密类型不能为空且不能超过 40 个字符".to_string(),
            ));
        }
        if input.secret_value.trim().is_empty() || input.secret_value.chars().count() > 100_000 {
            return Err(AppError::Validation("秘密值为空或过长".to_string()));
        }
        item.payload.title = title.to_string();
        item.payload.secret_type = secret_type.to_string();
        item.payload.account = normalize_optional(input.account.as_deref(), 300);
        item.payload.secret_value = input.secret_value.clone();
        item.payload.website = validate_optional_http_url(input.website.as_deref())?;
        item.payload.notes = normalize_optional(input.notes.as_deref(), 1_000);
        item.updated_at = Utc::now().timestamp_millis();
        item.feishu_synced_at = None;
        let encrypted = encrypt_record(
            &key,
            &item.id,
            &item.payload,
            item.created_at,
            item.updated_at,
            None,
        )?;
        database.update_secret_record(&encrypted)?;
        Ok(item)
    }

    pub fn delete_with_password(
        &self,
        database: &Database,
        id: &str,
        password: &str,
    ) -> AppResult<()> {
        self.current_key()?;
        self.verify_password(database, password)?;
        database.delete_secret_record(id)
    }

    pub fn delete_recent(&self, database: &Database, id: &str, now: i64) -> AppResult<()> {
        self.current_key()?;
        let record = database.get_encrypted_secret_record(id)?;
        if now - record.created_at > 10_000 {
            return Err(AppError::Vault("撤销时间已过".to_string()));
        }
        database.delete_secret_record(id)
    }

    fn is_unlocked(&self) -> bool {
        self.key.lock().expect("vault key lock poisoned").is_some()
    }

    fn current_key(&self) -> AppResult<Zeroizing<[u8; 32]>> {
        self.key
            .lock()
            .expect("vault key lock poisoned")
            .as_ref()
            .map(|key| Zeroizing::new(**key))
            .ok_or_else(|| AppError::Vault("保险箱已锁定".to_string()))
    }

    fn set_key(&self, key: [u8; 32]) {
        *self.key.lock().expect("vault key lock poisoned") = Some(Zeroizing::new(key));
    }

    fn verify_password(&self, database: &Database, password: &str) -> AppResult<()> {
        validate_master_password(password)?;
        let meta = database
            .get_vault_meta()?
            .ok_or_else(|| AppError::Vault("请先设置主密码".to_string()))?;
        let mut key = derive_key(password, &meta.salt)?;
        let verified = decrypt_bytes(
            &key,
            &meta.verifier_nonce,
            &meta.verifier_ciphertext,
            VERIFIER_AAD,
        )
        .is_ok_and(|value| value == VERIFIER);
        key.zeroize();
        if verified {
            Ok(())
        } else {
            Err(AppError::Vault("主密码错误，未删除秘密".to_string()))
        }
    }
}

pub fn redact_context(secret: &str, surrounding: &str) -> String {
    let secret = secret.trim();
    if secret.is_empty() || !surrounding.contains(secret) {
        return "[周边文本未发送：无法确认脱敏位置]".to_string();
    }
    redact_text(secret, surrounding)
        .chars()
        .take(4_000)
        .collect()
}

pub fn redact_text(secret: &str, text: &str) -> String {
    let secret = secret.trim();
    if secret.is_empty() {
        return String::new();
    }
    text.replace(secret, "[SECRET]")
}

fn derive_key(password: &str, salt: &[u8]) -> AppResult<[u8; 32]> {
    let params = Params::new(19_456, 2, 1, Some(32))
        .map_err(|_| AppError::Vault("主密码派生参数无效".to_string()))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = [0_u8; 32];
    argon2
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|_| AppError::Vault("无法派生保险箱密钥".to_string()))?;
    Ok(key)
}

fn validate_master_password(password: &str) -> AppResult<()> {
    if password.chars().count() < 6 || password.chars().count() > 256 {
        return Err(AppError::Validation(
            "主密码长度必须为 6 到 256 个字符".to_string(),
        ));
    }
    Ok(())
}

fn encrypt_record(
    key: &[u8; 32],
    id: &str,
    payload: &SecretPayload,
    created_at: i64,
    updated_at: i64,
    feishu_synced_at: Option<i64>,
) -> AppResult<EncryptedSecretRecord> {
    let plaintext = Zeroizing::new(serde_json::to_vec(payload)?);
    let (nonce, ciphertext) = encrypt_bytes(key, plaintext.as_slice(), id.as_bytes())?;
    Ok(EncryptedSecretRecord {
        id: id.to_string(),
        nonce,
        ciphertext,
        created_at,
        updated_at,
        feishu_synced_at,
    })
}

fn decrypt_record(key: &[u8; 32], record: &EncryptedSecretRecord) -> AppResult<SecretItem> {
    let plaintext = Zeroizing::new(decrypt_bytes(
        key,
        &record.nonce,
        &record.ciphertext,
        record.id.as_bytes(),
    )?);
    let payload: SecretPayload = serde_json::from_slice(plaintext.as_slice())
        .map_err(|_| AppError::Vault("秘密记录已损坏".to_string()))?;
    Ok(SecretItem {
        id: record.id.clone(),
        payload,
        created_at: record.created_at,
        updated_at: record.updated_at,
        feishu_synced_at: record.feishu_synced_at,
    })
}

fn encrypt_bytes(key: &[u8; 32], plaintext: &[u8], aad: &[u8]) -> AppResult<(Vec<u8>, Vec<u8>)> {
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|_| AppError::Vault("保险箱密钥无效".to_string()))?;
    let mut nonce = vec![0_u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce);
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| AppError::Vault("秘密加密失败".to_string()))?;
    Ok((nonce, ciphertext))
}

fn decrypt_bytes(
    key: &[u8; 32],
    nonce: &[u8],
    ciphertext: &[u8],
    aad: &[u8],
) -> AppResult<Vec<u8>> {
    if nonce.len() != NONCE_LEN {
        return Err(AppError::Vault("秘密记录已损坏".to_string()));
    }
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|_| AppError::Vault("保险箱密钥无效".to_string()))?;
    cipher
        .decrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|_| AppError::Vault("主密码错误或秘密记录已损坏".to_string()))
}

fn detect_secret_type(value: &str) -> String {
    let value = value.trim();
    if value.contains("BEGIN") && value.contains("PRIVATE KEY") {
        "私钥".to_string()
    } else if value.starts_with("sk-")
        || value.starts_with("ghp_")
        || value.starts_with("github_pat_")
        || value.starts_with("AKIA")
        || (value.len() >= 24 && !value.chars().any(char::is_whitespace))
    {
        "API Key".to_string()
    } else {
        "密码".to_string()
    }
}

fn default_title(source_title: &str) -> String {
    let title: String = source_title.trim().chars().take(80).collect();
    if title.is_empty() {
        "未命名秘密".to_string()
    } else {
        title
    }
}

fn normalized(value: &str, max: usize) -> Option<String> {
    Some(value.trim().chars().take(max).collect())
}

fn normalize_optional(value: Option<&str>, max: usize) -> Option<String> {
    value
        .and_then(|value| normalized(value, max))
        .filter(|value| !value.is_empty())
}

fn normalize_http_url(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    let url = reqwest::Url::parse(value).ok()?;
    matches!(url.scheme(), "http" | "https").then(|| url.to_string())
}

fn validate_optional_http_url(value: Option<&str>) -> AppResult<Option<String>> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let url = reqwest::Url::parse(value)
        .map_err(|_| AppError::Validation("网站必须是有效的 http 或 https 地址".to_string()))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(AppError::Validation(
            "网站只允许 http 或 https 地址".to_string(),
        ));
    }
    Ok(Some(url.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn vault_encrypts_unlocks_and_never_stores_plaintext() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vault.db");
        let database = Database::open(&path).unwrap();
        let vault = Vault::new();
        vault
            .initialize(&database, "correct horse battery staple")
            .unwrap();
        vault
            .stash(&database, "sk-super-secret-value", "API 控制台")
            .unwrap();
        assert_eq!(
            vault.list(&database).unwrap()[0].payload.secret_value,
            "sk-super-secret-value"
        );
        for entry in std::fs::read_dir(dir.path()).unwrap() {
            let bytes = std::fs::read(entry.unwrap().path()).unwrap();
            assert!(!bytes
                .windows(21)
                .any(|window| window == b"sk-super-secret-value"));
        }

        vault.lock();
        assert!(vault.list(&database).is_err());
        assert!(vault.unlock(&database, "wrong password value").is_err());
        vault
            .unlock(&database, "correct horse battery staple")
            .unwrap();
        assert_eq!(vault.list(&database).unwrap().len(), 1);
    }

    #[test]
    fn redaction_removes_every_copy_of_the_selected_secret() {
        let redacted = redact_context("secret-123", "账号 secret-123，确认 secret-123");
        assert_eq!(redacted, "账号 [SECRET]，确认 [SECRET]");
    }

    #[test]
    fn redaction_withholds_context_when_the_secret_location_is_unknown() {
        let redacted = redact_context("secret-123", "页面只返回了不完整的可访问文本");
        assert_eq!(redacted, "[周边文本未发送：无法确认脱敏位置]");
    }

    #[test]
    fn master_password_requires_at_least_six_characters() {
        assert!(validate_master_password("12345").is_err());
        assert!(validate_master_password("123456").is_ok());
    }

    #[test]
    fn editing_reencrypts_fields_and_preserves_the_source() {
        let database = Database::in_memory().unwrap();
        let vault = Vault::new();
        vault.initialize(&database, "123456").unwrap();
        let item = vault.stash(&database, "old-secret", "原始页面").unwrap();

        let updated = vault
            .update(
                &database,
                &item.id,
                &UpdateSecretInput {
                    title: "新名称".to_string(),
                    secret_type: "令牌".to_string(),
                    account: Some("demo@example.com".to_string()),
                    secret_value: "new-secret".to_string(),
                    website: Some("https://example.com/login".to_string()),
                    notes: Some("手动更新".to_string()),
                },
            )
            .unwrap();

        assert_eq!(updated.payload.title, "新名称");
        assert_eq!(updated.payload.secret_value, "new-secret");
        assert_eq!(updated.payload.source_title, "原始页面");
        assert_eq!(
            updated.payload.website.as_deref(),
            Some("https://example.com/login")
        );
        assert!(updated.feishu_synced_at.is_none());
    }

    #[test]
    fn permanent_delete_requires_the_master_password_again() {
        let database = Database::in_memory().unwrap();
        let vault = Vault::new();
        vault.initialize(&database, "123456").unwrap();
        let item = vault.stash(&database, "keep-me", "测试").unwrap();

        assert!(vault
            .delete_with_password(&database, &item.id, "wrong-password")
            .is_err());
        assert_eq!(database.count_secret_records().unwrap(), 1);

        vault
            .delete_with_password(&database, &item.id, "123456")
            .unwrap();
        assert_eq!(database.count_secret_records().unwrap(), 0);
    }
}
