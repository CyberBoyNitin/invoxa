//! Security primitives used by the API and worker:
//!
//! * Argon2id password hashing and verification
//! * high-entropy session, API, reset and CSRF tokens
//! * SHA-256 token storage hashes
//! * HMAC-SHA256 signing and constant-time verification
//! * AES-256-GCM encryption for integration secrets
//! * short-lived signed download grants
//!
//! Nothing in this crate logs secrets. Debug implementations of key-bearing
//! types are redacted.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::password_hash::rand_core::OsRng as PasswordOsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE_NO_PAD};
use base64::Engine;
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};
use subtle::ConstantTimeEq;
use thiserror::Error;
use uuid::Uuid;

const PASSWORD_MIN_CHARS: usize = 12;
const PASSWORD_MAX_CHARS: usize = 256;
const TOKEN_BYTES: usize = 32;
const AES_KEY_BYTES: usize = 32;
const AES_NONCE_BYTES: usize = 12;
const SECRET_AAD: &[u8] = b"invoxa:integration-secret:v1";
const DOWNLOAD_MAX_TTL_SECONDS: i64 = 7 * 24 * 60 * 60;

#[derive(Debug, Error)]
pub enum SecurityError {
    #[error("password must be between {min} and {max} characters")]
    PasswordLength { min: usize, max: usize },
    #[error("password could not be hashed")]
    PasswordHash,
    #[error("encryption key must be exactly 32 bytes")]
    InvalidEncryptionKey,
    #[error("encryption failed")]
    Encryption,
    #[error("decryption failed")]
    Decryption,
    #[error("token is invalid, tampered with, or expired")]
    InvalidToken,
    #[error("a random number generator failure occurred")]
    Random,
    #[error("background task failed")]
    TaskJoin,
}

// ---------------------------------------------------------------------------
// Passwords
// ---------------------------------------------------------------------------

pub fn validate_password(password: &str) -> Result<(), SecurityError> {
    let length = password.chars().count();
    if password.contains('\0') || !(PASSWORD_MIN_CHARS..=PASSWORD_MAX_CHARS).contains(&length) {
        return Err(SecurityError::PasswordLength {
            min: PASSWORD_MIN_CHARS,
            max: PASSWORD_MAX_CHARS,
        });
    }
    Ok(())
}

/// Hash a password with Argon2id using the crate's conservative defaults
/// (19 MiB memory, 2 iterations, 1 lane). This function is CPU-intensive;
/// API handlers should call [`hash_password_async`].
pub fn hash_password(password: &str) -> Result<String, SecurityError> {
    validate_password(password)?;
    let salt = SaltString::generate(&mut PasswordOsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| SecurityError::PasswordHash)
}

/// Verify a password without leaking whether the hash itself is malformed.
pub fn verify_password(password: &str, encoded_hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(encoded_hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

pub async fn hash_password_async(password: String) -> Result<String, SecurityError> {
    tokio::task::spawn_blocking(move || hash_password(&password))
        .await
        .map_err(|_| SecurityError::TaskJoin)?
}

pub async fn verify_password_async(password: String, encoded_hash: String) -> bool {
    tokio::task::spawn_blocking(move || verify_password(&password, &encoded_hash))
        .await
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Tokens and hashes
// ---------------------------------------------------------------------------

fn random_bytes<const N: usize>() -> Result<[u8; N], SecurityError> {
    let mut bytes = [0u8; N];
    OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| SecurityError::Random)?;
    Ok(bytes)
}

fn random_token() -> Result<String, SecurityError> {
    Ok(URL_SAFE_NO_PAD.encode(random_bytes::<TOKEN_BYTES>()?))
}

pub fn generate_session_token() -> Result<String, SecurityError> {
    random_token()
}

pub fn generate_reset_token() -> Result<String, SecurityError> {
    random_token()
}

pub fn generate_invitation_token() -> Result<String, SecurityError> {
    random_token()
}

pub fn generate_csrf_nonce() -> Result<String, SecurityError> {
    random_token()
}

#[derive(Debug, Clone)]
pub struct GeneratedApiKey {
    /// Shown exactly once. Only `hash` is persisted.
    pub secret: String,
    pub hash: String,
    /// Safe for display in key lists, for example `ivx_ab12cd34`.
    pub prefix: String,
    pub last_four: String,
}

pub fn generate_api_key() -> Result<GeneratedApiKey, SecurityError> {
    let secret = format!("ivx_{}", random_token()?);
    let hash = hash_token(&secret);
    let prefix = secret.chars().take(12).collect::<String>();
    let last_four = secret
        .chars()
        .rev()
        .take(4)
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    Ok(GeneratedApiKey {
        secret,
        hash,
        prefix,
        last_four,
    })
}

pub fn generate_webhook_secret() -> Result<String, SecurityError> {
    Ok(format!("whsec_{}", random_token()?))
}

/// SHA-256 hex digest. Used for session, API, invitation and reset tokens,
/// which are already high-entropy and therefore do not need a slow KDF.
pub fn sha256_hex(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    hex_encode(&digest)
}

pub fn hash_token(token: &str) -> String {
    sha256_hex(token)
}

pub fn token_matches(raw_token: &str, stored_hash: &str) -> bool {
    let computed = hash_token(raw_token);
    let stored = stored_hash.trim().to_ascii_lowercase();
    secure_equals(computed.as_bytes(), stored.as_bytes())
}

pub fn secure_equals(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.ct_eq(right).into()
}

// ---------------------------------------------------------------------------
// HMAC
// ---------------------------------------------------------------------------

pub fn hmac_sha256_hex(secret: &[u8], message: &[u8]) -> String {
    use hmac::{Hmac, Mac};
    let mut mac =
        <Hmac<Sha256> as Mac>::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(message);
    hex_encode(&mac.finalize().into_bytes())
}

pub fn verify_hmac_sha256_hex(secret: &[u8], message: &[u8], expected_hex: &str) -> bool {
    let expected = expected_hex.trim();
    if expected.len() != 64 || !expected.chars().all(|c| c.is_ascii_hexdigit()) {
        return false;
    }
    let computed = hmac_sha256_hex(secret, message);
    secure_equals(
        computed.as_bytes(),
        expected.to_ascii_lowercase().as_bytes(),
    )
}

/// Canonical webhook signature input. Keep this in sync with the public docs.
pub fn webhook_signature(secret: &[u8], timestamp: i64, body: &[u8]) -> String {
    let mut message = timestamp.to_string().into_bytes();
    message.push(b'.');
    message.extend_from_slice(body);
    hmac_sha256_hex(secret, &message)
}

fn hex_encode(bytes: impl AsRef<[u8]>) -> String {
    let bytes = bytes.as_ref();
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

// ---------------------------------------------------------------------------
// AES-256-GCM secret encryption
// ---------------------------------------------------------------------------

pub struct SecretCipher {
    key: [u8; AES_KEY_BYTES],
}

impl fmt::Debug for SecretCipher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecretCipher").finish_non_exhaustive()
    }
}

impl SecretCipher {
    pub fn from_base64_key(encoded: &str) -> Result<Self, SecurityError> {
        let bytes = decode_base64(encoded.trim()).ok_or(SecurityError::InvalidEncryptionKey)?;
        Self::from_bytes(&bytes)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, SecurityError> {
        if bytes.len() != AES_KEY_BYTES {
            return Err(SecurityError::InvalidEncryptionKey);
        }
        let mut key = [0u8; AES_KEY_BYTES];
        key.copy_from_slice(bytes);
        Ok(Self { key })
    }

    pub fn generate_key_base64() -> Result<String, SecurityError> {
        Ok(STANDARD.encode(random_bytes::<AES_KEY_BYTES>()?))
    }

    pub fn encrypt(&self, plaintext: &str) -> Result<String, SecurityError> {
        let cipher = Aes256Gcm::new_from_slice(&self.key)
            .map_err(|_| SecurityError::InvalidEncryptionKey)?;
        let nonce_bytes = random_bytes::<AES_NONCE_BYTES>()?;
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher
            .encrypt(
                nonce,
                aes_gcm::aead::Payload {
                    msg: plaintext.as_bytes(),
                    aad: SECRET_AAD,
                },
            )
            .map_err(|_| SecurityError::Encryption)?;
        Ok(format!(
            "v1:{}:{}",
            URL_SAFE_NO_PAD.encode(nonce_bytes),
            URL_SAFE_NO_PAD.encode(ciphertext)
        ))
    }

    pub fn decrypt(&self, envelope: &str) -> Result<String, SecurityError> {
        let mut parts = envelope.trim().split(':');
        let version = parts.next().ok_or(SecurityError::InvalidToken)?;
        let nonce_b64 = parts.next().ok_or(SecurityError::InvalidToken)?;
        let ciphertext_b64 = parts.next().ok_or(SecurityError::InvalidToken)?;
        if version != "v1" || parts.next().is_some() {
            return Err(SecurityError::InvalidToken);
        }
        let nonce_bytes = URL_SAFE_NO_PAD
            .decode(nonce_b64)
            .map_err(|_| SecurityError::InvalidToken)?;
        if nonce_bytes.len() != AES_NONCE_BYTES {
            return Err(SecurityError::InvalidToken);
        }
        let ciphertext = URL_SAFE_NO_PAD
            .decode(ciphertext_b64)
            .map_err(|_| SecurityError::InvalidToken)?;
        let cipher = Aes256Gcm::new_from_slice(&self.key)
            .map_err(|_| SecurityError::InvalidEncryptionKey)?;
        let plaintext = cipher
            .decrypt(
                Nonce::from_slice(&nonce_bytes),
                aes_gcm::aead::Payload {
                    msg: &ciphertext,
                    aad: SECRET_AAD,
                },
            )
            .map_err(|_| SecurityError::Decryption)?;
        String::from_utf8(plaintext).map_err(|_| SecurityError::Decryption)
    }
}

fn decode_base64(value: &str) -> Option<Vec<u8>> {
    STANDARD
        .decode(value)
        .or_else(|_| STANDARD_NO_PAD.decode(value))
        .or_else(|_| URL_SAFE_NO_PAD.decode(value))
        .ok()
}

// ---------------------------------------------------------------------------
// CSRF tokens
// ---------------------------------------------------------------------------

/// Stateless signed CSRF token. The browser receives it in a readable cookie
/// and must echo it in `X-CSRF-Token`; the API also checks `Origin`/`Referer`.
pub struct CsrfProtector {
    key: [u8; 32],
}

impl fmt::Debug for CsrfProtector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CsrfProtector").finish_non_exhaustive()
    }
}

impl CsrfProtector {
    pub fn new(secret: &[u8]) -> Result<Self, SecurityError> {
        if secret.len() < 32 {
            return Err(SecurityError::InvalidEncryptionKey);
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(&secret[..32]);
        Ok(Self { key })
    }

    pub fn issue(&self, ttl_seconds: i64) -> Result<String, SecurityError> {
        let expires = now_unix() + ttl_seconds.clamp(60, 24 * 60 * 60);
        let nonce = random_token()?;
        let payload = format!("v1.{expires}.{nonce}");
        let mac = hmac_sha256_hex(&self.key, payload.as_bytes());
        Ok(format!("{payload}.{mac}"))
    }

    pub fn verify(&self, token: &str) -> bool {
        let mut parts = token.trim().split('.');
        let (Some("v1"), Some(expires), Some(nonce), Some(mac), None) = (
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
        ) else {
            return false;
        };
        let Ok(expires) = expires.parse::<i64>() else {
            return false;
        };
        let now = now_unix();
        if expires < now || expires > now + 24 * 60 * 60 + 60 || nonce.len() < 20 {
            return false;
        }
        let payload = format!("v1.{expires}.{nonce}");
        verify_hmac_sha256_hex(&self.key, payload.as_bytes(), mac)
    }
}

// ---------------------------------------------------------------------------
// Signed download grants
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadGrant {
    pub organization_id: Uuid,
    pub document_id: Uuid,
    pub user_id: Uuid,
    /// Unix seconds.
    pub expires_at: i64,
}

pub fn sign_download_grant(secret: &[u8], grant: &DownloadGrant) -> Result<String, SecurityError> {
    if secret.len() < 32 {
        return Err(SecurityError::InvalidEncryptionKey);
    }
    let payload = serde_json::to_vec(grant).map_err(|_| SecurityError::InvalidToken)?;
    let payload = URL_SAFE_NO_PAD.encode(payload);
    let mac = hmac_sha256_hex(secret, format!("v1.{payload}").as_bytes());
    Ok(format!("v1.{payload}.{mac}"))
}

pub fn verify_download_grant(
    secret: &[u8],
    token: &str,
    now: i64,
) -> Result<DownloadGrant, SecurityError> {
    if secret.len() < 32 {
        return Err(SecurityError::InvalidEncryptionKey);
    }
    let mut parts = token.trim().split('.');
    let (Some("v1"), Some(payload), Some(mac), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(SecurityError::InvalidToken);
    };
    if !verify_hmac_sha256_hex(secret, format!("v1.{payload}").as_bytes(), mac) {
        return Err(SecurityError::InvalidToken);
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| SecurityError::InvalidToken)?;
    let grant: DownloadGrant =
        serde_json::from_slice(&bytes).map_err(|_| SecurityError::InvalidToken)?;
    if grant.expires_at <= now || grant.expires_at > now + DOWNLOAD_MAX_TTL_SECONDS {
        return Err(SecurityError::InvalidToken);
    }
    Ok(grant)
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_and_verifies_passwords() {
        let password = "correct horse battery staple";
        let hash = hash_password(password).unwrap();
        assert!(hash.starts_with("$argon2id$"));
        assert!(verify_password(password, &hash));
        assert!(!verify_password("wrong password", &hash));
        assert!(!verify_password(password, "not-a-hash"));
        assert_ne!(hash, hash_password(password).unwrap(), "salts must differ");
    }

    #[test]
    fn enforces_password_policy() {
        assert!(matches!(
            hash_password("short"),
            Err(SecurityError::PasswordLength { .. })
        ));
        let too_long = "a".repeat(257);
        assert!(hash_password(&too_long).is_err());
        assert!(hash_password("a\0aaaaaaaaaaaa").is_err());
    }

    #[test]
    fn generates_high_entropy_tokens_and_api_keys() {
        let a = generate_session_token().unwrap();
        let b = generate_session_token().unwrap();
        assert_ne!(a, b);
        assert!(a.len() >= 40);
        assert_eq!(hash_token(&a), sha256_hex(&a));
        assert!(token_matches(&a, &hash_token(&a)));
        assert!(!token_matches(&b, &hash_token(&a)));
        assert!(!token_matches("x", "nothex"));

        let key = generate_api_key().unwrap();
        assert!(key.secret.starts_with("ivx_"));
        assert_eq!(key.hash, hash_token(&key.secret));
        assert_eq!(key.prefix.len(), 12);
        assert_eq!(key.last_four.len(), 4);
        assert!(key.secret.ends_with(&key.last_four));
        assert!(!key.hash.contains(key.secret.as_str()));

        assert!(generate_webhook_secret().unwrap().starts_with("whsec_"));
    }

    #[test]
    fn verifies_hmac_and_rejects_tampering() {
        let secret = b"0123456789abcdef0123456789abcdef";
        let mac = hmac_sha256_hex(secret, b"hello");
        assert_eq!(mac.len(), 64);
        assert!(verify_hmac_sha256_hex(secret, b"hello", &mac));
        assert!(verify_hmac_sha256_hex(
            secret,
            b"hello",
            &mac.to_uppercase()
        ));
        assert!(!verify_hmac_sha256_hex(secret, b"hello!", &mac));
        assert!(!verify_hmac_sha256_hex(secret, b"hello", "zz"));

        let webhook = webhook_signature(secret, 1_700_000_000, br#"{"a":1}"#);
        assert_eq!(webhook, hmac_sha256_hex(secret, b"1700000000.{\"a\":1}"));
    }

    #[test]
    fn encrypts_secrets_with_authenticated_encryption() {
        let key = SecretCipher::generate_key_base64().unwrap();
        let cipher = SecretCipher::from_base64_key(&key).unwrap();
        let encrypted = cipher.encrypt("super-secret-token").unwrap();
        assert!(encrypted.starts_with("v1:"));
        assert!(!encrypted.contains("super-secret-token"));
        assert_eq!(cipher.decrypt(&encrypted).unwrap(), "super-secret-token");
        assert!(!format!("{cipher:?}").contains(&key));

        let mut tampered = encrypted.clone().into_bytes();
        let last = tampered.len() - 1;
        tampered[last] = if tampered[last] == b'A' { b'B' } else { b'A' };
        let tampered = String::from_utf8(tampered).unwrap();
        assert!(cipher.decrypt(&tampered).is_err());

        let other =
            SecretCipher::from_base64_key(&SecretCipher::generate_key_base64().unwrap()).unwrap();
        assert!(other.decrypt(&encrypted).is_err());

        assert!(SecretCipher::from_base64_key("PoorKey").is_err());
    }

    #[test]
    fn issues_and_verifies_csrf_tokens() {
        let protector = CsrfProtector::new(b"0123456789abcdef0123456789abcdef").unwrap();
        let token = protector.issue(600).unwrap();
        assert!(protector.verify(&token));
        let mut tampered = token.into_bytes();
        let last = tampered.len() - 1;
        tampered[last] = if tampered[last] == b'a' { b'b' } else { b'a' };
        assert!(!protector.verify(&String::from_utf8(tampered).unwrap()));
        assert!(!protector.verify("v1.1.nonce"));
        assert!(CsrfProtector::new(b"short").is_err());
    }

    #[test]
    fn signs_and_verifies_download_grants() {
        let secret = b"0123456789abcdef0123456789abcdef";
        let grant = DownloadGrant {
            organization_id: Uuid::new_v4(),
            document_id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            expires_at: now_unix() + 60,
        };
        let token = sign_download_grant(secret, &grant).unwrap();
        let verified = verify_download_grant(secret, &token, now_unix()).unwrap();
        assert_eq!(verified.document_id, grant.document_id);
        assert_eq!(verified.organization_id, grant.organization_id);
        assert!(verify_download_grant(secret, &token, now_unix() + 120).is_err());

        let expired = DownloadGrant {
            expires_at: now_unix() - 1,
            ..grant
        };
        let token = sign_download_grant(secret, &expired).unwrap();
        assert!(verify_download_grant(secret, &token, now_unix()).is_err());
        assert!(verify_download_grant(secret, "v1.abc.def", now_unix()).is_err());
    }

    #[test]
    fn constant_time_equality_rejects_length_mismatch() {
        assert!(secure_equals(b"abc", b"abc"));
        assert!(!secure_equals(b"abc", b"abd"));
        assert!(!secure_equals(b"abc", b"ab"));
    }
}
