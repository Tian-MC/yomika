//! DeepL API (<https://developers.deepl.com/api-reference/translate>):
//! `POST https://api-free.deepl.com/v2/translate` (free keys, ending in `:fx`)
//! or `https://api.deepl.com/v2/translate` (pro), header
//! `Authorization: DeepL-Auth-Key <key>`, JSON `{text:[..], target_lang, source_lang?}`
//! → `{translations:[{detected_source_language, text}]}`.
//! Errors: 403 bad key, 456 quota exhausted, 429 too many requests.

use async_trait::async_trait;
use serde_json::{Map, Value, json};
use vnl_core::{AppError, ErrorCode};

use super::{
    GlossaryStrategy, ProviderRequest, TranslationProvider, cfg_f64, cfg_str, http_client, network_error, validate_url,
};
use crate::types::{FieldKind, ProviderConfigField, ProviderKind};

pub const ID: &str = "deepl";
pub const FREE_ENDPOINT: &str = "https://api-free.deepl.com/v2/translate";
pub const PRO_ENDPOINT: &str = "https://api.deepl.com/v2/translate";

pub fn fields() -> Vec<ProviderConfigField> {
    vec![
        ProviderConfigField::new("plan", "Plan", FieldKind::Select, false, json!("auto")).with_options(&["auto", "free", "pro"]),
        ProviderConfigField::new("formality", "Formality", FieldKind::Select, false, json!("default")).with_options(&[
            "default",
            "prefer_more",
            "prefer_less",
        ]),
        ProviderConfigField::new("endpoint", "Custom endpoint (advanced)", FieldKind::Url, false, json!("")),
        ProviderConfigField::new("timeoutSecs", "Timeout (seconds)", FieldKind::Number, false, json!(30)),
    ]
}

/// Free-plan keys end with `:fx`.
pub fn endpoint_for(plan: &str, key: &str) -> &'static str {
    match plan {
        "free" => FREE_ENDPOINT,
        "pro" => PRO_ENDPOINT,
        _ if key.trim().ends_with(":fx") => FREE_ENDPOINT,
        _ => PRO_ENDPOINT,
    }
}

pub fn source_code(code: &str) -> String {
    crate::languages::primary_subtag(code).to_ascii_uppercase()
}

pub fn target_code(code: &str) -> String {
    match code.to_ascii_lowercase().as_str() {
        "zh" | "zh-hans" | "zh-cn" => "ZH-HANS".into(),
        "zh-hant" | "zh-tw" | "zh-hk" => "ZH-HANT".into(),
        "en-us" => "EN-US".into(),
        "en-gb" => "EN-GB".into(),
        "pt-br" => "PT-BR".into(),
        "pt-pt" => "PT-PT".into(),
        other => crate::languages::primary_subtag(other).to_ascii_uppercase(),
    }
}

pub struct DeepLProvider {
    endpoint: String,
    key: String,
    formality: String,
    client: reqwest::Client,
}

impl DeepLProvider {
    pub fn from_config(cfg: &Map<String, Value>, api_key: Option<String>) -> Result<Self, AppError> {
        let key = api_key
            .filter(|k| !k.trim().is_empty())
            .ok_or_else(|| AppError::invalid("DeepL: add your API key in the provider settings"))?;
        let custom = cfg_str(cfg, "endpoint", "");
        let endpoint = if custom.is_empty() {
            endpoint_for(&cfg_str(cfg, "plan", "auto"), &key).to_string()
        } else {
            validate_url(&custom, "Custom endpoint")?;
            custom
        };
        Ok(Self {
            endpoint,
            key,
            formality: cfg_str(cfg, "formality", "default"),
            client: http_client(cfg_f64(cfg, "timeoutSecs", 30.0))?,
        })
    }

    pub fn request_body(&self, req: &ProviderRequest) -> Value {
        let mut body = json!({
            "text": [req.text],
            "target_lang": target_code(&req.target_language),
            "preserve_formatting": true,
        });
        if let Some(src) = &req.source_language {
            body["source_lang"] = json!(source_code(src));
        }
        if self.formality != "default" {
            body["formality"] = json!(self.formality);
        }
        body
    }
}

#[async_trait]
impl TranslationProvider for DeepLProvider {
    fn id(&self) -> &str {
        ID
    }

    fn kind(&self) -> ProviderKind {
        ProviderKind::Online
    }

    fn sends_data_remotely(&self) -> bool {
        true
    }

    fn glossary_strategy(&self) -> GlossaryStrategy {
        GlossaryStrategy::Placeholders
    }

    async fn translate(&self, req: &ProviderRequest) -> Result<String, AppError> {
        let resp = self
            .client
            .post(&self.endpoint)
            .header("Authorization", format!("DeepL-Auth-Key {}", self.key))
            .json(&self.request_body(req))
            .send()
            .await
            .map_err(|e| network_error("DeepL", e))?;
        let status = resp.status().as_u16();
        if !(200..300).contains(&status) {
            let body = resp.text().await.unwrap_or_default();
            let detail = serde_json::from_str::<Value>(&body)
                .ok()
                .and_then(|v| v.get("message").and_then(Value::as_str).map(str::to_string))
                .unwrap_or_else(|| body.chars().take(200).collect());
            let msg = match status {
                403 => "DeepL rejected the API key (HTTP 403). Check the key and the free/pro plan setting.".to_string(),
                456 => "DeepL character quota exhausted (HTTP 456).".to_string(),
                429 => "DeepL: too many requests (HTTP 429); try again shortly.".to_string(),
                s => format!("DeepL: HTTP {s}"),
            };
            return Err(AppError::new(ErrorCode::Network, msg).with_details(detail));
        }
        let v: Value = resp.json().await.map_err(|e| network_error("DeepL", e))?;
        v.pointer("/translations/0/text")
            .and_then(Value::as_str)
            .map(|s| s.trim().to_string())
            .ok_or_else(|| AppError::internal("DeepL: unexpected response (no translations[0].text)"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn provider(endpoint: &str) -> DeepLProvider {
        let mut cfg = Map::new();
        cfg.insert("endpoint".into(), json!(endpoint));
        DeepLProvider::from_config(&cfg, Some("abc:fx".into())).unwrap()
    }

    #[test]
    fn endpoints_and_codes() {
        assert_eq!(endpoint_for("auto", "abc:fx"), FREE_ENDPOINT);
        assert_eq!(endpoint_for("auto", "abc"), PRO_ENDPOINT);
        assert_eq!(endpoint_for("free", "abc"), FREE_ENDPOINT);
        assert_eq!(target_code("en-GB"), "EN-GB");
        assert_eq!(target_code("zh-Hant"), "ZH-HANT");
        assert_eq!(target_code("de"), "DE");
        assert_eq!(source_code("ja"), "JA");
        assert!(DeepLProvider::from_config(&Map::new(), None).is_err());
    }

    #[tokio::test]
    async fn translate_format() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v2/translate"))
            .and(header("authorization", "DeepL-Auth-Key abc:fx"))
            .and(body_json(json!({"text": ["おはよう"], "target_lang": "PT-BR", "source_lang": "JA", "preserve_formatting": true})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "translations": [{"detected_source_language": "JA", "text": "Bom dia"}]
            })))
            .expect(1)
            .mount(&server)
            .await;
        let p = provider(&format!("{}/v2/translate", server.uri()));
        let req = ProviderRequest {
            text: "おはよう".into(),
            source_language: Some("ja".into()),
            target_language: "pt-BR".into(),
            glossary: vec![],
        };
        assert_eq!(p.translate(&req).await.unwrap(), "Bom dia");
        assert!(p.sends_data_remotely());
    }

    #[tokio::test]
    async fn quota_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(456).set_body_json(json!({"message": "Quota exceeded"})))
            .mount(&server)
            .await;
        let p = provider(&server.uri());
        let req = ProviderRequest { text: "x".into(), source_language: None, target_language: "en".into(), glossary: vec![] };
        let err = p.translate(&req).await.unwrap_err();
        assert!(err.message.contains("quota"));
        assert_eq!(err.details.as_deref(), Some("Quota exceeded"));
    }
}
