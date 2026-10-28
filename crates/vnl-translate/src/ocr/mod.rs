//! OCR: `OcrAdapter` (image → text) backends and the periodic OCR text source.
//!
//! Backends:
//! - `tesseract`: the Tesseract CLI with `jpn` / `jpn_vert` traineddata.
//! - `ollama:<model>`: a local vision-capable Ollama model (capability
//!   `vision` in `/api/tags`). Cloud models are never used for OCR.

pub mod ollama_vision;
pub mod source;
pub mod tesseract;

use async_trait::async_trait;
use vnl_core::AppError;

use crate::capture::CapturedImage;
use crate::types::BackendInfo;

#[async_trait]
pub trait OcrAdapter: Send + Sync {
    fn id(&self) -> String;
    /// `language`: BCP-47 source language hint (e.g. `ja`).
    async fn recognize(&self, image: &CapturedImage, language: Option<&str>) -> Result<String, AppError>;
}

/// Removes spaces OCR engines insert between CJK characters and trims lines.
pub fn clean_ocr_text(s: &str) -> String {
    fn cjk(c: char) -> bool {
        matches!(c as u32,
            0x3000..=0x303F | 0x3040..=0x30FF | 0x31F0..=0x31FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF |
            0xF900..=0xFAFF | 0xFF00..=0xFFEF | 0xAC00..=0xD7AF)
    }
    let mut lines = Vec::new();
    for line in s.lines() {
        let chars: Vec<char> = line.trim().chars().collect();
        let mut out = String::new();
        for (i, &c) in chars.iter().enumerate() {
            if c == ' ' || c == '\u{3000}' {
                let prev = out.chars().last();
                let next = chars[i + 1..].iter().find(|c| **c != ' ' && **c != '\u{3000}');
                if prev.is_some_and(cjk) && next.is_some_and(|n| cjk(*n)) {
                    continue;
                }
                if out.ends_with(' ') {
                    continue;
                }
                out.push(' ');
            } else {
                out.push(c);
            }
        }
        if !out.trim().is_empty() {
            lines.push(out.trim().to_string());
        }
    }
    lines.join("\n")
}

/// All OCR backends with availability diagnostics.
pub async fn ocr_backends(ollama_base_url: &str) -> Vec<BackendInfo> {
    let mut out = vec![tesseract::backend_info().await];
    out.extend(ollama_vision::backend_infos(ollama_base_url).await);
    out
}

/// Builds an adapter for a backend id (`tesseract`, `ollama`, `ollama:<model>`).
pub async fn build_adapter(
    backend: Option<&str>,
    ollama_base_url: &str,
) -> Result<std::sync::Arc<dyn OcrAdapter>, AppError> {
    match backend.map(str::trim).filter(|b| !b.is_empty()) {
        Some("tesseract") => Ok(std::sync::Arc::new(tesseract::TesseractOcr::detect().await?)),
        Some(b) if b == "ollama" || b.starts_with("ollama:") => {
            let model = b.strip_prefix("ollama:").filter(|m| !m.is_empty());
            Ok(std::sync::Arc::new(ollama_vision::OllamaVisionOcr::resolve(ollama_base_url, model).await?))
        }
        Some(other) => Err(AppError::invalid(format!("unknown OCR backend '{other}'"))),
        None => {
            // Auto: tesseract first (fast), then a local vision model.
            if let Ok(t) = tesseract::TesseractOcr::detect().await {
                return Ok(std::sync::Arc::new(t));
            }
            ollama_vision::OllamaVisionOcr::resolve(ollama_base_url, None)
                .await
                .map(|o| std::sync::Arc::new(o) as std::sync::Arc<dyn OcrAdapter>)
                .map_err(|e| {
                    AppError::new(vnl_core::ErrorCode::MissingDependency, "No OCR backend is available").with_details(format!(
                        "Tesseract: not installed (pacman -S tesseract tesseract-data-jpn). Ollama vision: {}",
                        e.message
                    ))
                })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_cjk_spacing() {
        assert_eq!(clean_ocr_text(" 今 日 は いい 天気 \n\n Hello   world "), "今日はいい天気\nHello world");
        assert_eq!(clean_ocr_text("「 あ 」 abc"), "「あ」 abc");
    }
}
