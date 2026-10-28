//! OCR with a local vision-capable Ollama model: `POST /api/chat` with
//! `messages[0].images = [<base64 PNG>]`. Only models whose `/api/tags`
//! capabilities include `vision` and that are not cloud models are used.

use async_trait::async_trait;
use base64::Engine;
use serde_json::{Value, json};
use vnl_core::{AppError, ErrorCode};

use super::{OcrAdapter, clean_ocr_text};
use crate::capture::CapturedImage;
use crate::providers::ollama::{OllamaModel, list_models, parse_chat_response};
use crate::providers::{check_status, http_client, is_local_url, join_url, network_error};
use crate::types::BackendInfo;

pub struct OllamaVisionOcr {
    pub base_url: String,
    pub model: String,
    client: reqwest::Client,
}

pub fn vision_models(models: &[OllamaModel]) -> Vec<&OllamaModel> {
    models.iter().filter(|m| m.capabilities.iter().any(|c| c == "vision") && !m.is_remote()).collect()
}

pub fn ocr_prompt(language: Option<&str>) -> String {
    let lang = language.map(crate::languages::language_name).unwrap_or_else(|| "Japanese".into());
    format!(
        "This is a screenshot of a visual novel. Transcribe the dialogue/narration text exactly as written ({lang}), \
         including the speaker name if shown. Output only the transcribed text with its original line breaks — \
         no translation, no description, no commentary. If there is no text, output nothing."
    )
}

impl OllamaVisionOcr {
    pub fn new(base_url: &str, model: &str) -> Result<Self, AppError> {
        Ok(Self { base_url: base_url.to_string(), model: model.to_string(), client: http_client(180.0)? })
    }

    /// Picks `model` (validated) or the first local vision model.
    pub async fn resolve(base_url: &str, model: Option<&str>) -> Result<Self, AppError> {
        if !is_local_url(base_url) {
            return Err(AppError::invalid(format!(
                "Ollama OCR only uses a local server; {base_url} is not on this machine"
            )));
        }
        let models = list_models(base_url).await?;
        let vision = vision_models(&models);
        let chosen = match model {
            Some(m) => vision
                .iter()
                .find(|v| v.name == m || v.name == format!("{m}:latest"))
                .ok_or_else(|| {
                    AppError::new(ErrorCode::MissingDependency, format!("'{m}' is not an installed local vision model"))
                })?
                .name
                .clone(),
            None => vision
                .first()
                .ok_or_else(|| {
                    AppError::new(ErrorCode::MissingDependency, "No vision-capable Ollama model is installed")
                        .with_details("e.g. `ollama pull qwen2.5vl:7b` or `ollama pull gemma3`")
                })?
                .name
                .clone(),
        };
        Self::new(base_url, &chosen)
    }

    pub fn request_body(&self, image: &CapturedImage, language: Option<&str>, think: Option<bool>) -> Value {
        let b64 = base64::engine::general_purpose::STANDARD.encode(&image.bytes);
        let mut body = json!({
            "model": self.model,
            "stream": false,
            "options": {"temperature": 0},
            "messages": [{"role": "user", "content": ocr_prompt(language), "images": [b64]}],
        });
        if let Some(t) = think {
            body["think"] = json!(t);
        }
        body
    }

    async fn call(&self, body: &Value) -> Result<String, AppError> {
        let resp = self
            .client
            .post(join_url(&self.base_url, "api/chat"))
            .json(body)
            .send()
            .await
            .map_err(|e| network_error("Ollama OCR", e))?;
        let resp = check_status("Ollama OCR", resp).await?;
        let v: Value = resp.json().await.map_err(|e| network_error("Ollama OCR", e))?;
        match parse_chat_response(&v) {
            Ok(s) => Ok(s),
            // An empty answer means "no text found".
            Err(e) if e.message.contains("empty") => Ok(String::new()),
            Err(e) => Err(e),
        }
    }
}

#[async_trait]
impl OcrAdapter for OllamaVisionOcr {
    fn id(&self) -> String {
        format!("ollama:{}", self.model)
    }

    async fn recognize(&self, image: &CapturedImage, language: Option<&str>) -> Result<String, AppError> {
        let text = match self.call(&self.request_body(image, language, Some(false))).await {
            Err(e) if e.message.to_lowercase().contains("think") => {
                self.call(&self.request_body(image, language, None)).await?
            }
            other => other?,
        };
        Ok(clean_ocr_text(&text))
    }
}

pub async fn backend_infos(base_url: &str) -> Vec<BackendInfo> {
    if !is_local_url(base_url) {
        return vec![BackendInfo {
            id: "ollama".into(),
            name: "Ollama vision model".into(),
            available: false,
            diagnostics: vec![format!("Ollama URL {base_url} is not local; OCR images are never sent to remote servers.")],
        }];
    }
    match list_models(base_url).await {
        Ok(models) => {
            let vision = vision_models(&models);
            if vision.is_empty() {
                let cloud_vision = models.iter().any(|m| m.is_remote() && m.capabilities.iter().any(|c| c == "vision"));
                let mut d = vec![
                    "No local vision-capable model installed (e.g. `ollama pull qwen2.5vl:7b`).".to_string(),
                ];
                if cloud_vision {
                    d.push("Cloud vision models are ignored for privacy.".into());
                }
                return vec![BackendInfo { id: "ollama".into(), name: "Ollama vision model".into(), available: false, diagnostics: d }];
            }
            vision
                .into_iter()
                .map(|m| BackendInfo {
                    id: format!("ollama:{}", m.name),
                    name: format!("Ollama vision: {}", m.name),
                    available: true,
                    diagnostics: vec![format!(
                        "Local model with capabilities [{}]; slower than Tesseract (seconds per capture) but handles stylised fonts.",
                        m.capabilities.join(", ")
                    )],
                })
                .collect()
        }
        Err(e) => vec![BackendInfo {
            id: "ollama".into(),
            name: "Ollama vision model".into(),
            available: false,
            diagnostics: vec![format!("Ollama not reachable at {base_url}: {}", e.message)],
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn picks_local_vision_model_and_sends_image() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/tags"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"models": [
                {"name": "text:1b", "capabilities": ["completion"]},
                {"name": "vis-cloud:cloud", "remote_host": "https://ollama.com", "capabilities": ["completion", "vision"]},
                {"name": "vis:7b", "capabilities": ["completion", "vision"]}
            ]})))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"message": {"content": "今日は いい 天気"}})))
            .mount(&server)
            .await;
        let infos = backend_infos(&server.uri()).await;
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].id, "ollama:vis:7b");
        let ocr = OllamaVisionOcr::resolve(&server.uri(), None).await.unwrap();
        assert_eq!(ocr.model, "vis:7b");
        assert!(OllamaVisionOcr::resolve(&server.uri(), Some("vis-cloud:cloud")).await.is_err());
        let img = CapturedImage { bytes: vec![1, 2, 3], source: "t".into() };
        assert_eq!(ocr.recognize(&img, Some("ja")).await.unwrap(), "今日はいい天気");
        let reqs = server.received_requests().await.unwrap();
        let chat = reqs.iter().find(|r| r.url.path() == "/api/chat").unwrap();
        let body: Value = serde_json::from_slice(&chat.body).unwrap();
        assert_eq!(body["messages"][0]["images"][0], "AQID");
        assert_eq!(body["think"], false);
    }

    #[tokio::test]
    async fn remote_base_url_refused() {
        assert!(OllamaVisionOcr::resolve("https://ollama.example.com", None).await.is_err());
        assert!(!backend_infos("https://ollama.example.com").await[0].available);
    }
}
