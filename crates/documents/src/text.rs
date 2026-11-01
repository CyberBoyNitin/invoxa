use crate::ocr::{OcrConfig, OcrError, TesseractOcr};
use crate::validation::{FileKind, UploadLimits};
use image::{DynamicImage, ImageFormat, RgbImage};
use std::io::Cursor;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextMethod {
    PdfText,
    Ocr,
    Mixed,
}

impl TextMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PdfText => "pdf_text",
            Self::Ocr => "ocr",
            Self::Mixed => "mixed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExtractedPage {
    pub page_number: u32,
    pub text: String,
    pub method: TextMethod,
    pub confidence: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct ExtractedText {
    pub pages: Vec<ExtractedPage>,
    pub method: TextMethod,
    pub warnings: Vec<String>,
    pub quality: Option<f64>,
    pub ocr_pages: usize,
    pub ocr_engine_version: Option<String>,
}

impl ExtractedText {
    pub fn total_chars(&self) -> usize {
        self.pages.iter().map(|p| p.text.chars().count()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.total_chars() < 10
    }

    /// Text sent to an AI provider. Page markers let the provider report
    /// provenance without exposing hidden model reasoning.
    pub fn prompt_text(&self, max_chars: usize) -> String {
        let mut out = String::new();
        for page in &self.pages {
            if page.text.trim().is_empty() {
                continue;
            }
            let header = format!("\n\n[Page {}]\n", page.page_number);
            if out.len() + header.len() + page.text.len() > max_chars {
                let remaining = max_chars.saturating_sub(out.len() + header.len());
                if remaining > 0 {
                    out.push_str(&header);
                    out.extend(page.text.chars().take(remaining));
                }
                break;
            }
            out.push_str(&header);
            out.push_str(&page.text);
        }
        out.trim().to_string()
    }
}

#[derive(Debug, Error)]
pub enum TextError {
    #[error("document could not be read")]
    Io,
    #[error("PDF is encrypted and cannot be processed")]
    Encrypted,
    #[error("PDF is corrupted or cannot be parsed")]
    CorruptPdf,
    #[error("document exceeds the page limit")]
    TooManyPages,
    #[error("OCR is not available. Install Tesseract or configure TESSERACT_PATH.")]
    OcrUnavailable,
    #[error("OCR timed out")]
    OcrTimeout,
    #[error("OCR failed: {0}")]
    OcrFailed(String),
    #[error("no extractable text was found; the document may need OCR or a higher-quality scan")]
    NoText,
    #[error("page {0} contains an unsupported embedded image encoding")]
    UnsupportedImage(u32),
}

impl TextError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Io => "DOCUMENT_IO_ERROR",
            Self::Encrypted => "PDF_ENCRYPTED",
            Self::CorruptPdf => "PDF_CORRUPT",
            Self::TooManyPages => "PAGE_LIMIT_EXCEEDED",
            Self::OcrUnavailable => "OCR_UNAVAILABLE",
            Self::OcrTimeout => "OCR_TIMEOUT",
            Self::OcrFailed(_) => "OCR_FAILED",
            Self::NoText => "NO_EXTRACTABLE_TEXT",
            Self::UnsupportedImage(_) => "UNSUPPORTED_PDF_IMAGE",
        }
    }

    pub fn retryable(&self) -> bool {
        matches!(self, Self::Io | Self::OcrTimeout | Self::OcrFailed(_))
    }
}

#[derive(Debug, Clone)]
pub struct TextExtractionConfig {
    pub limits: UploadLimits,
    pub ocr: OcrConfig,
    pub ocr_enabled: bool,
    pub min_text_chars: usize,
    pub max_chars_per_page: usize,
    pub max_chars_total: usize,
    pub max_prompt_chars: usize,
}

impl Default for TextExtractionConfig {
    fn default() -> Self {
        Self {
            limits: UploadLimits::default(),
            ocr: OcrConfig::default(),
            ocr_enabled: true,
            min_text_chars: 40,
            max_chars_per_page: 200_000,
            max_chars_total: 1_500_000,
            max_prompt_chars: 60_000,
        }
    }
}

pub struct TextExtractor {
    config: TextExtractionConfig,
    ocr: TesseractOcr,
}

struct ImagePlan {
    page_number: u32,
    bytes: Vec<u8>,
    extension: &'static str,
}

struct PdfPlan {
    pages: Vec<Option<String>>,
    page_numbers: Vec<u32>,
    images: Vec<ImagePlan>,
    warnings: Vec<String>,
    has_unsupported_image: bool,
}

impl TextExtractor {
    pub fn new(config: TextExtractionConfig) -> Self {
        let ocr = TesseractOcr::new(config.ocr.clone());
        Self { config, ocr }
    }

    pub fn ocr(&self) -> &TesseractOcr {
        &self.ocr
    }

    pub async fn ocr_available(&self) -> bool {
        self.ocr.available().await
    }

    pub async fn extract(&self, path: &Path, kind: FileKind) -> Result<ExtractedText, TextError> {
        match kind {
            FileKind::Pdf => self.extract_pdf(path).await,
            _ => self.extract_image(path, kind).await,
        }
    }

    async fn extract_image(&self, path: &Path, kind: FileKind) -> Result<ExtractedText, TextError> {
        if !self.config.ocr_enabled {
            return Err(TextError::OcrUnavailable);
        }
        let bytes = tokio::fs::read(path).await.map_err(|_| TextError::Io)?;
        let result = self
            .ocr
            .recognize_bytes(&bytes, kind.extension())
            .await
            .map_err(map_ocr_error)?;
        let text = clean_text(&result.text, self.config.max_chars_per_page);
        if text.trim().is_empty() {
            return Err(TextError::NoText);
        }
        Ok(ExtractedText {
            pages: vec![ExtractedPage {
                page_number: 1,
                text,
                method: TextMethod::Ocr,
                confidence: result.mean_confidence,
            }],
            method: TextMethod::Ocr,
            warnings: Vec::new(),
            quality: result.mean_confidence,
            ocr_pages: 1,
            ocr_engine_version: Some(result.engine_version),
        })
    }

    async fn extract_pdf(&self, path: &Path) -> Result<ExtractedText, TextError> {
        let path = path.to_path_buf();
        let max_pages = self.config.limits.max_pages;
        let max_pixels = self.config.limits.max_image_pixels;
        let min_chars = self.config.min_text_chars;
        let max_page_chars = self.config.max_chars_per_page;

        let plan = tokio::task::spawn_blocking(move || -> Result<PdfPlan, TextError> {
            let doc = lopdf::Document::load(&path).map_err(|_| TextError::CorruptPdf)?;
            if doc.is_encrypted() {
                return Err(TextError::Encrypted);
            }
            let page_map = doc.get_pages();
            if page_map.len() > max_pages as usize {
                return Err(TextError::TooManyPages);
            }
            if page_map.is_empty() {
                return Err(TextError::CorruptPdf);
            }

            let text_pages = std::panic::catch_unwind(|| {
                pdf_extract::extract_text_by_pages(&path)
            })
            .ok()
            .and_then(|r| r.ok());

            let mut pages: Vec<Option<String>> = Vec::new();
            let mut page_numbers: Vec<u32> = Vec::new();
            let mut warnings = Vec::new();
            let mut images = Vec::new();
            let mut has_unsupported_image = false;

            for (index, (page_number, page_id)) in page_map.iter().enumerate() {
                page_numbers.push(*page_number);
                let mut page_text = text_pages
                    .as_ref()
                    .and_then(|all| all.get(index))
                    .cloned()
                    .unwrap_or_default();
                if page_text.trim().is_empty() {
                    // Fall back to lopdf's own text extractor for pages that
                    // the primary extractor could not read.
                    if let Ok(t) = doc.extract_text(&[*page_number]) {
                        page_text = t;
                    }
                }
                let cleaned = clean_text(&page_text, max_page_chars);
                let needs_ocr = cleaned.trim().chars().count() < min_chars;
                let mut pushed_image = false;
                if needs_ocr {
                    match largest_page_image(&doc, *page_id, max_pixels) {
                        Ok(Some((bytes, extension))) => {
                            images.push(ImagePlan {
                                page_number: *page_number,
                                bytes,
                                extension,
                            });
                            pushed_image = true;
                        }
                        Ok(None) => {}
                        Err(ImageError::TooLarge) => {
                            warnings.push(format!(
                                "Page {page_number}: embedded image exceeds the pixel limit and was skipped."
                            ));
                        }
                        Err(ImageError::Unsupported) => {
                            has_unsupported_image = true;
                            warnings.push(format!(
                                "Page {page_number}: embedded image encoding is not supported by the OCR adapter."
                            ));
                        }
                        Err(ImageError::Decode) => {
                            warnings.push(format!(
                                "Page {page_number}: embedded image could not be decoded."
                            ));
                        }
                    }
                }
                pages.push(if cleaned.trim().is_empty() { None } else { Some(cleaned) });
                if !pushed_image && needs_ocr {
                    warnings.push(format!(
                        "Page {page_number}: little or no embedded text; no OCR-able image was found."
                    ));
                }
            }
            Ok(PdfPlan { pages, page_numbers, images, warnings, has_unsupported_image })
        })
        .await
        .map_err(|_| TextError::CorruptPdf)??;

        let mut ocr_results: Vec<(u32, String, Option<f64>, String)> = Vec::new();
        let mut ocr_warnings = Vec::new();
        if !plan.images.is_empty() {
            if !self.config.ocr_enabled {
                return Err(TextError::OcrUnavailable);
            }
            for image in &plan.images {
                match self
                    .ocr
                    .recognize_bytes(&image.bytes, image.extension)
                    .await
                {
                    Ok(result) => {
                        let cleaned = clean_text(&result.text, self.config.max_chars_per_page);
                        if !cleaned.trim().is_empty() {
                            ocr_results.push((
                                image.page_number,
                                cleaned,
                                result.mean_confidence,
                                result.engine_version,
                            ));
                        }
                    }
                    Err(OcrError::Unavailable) => return Err(TextError::OcrUnavailable),
                    Err(OcrError::Timeout) => return Err(TextError::OcrTimeout),
                    Err(OcrError::Failed(message)) => ocr_warnings
                        .push(format!("Page {}: OCR failed: {message}", image.page_number)),
                    Err(OcrError::Io) => return Err(TextError::Io),
                }
            }
        }

        let mut warnings = plan.warnings;
        warnings.extend(ocr_warnings);
        if plan.has_unsupported_image && ocr_results.is_empty() {
            if let Some(plan) = plan.images.first() {
                return Err(TextError::UnsupportedImage(plan.page_number));
            }
        }

        let mut pages: Vec<ExtractedPage> = Vec::new();
        let mut pdf_text_pages = 0usize;
        let mut ocr_page_count = 0usize;
        let mut confidences = Vec::new();
        let mut engine_version: Option<String> = None;
        let mut total_chars = 0usize;
        for (index, _) in plan.pages.iter().enumerate() {
            let page_number = plan
                .page_numbers
                .get(index)
                .copied()
                .unwrap_or((index + 1) as u32);
            let ocr = ocr_results
                .iter()
                .find(|(number, _, _, _)| *number == page_number);
            let text = plan
                .pages
                .get(index)
                .cloned()
                .flatten()
                .filter(|t| !t.trim().is_empty());
            let (final_text, method, confidence) = if let Some((_, ocr_text, conf, version)) = ocr {
                engine_version = Some(version.clone());
                ocr_page_count += 1;
                if let Some(conf) = conf {
                    confidences.push(*conf);
                }
                if text.is_some() {
                    warnings.push(format!(
                        "Page {page_number}: OCR was used to supplement embedded text."
                    ));
                    (
                        format!("{}\n{}", text.clone().unwrap_or_default(), ocr_text),
                        TextMethod::Mixed,
                        *conf,
                    )
                } else {
                    (ocr_text.clone(), TextMethod::Ocr, *conf)
                }
            } else if let Some(text) = text {
                pdf_text_pages += 1;
                (text, TextMethod::PdfText, None)
            } else {
                continue;
            };
            let remaining = self.config.max_chars_total.saturating_sub(total_chars);
            let truncated: String = final_text.chars().take(remaining).collect();
            total_chars += truncated.chars().count();
            pages.push(ExtractedPage {
                page_number,
                text: truncated,
                method,
                confidence,
            });
            if total_chars >= self.config.max_chars_total {
                warnings.push("Extracted text limit reached; later pages were truncated.".into());
                break;
            }
        }

        if pages.is_empty() {
            return Err(TextError::NoText);
        }
        let method = if pdf_text_pages > 0 && ocr_page_count > 0 {
            TextMethod::Mixed
        } else if ocr_page_count > 0 {
            TextMethod::Ocr
        } else {
            TextMethod::PdfText
        };
        let quality = if confidences.is_empty() {
            None
        } else {
            Some(confidences.iter().sum::<f64>() / confidences.len() as f64)
        };
        Ok(ExtractedText {
            pages,
            method,
            warnings,
            quality,
            ocr_pages: ocr_page_count,
            ocr_engine_version: engine_version,
        })
    }
}

fn _unused_page_number_helper() {
    // Reserved for future page numbering adapters.
}

fn map_ocr_error(e: OcrError) -> TextError {
    match e {
        OcrError::Unavailable => TextError::OcrUnavailable,
        OcrError::Timeout => TextError::OcrTimeout,
        OcrError::Failed(m) => TextError::OcrFailed(m),
        OcrError::Io => TextError::Io,
    }
}

pub fn clean_text(input: &str, max_chars: usize) -> String {
    let mut out = String::with_capacity(input.len().min(max_chars));
    let mut last_was_space = false;
    for ch in input.chars().take(max_chars) {
        let c = match ch {
            '\u{00A0}' => ' ',
            '\t' => ' ',
            '\r' => '\n',
            c if c.is_control() && c != '\n' => continue,
            c => c,
        };
        if c == ' ' {
            if last_was_space {
                continue;
            }
            last_was_space = true;
        } else {
            last_was_space = false;
        }
        out.push(c);
    }
    out.trim().to_string()
}

enum ImageError {
    TooLarge,
    Unsupported,
    Decode,
}

fn largest_page_image(
    doc: &lopdf::Document,
    page_id: lopdf::ObjectId,
    max_pixels: u64,
) -> Result<Option<(Vec<u8>, &'static str)>, ImageError> {
    let images = doc
        .get_page_images(page_id)
        .map_err(|_| ImageError::Decode)?;
    let mut best: Option<(&lopdf::xobject::PdfImage<'_>, i64)> = None;
    for image in &images {
        let pixels = image.width.max(0).saturating_mul(image.height.max(0));
        if pixels < 40_000 {
            continue;
        }
        if pixels as u64 > max_pixels {
            return Err(ImageError::TooLarge);
        }
        if best
            .as_ref()
            .map(|(_, area)| pixels > *area)
            .unwrap_or(true)
        {
            best = Some((image, pixels));
        }
    }
    let Some((image, _)) = best else {
        return Ok(None);
    };
    decode_pdf_image(image).map(Some)
}

fn decode_pdf_image(
    image: &lopdf::xobject::PdfImage<'_>,
) -> Result<(Vec<u8>, &'static str), ImageError> {
    let width = u32::try_from(image.width).map_err(|_| ImageError::Decode)?;
    let height = u32::try_from(image.height).map_err(|_| ImageError::Decode)?;
    if width == 0 || height == 0 {
        return Err(ImageError::Decode);
    }
    let filters: Vec<String> = image
        .filters
        .clone()
        .unwrap_or_default()
        .into_iter()
        .map(|f| f.to_ascii_uppercase())
        .collect();
    let has = |name: &str| filters.iter().any(|f| f == name);

    if has("DCTDECODE") && filters.len() == 1 {
        let decoded = image::load_from_memory_with_format(image.content, ImageFormat::Jpeg)
            .map_err(|_| ImageError::Decode)?;
        if decoded.width() as u64 * decoded.height() as u64 > 25_000_000 {
            return Err(ImageError::TooLarge);
        }
        return Ok((image.content.to_vec(), "jpg"));
    }
    if has("JPXDECODE") {
        return Err(ImageError::Unsupported);
    }
    if has("CCITTFaxDecode") {
        return Err(ImageError::Unsupported);
    }
    let raw = if has("FLATEDECODE") && filters.len() == 1 {
        use std::io::Read;
        let mut decoder = flate2::read::ZlibDecoder::new(image.content);
        let mut out = Vec::new();
        decoder
            .read_to_end(&mut out)
            .map_err(|_| ImageError::Decode)?;
        out
    } else if filters.is_empty() {
        image.content.to_vec()
    } else {
        return Err(ImageError::Unsupported);
    };

    let bpc = image.bits_per_component.unwrap_or(8);
    if bpc != 8 {
        return Err(ImageError::Unsupported);
    }
    let color_space = image
        .color_space
        .as_deref()
        .unwrap_or("DeviceRGB")
        .to_ascii_uppercase();
    let expected_channels = match color_space.as_str() {
        "DEVICERGB" | "RGB" => 3usize,
        "DEVICEGRAY" | "G" | "CALGRAY" => 1,
        "DEVICECMYK" | "CMYK" => 4,
        _ => return Err(ImageError::Unsupported),
    };
    let expected = width as usize * height as usize * expected_channels;
    if raw.len() < expected {
        return Err(ImageError::Decode);
    }
    let encoded = match expected_channels {
        3 => {
            let rgb = RgbImage::from_raw(width, height, raw[..expected].to_vec())
                .ok_or(ImageError::Decode)?;
            encode_png(DynamicImage::ImageRgb8(rgb))?
        }
        1 => {
            let gray = image::GrayImage::from_raw(width, height, raw[..expected].to_vec())
                .ok_or(ImageError::Decode)?;
            encode_png(DynamicImage::ImageLuma8(gray))?
        }
        4 => {
            let mut rgb = Vec::with_capacity(width as usize * height as usize * 3);
            for pixel in raw[..expected].chunks_exact(4) {
                let c = pixel[0] as f32 / 255.0;
                let m = pixel[1] as f32 / 255.0;
                let y = pixel[2] as f32 / 255.0;
                let k = pixel[3] as f32 / 255.0;
                rgb.push((255.0 * (1.0 - c) * (1.0 - k)) as u8);
                rgb.push((255.0 * (1.0 - m) * (1.0 - k)) as u8);
                rgb.push((255.0 * (1.0 - y) * (1.0 - k)) as u8);
            }
            let img = RgbImage::from_raw(width, height, rgb).ok_or(ImageError::Decode)?;
            encode_png(DynamicImage::ImageRgb8(img))?
        }
        _ => return Err(ImageError::Unsupported),
    };
    Ok((encoded, "png"))
}

fn encode_png(image: DynamicImage) -> Result<Vec<u8>, ImageError> {
    let mut out = Cursor::new(Vec::new());
    image
        .write_to(&mut out, ImageFormat::Png)
        .map_err(|_| ImageError::Decode)?;
    Ok(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_markers_are_preserved_and_limited() {
        let text = ExtractedText {
            pages: vec![
                ExtractedPage {
                    page_number: 1,
                    text: "First page".into(),
                    method: TextMethod::PdfText,
                    confidence: None,
                },
                ExtractedPage {
                    page_number: 2,
                    text: "Second page".into(),
                    method: TextMethod::PdfText,
                    confidence: None,
                },
            ],
            method: TextMethod::PdfText,
            warnings: vec![],
            quality: None,
            ocr_pages: 0,
            ocr_engine_version: None,
        };
        let prompt = text.prompt_text(10_000);
        assert!(prompt.contains("[Page 1]"));
        assert!(prompt.contains("[Page 2]"));
        let truncated = text.prompt_text(12);
        assert!(truncated.len() <= 12);
    }

    #[test]
    fn cleans_control_characters() {
        assert_eq!(clean_text("A\u{0000}B\tC\r\nD", 100), "AB C\n\nD");
    }
}
