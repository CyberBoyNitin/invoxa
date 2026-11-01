//! Document ingestion primitives: private storage, untrusted-file validation,
//! PDF text extraction and OCR adapters.

pub mod ocr;
pub mod storage;
pub mod text;
pub mod validation;

pub use storage::{LocalStorage, ObjectStorage, StorageError};
pub use text::{ExtractedPage, ExtractedText, TextExtractionConfig, TextExtractor, TextMethod};
pub use validation::{
    detect_kind, sanitize_filename, sha256_file, validate_upload, FileKind, Preflight,
    UploadLimits, ValidationError,
};
