use std::collections::BTreeMap;
use std::io::Write;
use thiserror::Error;
use tokio::process::Command;

#[derive(Debug, Error)]
pub enum OcrError {
    #[error("OCR engine is not available. Configure TESSERACT_PATH or install Tesseract OCR.")]
    Unavailable,
    #[error("OCR timed out")]
    Timeout,
    #[error("OCR failed: {0}")]
    Failed(String),
    #[error("OCR output could not be written")]
    Io,
}

#[derive(Debug, Clone)]
pub struct OcrConfig {
    pub tesseract_path: Option<String>,
    pub language: String,
    pub timeout_secs: u64,
    pub page_segmentation_mode: String,
}

impl Default for OcrConfig {
    fn default() -> Self {
        Self {
            tesseract_path: None,
            language: "eng".into(),
            timeout_secs: 60,
            page_segmentation_mode: "6".into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct OcrResult {
    pub text: String,
    pub mean_confidence: Option<f64>,
    pub engine: String,
    pub engine_version: String,
}

#[derive(Clone)]
pub struct TesseractOcr {
    config: OcrConfig,
    version: tokio::sync::OnceCell<String>,
}

impl TesseractOcr {
    pub fn new(config: OcrConfig) -> Self {
        Self {
            config,
            version: tokio::sync::OnceCell::new(),
        }
    }

    fn binary(&self) -> &str {
        self.config
            .tesseract_path
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("tesseract")
    }

    pub async fn version(&self) -> String {
        self.version
            .get_or_init(|| async {
                match Command::new(self.binary()).arg("--version").output().await {
                    Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
                        .lines()
                        .next()
                        .unwrap_or("tesseract")
                        .trim()
                        .to_string(),
                    _ => "unavailable".to_string(),
                }
            })
            .await
            .clone()
    }

    pub async fn available(&self) -> bool {
        matches!(
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                Command::new(self.binary()).arg("--version").output()
            )
            .await,
            Ok(Ok(out)) if out.status.success()
        )
    }

    pub async fn recognize_bytes(
        &self,
        image_bytes: &[u8],
        extension: &str,
    ) -> Result<OcrResult, OcrError> {
        let extension = match extension {
            "png" | "jpg" | "jpeg" | "tiff" | "tif" | "bmp" => extension,
            _ => "png",
        };
        let mut temp = tempfile::Builder::new()
            .prefix("invoxa-ocr-")
            .suffix(&format!(".{extension}"))
            .tempfile()
            .map_err(|_| OcrError::Io)?;
        temp.write_all(image_bytes).map_err(|_| OcrError::Io)?;
        temp.flush().map_err(|_| OcrError::Io)?;
        let path = temp.path().to_path_buf();

        let output = tokio::time::timeout(
            std::time::Duration::from_secs(self.config.timeout_secs.max(5)),
            Command::new(self.binary())
                .arg(&path)
                .arg("stdout")
                .arg("-l")
                .arg(&self.config.language)
                .arg("--psm")
                .arg(&self.config.page_segmentation_mode)
                .arg("tsv")
                .output(),
        )
        .await
        .map_err(|_| OcrError::Timeout)?
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                OcrError::Unavailable
            } else {
                OcrError::Failed("could not start OCR engine".into())
            }
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let safe = stderr
                .lines()
                .take(5)
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(300)
                .collect::<String>();
            return Err(OcrError::Failed(if safe.trim().is_empty() {
                "OCR engine returned an error".into()
            } else {
                safe
            }));
        }

        let tsv = String::from_utf8_lossy(&output.stdout);
        let (text, confidence) = parse_tsv(&tsv);
        if text.len() > 2_000_000 {
            return Err(OcrError::Failed("OCR output exceeds the size limit".into()));
        }
        Ok(OcrResult {
            text,
            mean_confidence: confidence,
            engine: "tesseract".into(),
            engine_version: self.version().await,
        })
    }
}

/// Parse Tesseract TSV output into line text plus an uncalibrated mean word
/// confidence. Tesseract confidence is stored as evidence, not treated as a
/// calibrated probability.
pub fn parse_tsv(tsv: &str) -> (String, Option<f64>) {
    let mut lines: BTreeMap<(u32, u32, u32, u32), Vec<String>> = BTreeMap::new();
    let mut confidences = Vec::new();
    for (index, row) in tsv.lines().enumerate() {
        if index == 0 {
            continue;
        }
        let columns: Vec<&str> = row.split('\t').collect();
        if columns.len() < 12 {
            continue;
        }
        let Ok(level) = columns[0].parse::<u32>() else {
            continue;
        };
        if level != 5 {
            continue;
        }
        let text = columns[11].trim();
        if text.is_empty() {
            continue;
        }
        let page = columns[1].parse::<u32>().unwrap_or(1);
        let block = columns[2].parse::<u32>().unwrap_or(0);
        let par = columns[3].parse::<u32>().unwrap_or(0);
        let line = columns[4].parse::<u32>().unwrap_or(0);
        if let Ok(conf) = columns[10].parse::<f64>() {
            if conf >= 0.0 {
                confidences.push(conf);
            }
        }
        lines
            .entry((page, block, par, line))
            .or_default()
            .push(text.to_string());
    }
    let text = lines
        .into_values()
        .map(|words| words.join(" "))
        .collect::<Vec<_>>()
        .join("\n");
    let confidence = if confidences.is_empty() {
        None
    } else {
        Some(confidences.iter().sum::<f64>() / confidences.len() as f64 / 100.0)
    };
    (text, confidence)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n\
5\t1\t1\t1\t1\t1\t10\t10\t30\t12\t96.5\tInvoice\n\
5\t1\t1\t1\t1\t2\t50\t10\t20\t12\t88.0\tNo:\n\
5\t1\t1\t1\t2\t1\t10\t30\t60\t12\t75.0\tINV-1001\n";

    #[test]
    fn reconstructs_lines_and_confidence() {
        let (text, confidence) = parse_tsv(SAMPLE);
        assert_eq!(text, "Invoice No:\nINV-1001");
        let c = confidence.unwrap();
        assert!((c - 0.865).abs() < 0.001, "{c}");
    }

    #[test]
    fn ignores_empty_words_and_headers() {
        let (text, confidence) =
            parse_tsv("level\tpage_num\n5\t1\t1\t1\t1\t1\t0\t0\t0\t0\t-1\t \n");
        assert_eq!(text, "");
        assert!(confidence.is_none());
    }
}
