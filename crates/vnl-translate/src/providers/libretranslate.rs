//! LibreTranslate (<https://docs.libretranslate.com/guides/api_usage/>):
//! `POST {url}/translate` JSON `{q, source, target, format:"text", api_key?}` →
//! `{translatedText}`; errors are `{error: "..."}`.

use async_trait::async_trait;
use serde_json::{Map, Value, json};
use vnl_core::AppError;

use super::{
    GlossaryStrategy, ProviderRequest, TranslationProvider, cfg_f64, cfg_str, check_status, http_client, is_local_url,
    join_url, network_error, validate_url,
};
use crate::types::{FieldKind, ProviderConfigField, ProviderKind};

pub const ID: &str = "libretranslate";

pub fn fields() -> Vec<ProviderConfigField> {
    vec![
        ProviderConfigField::new("url", "Server URL", FieldKind::Url, true, json!("http://127.0.0.1:5000")),
        ProviderConfigField::new("timeoutSecs", "Timeout (seconds)", FieldKind::Number, false, json!(30)),
    ]
}

/// LibreTranslate uses ISO 639-1 codes, plus `zh-Hans`/`zh-Hant` and `pt-BR`.
pub fn map_code(code: &str) -> String {
    let lower = code.to_ascii_lowercase();
    match lower.as_str() {
        "zh" | "zh-hans" | "zh-cn" | "zh-sg" => "zh-Hans".into(),
        "zh-hant" | "zh-tw" | "zh-hk" => "zh-Hant".into(),
        "pt-br" => "pt-BR".into(),
        _ => crate::languages::primary_subtag(&lower).to_string(),
    }
}

pub struct LibreTranslateProvider {
    url: String,
    api_key: Option<String>,
    client: reqwest::Client,
}

impl LibreTranslateProvider {
    pub fn from_config(cfg: &Map<String, Value>, api_key: Option<String>) -> Result<Self, AppError> {
        let url = cfg_str(cfg, "url", "");
        if url.is_empty() {
            return Err(AppError::invalid("LibreTranslate: set the server URL in the provider settings"));
        }
        validate_url(&url, "Server URL")?;
        Ok(Self {
            url,
            api_key: api_key.filter(|k| !k.is_empty()),
            client: http_client(cfg_f64(cfg, "timeoutSecs", 30.0))?,
        })
    }

    pub fn request_body(&self, req: &ProviderRequest) -> Value {
        let mut body = json!({
            "q": req.text,
            "source": req.source_language.as_deref().map(map_code).unwrap_or_else(|| "auto".into()),
            "target": map_code(&req.target_language),
            "format": "text",
        });
        if let Some(k) = &self.api_key {
            body["api_key"] = json!(k);
        }
        body
    }
}

#[async_trait]
impl TranslationProvider for LibreTranslateProvider {
    fn id(&self) -> &str {
        ID
    }

    fn kind(&self) -> ProviderKind {
        if self.sends_data_remotely() { ProviderKind::Online } else { ProviderKind::Local }
    }

    fn sends_data_remotely(&self) -> bool {
        !is_local_url(&self.url)
    }

    fn glossary_strategy(&self) -> GlossaryStrategy {
        GlossaryStrategy::Placeholders
    }

    async fn translate(&self, req: &ProviderRequest) -> Result<String, AppError> {
        let resp = self
            .client
            .post(join_url(&self.url, "translate"))
            .json(&self.request_body(req))
            .send()
            .await
            .map_err(|e| network_error("LibreTranslate", e))?;
        let resp = check_status("LibreTranslate", resp).await?;
        let v: Value = resp.json().await.map_err(|e| network_error("LibreTranslate", e))?;
        if let Some(err) = v.get("error").and_then(Value::as_str) {
            return Err(AppError::new(vnl_core::ErrorCode::Network, format!("LibreTranslate: {err}")));
        }
        v.get("translatedText")
            .and_then(Value::as_str)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| AppError::internal("LibreTranslate: unexpected response (no translatedText)"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn codes() {
        assert_eq!(map_code("zh-Hant"), "zh-Hant");
        assert_eq!(map_code("zh-CN"), "zh-Hans");
        assert_eq!(map_code("en-US"), "en");
        assert_eq!(map_code("pt-BR"), "pt-BR");
    }

    #[tokio::test]
    async fn translate_request_format() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/translate"))
            .and(body_json(
                json!({"q": "こんにちは", "source": "ja", "target": "de", "format": "text", "api_key": "lt-key"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"translatedText": "Hallo"})))
            .expect(1)
            .mount(&server)
            .await;
        let mut cfg = Map::new();
        cfg.insert("url".into(), json!(server.uri()));
        let p = LibreTranslateProvider::from_config(&cfg, Some("lt-key".into())).unwrap();
        let req = ProviderRequest {
            text: "こんにちは".into(),
            source_language: Some("ja".into()),
            target_language: "de".into(),
            glossary: vec![],
        };
        assert_eq!(p.translate(&req).await.unwrap(), "Hallo");
    }

    #[tokio::test]
    async fn auto_source_and_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/translate"))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({"error": "ja is not supported"})))
            .mount(&server)
            .await;
        let mut cfg = Map::new();
        cfg.insert("url".into(), json!(server.uri()));
        let p = LibreTranslateProvider::from_config(&cfg, None).unwrap();
        let req =
            ProviderRequest { text: "x".into(), source_language: None, target_language: "en".into(), glossary: vec![] };
        assert_eq!(p.request_body(&req)["source"], "auto");
        assert!(p.request_body(&req).get("api_key").is_none());
        let err = p.translate(&req).await.unwrap_err();
        assert!(err.message.contains("not supported"));
    }
}
