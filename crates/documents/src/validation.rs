use std::io::Read;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Pdf,
    Png,
    Jpeg,
    Tiff,
}

impl FileKind {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Tiff => "tiff",
        }
    }

    pub fn content_type(self) -> &'static str {
        match self {
            Self::Pdf => "application/pdf",
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Tiff => "image/tiff",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Pdf => "PDF",
            Self::Png => "PNG image",
            Self::Jpeg => "JPEG image",
            Self::Tiff => "TIFF image",
        }
    }

    pub fn is_pdf(self) -> bool {
        matches!(self, Self::Pdf)
    }
}

#[derive(Debug, Clone)]
pub struct UploadLimits {
    pub max_file_bytes: u64,
    pub max_pages: u32,
    pub max_image_pixels: u64,
}

impl Default for UploadLimits {
    fn default() -> Self {
        Self {
            max_file_bytes: 20 * 1024 * 1024,
            max_pages: 50,
            max_image_pixels: 25_000_000,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Preflight {
    pub kind: FileKind,
    pub page_count: u32,
    pub content_type: &'static str,
    pub extension: &'static str,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub extension_mismatch: bool,
}

#[derive(Debug, Error)]
pub enum ValidationError {
    #[error("empty file")]
    Empty,
    #[error("file exceeds the configured size limit")]
    TooLarge,
    #[error("unsupported file format; accepted formats are PDF, PNG, JPEG and TIFF")]
    UnsupportedFormat,
    #[error("the PDF is encrypted and cannot be processed")]
    EncryptedPdf,
    #[error("the PDF is corrupted or cannot be parsed")]
    CorruptPdf,
    #[error("the image is corrupted or cannot be decoded")]
    CorruptImage,
    #[error("the document exceeds the maximum page count")]
    TooManyPages,
    #[error("the image dimensions exceed the processing limit")]
    ImageTooLarge,
    #[error("the image has invalid dimensions")]
    InvalidDimensions,
    #[error("file could not be read")]
    Io,
}

pub fn detect_kind(prefix: &[u8]) -> Option<FileKind> {
    if prefix.starts_with(b"%PDF-") {
        return Some(FileKind::Pdf);
    }
    if prefix.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some(FileKind::Png);
    }
    if prefix.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some(FileKind::Jpeg);
    }
    if prefix.starts_with(b"II\x2A\x00") || prefix.starts_with(b"MM\x00\x2A") {
        return Some(FileKind::Tiff);
    }
    None
}

pub fn sanitize_filename(name: &str) -> String {
    let base = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("document")
        .chars()
        .filter(|c| !c.is_control() && *c != '\0')
        .collect::<String>();
    let trimmed = base.trim().trim_matches('.');
    let cleaned = if trimmed.is_empty() {
        "document"
    } else {
        trimmed
    };
    let mut out: String = cleaned.chars().take(200).collect();
    if out.is_empty() {
        out = "document".into();
    }
    out
}

pub async fn sha256_file(path: &Path) -> Result<String, ValidationError> {
    use sha2::{Digest, Sha256};
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || -> Result<String, ValidationError> {
        let mut file = std::fs::File::open(&path).map_err(|_| ValidationError::Io)?;
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let read = file.read(&mut buffer).map_err(|_| ValidationError::Io)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        Ok(hex::encode(hasher.finalize()))
    })
    .await
    .map_err(|_| ValidationError::Io)?
}

/// Verify actual file content rather than trusting extension or MIME headers.
pub async fn validate_upload(
    path: &Path,
    original_filename: &str,
    limits: &UploadLimits,
) -> Result<Preflight, ValidationError> {
    let meta = tokio::fs::metadata(path)
        .await
        .map_err(|_| ValidationError::Io)?;
    if meta.len() == 0 {
        return Err(ValidationError::Empty);
    }
    if meta.len() > limits.max_file_bytes {
        return Err(ValidationError::TooLarge);
    }
    let mut file = std::fs::File::open(path).map_err(|_| ValidationError::Io)?;
    let mut prefix = [0u8; 16];
    let read = file.read(&mut prefix).map_err(|_| ValidationError::Io)?;
    let kind = detect_kind(&prefix[..read]).ok_or(ValidationError::UnsupportedFormat)?;
    drop(file);

    let extension_mismatch = std::path::Path::new(original_filename)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            let expected: &[&str] = match kind {
                FileKind::Pdf => &["pdf"],
                FileKind::Png => &["png"],
                FileKind::Jpeg => &["jpg", "jpeg"],
                FileKind::Tiff => &["tif", "tiff"],
            };
            !expected
                .iter()
                .any(|candidate| e.eq_ignore_ascii_case(candidate))
        })
        .unwrap_or(false);

    let path_owned = path.to_path_buf();
    let max_pages = limits.max_pages;
    let max_pixels = limits.max_image_pixels;
    let (page_count, width, height) = tokio::task::spawn_blocking(move || match kind {
        FileKind::Pdf => {
            let doc =
                lopdf::Document::load(&path_owned).map_err(|_| ValidationError::CorruptPdf)?;
            if doc.is_encrypted() {
                return Err(ValidationError::EncryptedPdf);
            }
            let pages = doc.get_pages().len() as u32;
            if pages == 0 {
                return Err(ValidationError::CorruptPdf);
            }
            if pages > max_pages {
                return Err(ValidationError::TooManyPages);
            }
            Ok((pages, None, None))
        }
        _ => {
            let reader =
                image::ImageReader::open(&path_owned).map_err(|_| ValidationError::CorruptImage)?;
            let (w, h) = reader
                .into_dimensions()
                .map_err(|_| ValidationError::CorruptImage)?;
            if w == 0 || h == 0 {
                return Err(ValidationError::InvalidDimensions);
            }
            if u64::from(w) * u64::from(h) > max_pixels {
                return Err(ValidationError::ImageTooLarge);
            }
            Ok((1, Some(w), Some(h)))
        }
    })
    .await
    .map_err(|_| ValidationError::Io)??;

    Ok(Preflight {
        kind,
        page_count,
        content_type: kind.content_type(),
        extension: kind.extension(),
        width,
        height,
        extension_mismatch,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_magic_bytes_only() {
        assert_eq!(detect_kind(b"%PDF-1.7\n"), Some(FileKind::Pdf));
        assert_eq!(
            detect_kind(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]),
            Some(FileKind::Png)
        );
        assert_eq!(detect_kind(&[0xFF, 0xD8, 0xFF, 0xE0]), Some(FileKind::Jpeg));
        assert_eq!(detect_kind(b"<html>"), None);
        assert_eq!(detect_kind(b"PK\x03\x04"), None);
    }

    #[test]
    fn sanitizes_path_traversal() {
        assert_eq!(sanitize_filename("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_filename("C:\\Windows\\evil.pdf"), "evil.pdf");
        assert_eq!(sanitize_filename(".."), "document");
        assert!(sanitize_filename(&"a".repeat(500)).len() <= 200);
    }
}
