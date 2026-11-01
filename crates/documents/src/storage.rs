use async_trait::async_trait;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tokio::io::AsyncWriteExt;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("storage I/O error")]
    Io(#[from] std::io::Error),
    #[error("invalid storage key")]
    InvalidKey,
    #[error("object not found")]
    NotFound,
    #[error("storage backend unavailable: {0}")]
    Unavailable(String),
}

/// Object storage abstraction. The local implementation is used for
/// development and single-node deployments. The same interface is the
/// integration point for an S3-compatible backend.
#[async_trait]
pub trait ObjectStorage: Send + Sync {
    async fn put_file(&self, key: &str, source: &Path) -> Result<(), StorageError>;
    async fn get_bytes(&self, key: &str, max_bytes: usize) -> Result<Vec<u8>, StorageError>;
    async fn delete(&self, key: &str) -> Result<(), StorageError>;
    async fn exists(&self, key: &str) -> bool;
    async fn health(&self) -> bool;
    fn backend_name(&self) -> &'static str;
}

#[derive(Debug, Clone)]
pub struct LocalStorage {
    root: PathBuf,
    max_bytes: u64,
}

impl LocalStorage {
    pub fn new(root: impl Into<PathBuf>, max_bytes: u64) -> Result<Self, StorageError> {
        let root = root.into();
        std::fs::create_dir_all(&root)?;
        let root = root.canonicalize()?;
        Ok(Self { root, max_bytes })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn safe_path(&self, key: &str) -> Result<PathBuf, StorageError> {
        if key.is_empty()
            || key.len() > 400
            || key.starts_with('/')
            || key.contains('\\')
            || key.contains("..")
            || key.contains("//")
            || !key
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.'))
        {
            return Err(StorageError::InvalidKey);
        }
        let path = self.root.join(key);
        if !path.starts_with(&self.root) {
            return Err(StorageError::InvalidKey);
        }
        Ok(path)
    }
}

#[async_trait]
impl ObjectStorage for LocalStorage {
    async fn put_file(&self, key: &str, source: &Path) -> Result<(), StorageError> {
        let destination = self.safe_path(key)?;
        if let Some(parent) = destination.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let size = tokio::fs::metadata(source).await?.len();
        if size > self.max_bytes {
            return Err(StorageError::Unavailable(
                "file exceeds storage limit".into(),
            ));
        }
        // Copy rather than rename so callers can hash/validate the temp file
        // independently. Both paths are server-generated.
        tokio::fs::copy(source, &destination).await?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&destination, std::fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }

    async fn get_bytes(&self, key: &str, max_bytes: usize) -> Result<Vec<u8>, StorageError> {
        let path = self.safe_path(key)?;
        let meta = tokio::fs::metadata(&path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound
            } else {
                StorageError::Io(e)
            }
        })?;
        if meta.len() > max_bytes as u64 {
            return Err(StorageError::Unavailable(
                "object exceeds read limit".into(),
            ));
        }
        Ok(tokio::fs::read(path).await?)
    }

    async fn delete(&self, key: &str) -> Result<(), StorageError> {
        let path = self.safe_path(key)?;
        match tokio::fs::remove_file(path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(StorageError::Io(e)),
        }
    }

    async fn exists(&self, key: &str) -> bool {
        self.safe_path(key).map(|p| p.exists()).unwrap_or(false)
    }

    async fn health(&self) -> bool {
        let probe = self.root.join(".health");
        match tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&probe)
            .await
        {
            Ok(mut f) => {
                let _ = f.write_all(b"ok").await;
                let _ = tokio::fs::remove_file(probe).await;
                true
            }
            Err(_) => false,
        }
    }

    fn backend_name(&self) -> &'static str {
        "local-private"
    }
}

pub fn storage_key(
    organization_id: uuid::Uuid,
    document_id: uuid::Uuid,
    extension: &str,
) -> String {
    let random = uuid::Uuid::new_v4().simple().to_string();
    format!("organizations/{organization_id}/documents/{document_id}/{random}.{extension}")
}

pub fn storage_key_for_export(
    organization_id: uuid::Uuid,
    export_id: uuid::Uuid,
    extension: &str,
) -> String {
    format!("organizations/{organization_id}/exports/{export_id}/export.{extension}")
}
