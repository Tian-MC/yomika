//! Tesseract CLI backend: `tesseract - - -l jpn --psm 6` (image on stdin,
//! text on stdout). Requires the `jpn` (horizontal) and optionally `jpn_vert`
//! (vertical) traineddata.

use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use tokio::io::AsyncWriteExt;
use vnl_core::{AppError, ErrorCode};

use super::{OcrAdapter, clean_ocr_text};
use crate::capture::CapturedImage;
use crate::types::BackendInfo;

pub const INSTALL_HINT: &str = "Install Tesseract with Japanese data: `pacman -S tesseract tesseract-data-jpn tesseract-data-jpn_vert` \
(Debian/Ubuntu: `apt install tesseract-ocr tesseract-ocr-jpn tesseract-ocr-jpn-vert`; Fedora: `dnf install tesseract tesseract-langpack-jpn`).";

/// BCP-47 → tesseract language name.
pub fn tesseract_lang(code: Option<&str>, vertical: bool) -> String {
    let base = match code.map(|c| c.to_ascii_lowercase()) {
        None => "jpn".to_string(),
        Some(c) => match c.as_str() {
            "ja" => "jpn".into(),
            "zh" | "zh-hans" | "zh-cn" => "chi_sim".into(),
            "zh-hant" | "zh-tw" | "zh-hk" => "chi_tra".into(),
            "ko" => "kor".into(),
            "en" | "en-us" | "en-gb" => "eng".into(),
            "fr" => "fra".into(),
            "de" => "deu".into(),
            "es" => "spa".into(),
            "ru" => "rus".into(),
            other => crate::languages::primary_subtag(other).to_string(),
        },
    };
    if vertical && matches!(base.as_str(), "jpn" | "chi_sim" | "chi_tra" | "kor") { format!("{base}_vert") } else { base }
}

pub fn parse_list_langs(output: &str) -> Vec<String> {
    output
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("List of available languages"))
        .map(str::to_string)
        .collect()
}

pub struct TesseractOcr {
    pub program: PathBuf,
    pub languages: Vec<String>,
    pub vertical: bool,
}

async fn list_langs(program: &PathBuf) -> Result<Vec<String>, AppError> {
    let out = tokio::process::Command::new(program)
        .arg("--list-langs")
        .output()
        .await
        .map_err(|e| AppError::new(ErrorCode::Process, format!("cannot run tesseract: {e}")))?;
    // Older versions print the list on stderr.
    let text = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    Ok(parse_list_langs(&text))
}

impl TesseractOcr {
    pub async fn detect() -> Result<Self, AppError> {
        let program = which::which("tesseract")
            .map_err(|_| AppError::new(ErrorCode::MissingDependency, "tesseract is not installed").with_details(INSTALL_HINT))?;
        let languages = list_langs(&program).await?;
        Ok(Self { program, languages, vertical: false })
    }

    pub fn args(&self, language: Option<&str>) -> Vec<String> {
        let lang = tesseract_lang(language, self.vertical);
        let psm = if self.vertical { "5" } else { "6" };
        vec!["-".into(), "-".into(), "-l".into(), lang, "--psm".into(), psm.into()]
    }
}

#[async_trait]
impl OcrAdapter for TesseractOcr {
    fn id(&self) -> String {
        "tesseract".into()
    }

    async fn recognize(&self, image: &CapturedImage, language: Option<&str>) -> Result<String, AppError> {
        let lang = tesseract_lang(language, self.vertical);
        if !self.languages.iter().any(|l| l == &lang) {
            return Err(AppError::new(
                ErrorCode::MissingDependency,
                format!("Tesseract language data '{lang}' is not installed"),
            )
            .with_details(INSTALL_HINT));
        }
        let mut child = tokio::process::Command::new(&self.program)
            .args(self.args(language))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| AppError::new(ErrorCode::Process, format!("cannot run tesseract: {e}")))?;
        let mut stdin = child.stdin.take().expect("piped stdin");
        let data = image.bytes.clone();
        let writer = tokio::spawn(async move {
            let _ = stdin.write_all(&data).await;
        });
        let out = tokio::time::timeout(Duration::from_secs(60), child.wait_with_output())
            .await
            .map_err(|_| AppError::new(ErrorCode::Process, "tesseract timed out"))??;
        let _ = writer.await;
        if !out.status.success() {
            return Err(AppError::new(ErrorCode::Process, "tesseract failed")
                .with_details(String::from_utf8_lossy(&out.stderr).to_string()));
        }
        Ok(clean_ocr_text(&String::from_utf8_lossy(&out.stdout)))
    }
}

pub async fn backend_info() -> BackendInfo {
    let mut diagnostics = Vec::new();
    let available = match TesseractOcr::detect().await {
        Ok(t) => {
            let has_jpn = t.languages.iter().any(|l| l == "jpn");
            let has_vert = t.languages.iter().any(|l| l == "jpn_vert");
            diagnostics.push(format!("{} (languages: {})", t.program.display(), t.languages.join(", ")));
            if !has_jpn {
                diagnostics.push(format!("Japanese data 'jpn' missing. {INSTALL_HINT}"));
            }
            if !has_vert {
                diagnostics.push("Vertical Japanese data 'jpn_vert' missing (only needed for vertical text).".into());
            }
            has_jpn || !t.languages.is_empty()
        }
        Err(e) => {
            diagnostics.push(e.message.clone());
            diagnostics.push(INSTALL_HINT.into());
            false
        }
    };
    BackendInfo { id: "tesseract".into(), name: "Tesseract OCR".into(), available, diagnostics }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lang_mapping() {
        assert_eq!(tesseract_lang(Some("ja"), false), "jpn");
        assert_eq!(tesseract_lang(Some("ja"), true), "jpn_vert");
        assert_eq!(tesseract_lang(Some("zh-Hant"), false), "chi_tra");
        assert_eq!(tesseract_lang(None, false), "jpn");
    }

    #[test]
    fn list_langs_parsing() {
        let out = "List of available languages in \"/usr/share/tessdata/\" (3):\neng\njpn\njpn_vert\n";
        assert_eq!(parse_list_langs(out), vec!["eng", "jpn", "jpn_vert"]);
    }

    #[tokio::test]
    async fn real_tesseract_if_installed() {
        let Ok(t) = TesseractOcr::detect().await else {
            eprintln!("SKIPPED: tesseract is not installed on this machine ({INSTALL_HINT})");
            return;
        };
        let Ok(magick) = which::which("magick") else {
            eprintln!("SKIPPED: ImageMagick not installed; cannot render a test image");
            return;
        };
        if !t.languages.iter().any(|l| l == "eng") {
            eprintln!("SKIPPED: tesseract 'eng' data missing");
            return;
        }
        let out = std::process::Command::new(magick)
            .args(["-size", "600x120", "xc:white", "-fill", "black", "-pointsize", "48", "-annotate", "+20+80", "HELLO OCR", "png:-"])
            .output()
            .unwrap();
        let img = CapturedImage { bytes: out.stdout, source: "test".into() };
        let text = t.recognize(&img, Some("en")).await.unwrap();
        assert!(text.contains("HELLO"), "{text}");
    }
}
