//! OpenAI-compatible chat completions
//! (<https://platform.openai.com/docs/api-reference/chat/create>):
//! `POST {baseUrl}/chat/completions` with `{model, messages, temperature, stream:false}`
//! and optional `Authorization: Bearer <key>` → `{choices:[{message:{content}}]}`.
//! Works with llama.cpp `llama-server`, LM Studio, vLLM, LocalAI and hosted
//! services. Remote-ness is derived from the base URL host.

use async_trait::async_trait;
use serde_json::{Map, Value, json};
use vnl_core::AppError;

use super::{
    GlossaryStrategy, ProviderRequest, TranslationProvider, cfg_f64, cfg_str, check_status, clean_llm_output, http_client,
    is_local_url, join_url, llm_system_prompt, network_error, validate_url,
};
use crate::types::{FieldKind, ProviderConfigField, ProviderKind};

pub const ID: &str = "openai-compatible";

pub fn fields() -> Vec<ProviderConfigField> {
    vec![
        ProviderConfigField::new("baseUrl", "Base URL (including /v1)", FieldKind::Url, true, json!("http://127.0.0.1:8080/v1")),
        ProviderConfigField::new("model", "Model", FieldKind::String, true, json!("")),
        ProviderConfigField::new("temperature", "Temperature", FieldKind::Number, false, json!(0.2)),
        ProviderConfigField::new("timeoutSecs", "Timeout (seconds)", FieldKind::Number, false, json!(120)),
    ]
}

pub struct OpenAiProvider {
    base_url: String,
    model: String,
    temperature: f64,
    api_key: Option<String>,
    client: reqwest::Client,
}

impl OpenAiProvider {
    pub fn from_config(cfg: &Map<String, Value>, api_key: Option<String>) -> Result<Self, AppError> {
        let base_url = cfg_str(cfg, "baseUrl", "");
        if base_url.is_empty() {
            return Err(AppError::invalid("OpenAI-compatible: set the base URL in the provider settings"));
        }
        validate_url(&base_url, "Base URL")?;
        let model = cfg_str(cfg, "model", "");
        if model.is_empty() {
            return Err(AppError::invalid("OpenAI-compatible: set the model name in the provider settings"));
        }
        Ok(Self {
            base_url,
            model,
            temperature: cfg_f64(cfg, "temperature", 0.2),
            api_key: api_key.filter(|k| !k.is_empty()),
            client: http_client(cfg_f64(cfg, "timeoutSecs", 120.0))?,
        })
    }

    pub fn request_body(&self, req: &ProviderRequest) -> Value {
        json!({
            "model": self.model,
            "stream": false,
            "temperature": self.temperature,
            "messages": [
                {"role": "system", "content": llm_system_prompt(req)},
                {"role": "user", "content": req.text},
            ],
        })
    }
}

pub fn parse_response(v: &Value) -> Result<String, AppError> {
    if let Some(err) = v.get("error") {
        let msg = err.get("message").and_then(Value::as_str).or_else(|| err.as_str()).unwrap_or("unknown error");
        return Err(AppError::new(vnl_core::ErrorCode::Network, format!("OpenAI-compatible: {msg}")));
    }
    let content = v
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::internal("OpenAI-compatible: unexpected response (no choices[0].message.content)"))?;
    let out = clean_llm_output(content);
    if out.is_empty() {
        return Err(AppError::internal("OpenAI-compatible server returned an empty translation"));
    }
    Ok(out)
}

#[async_trait]
impl TranslationProvider for OpenAiProvider {
    fn id(&self) -> &str {
        ID
    }

    fn kind(&self) -> ProviderKind {
        if self.sends_data_remotely() { ProviderKind::Online } else { ProviderKind::Local }
    }

    fn sends_data_remotely(&self) -> bool {
        !is_local_url(&self.base_url)
    }

    fn glossary_strategy(&self) -> GlossaryStrategy {
        GlossaryStrategy::PromptHints
    }

    async fn translate(&self, req: &ProviderRequest) -> Result<String, AppError> {
        let mut builder = self.client.post(join_url(&self.base_url, "chat/completions")).json(&self.request_body(req));
        if let Some(key) = &self.api_key {
            builder = builder.bearer_auth(key);
        }
        let resp = builder.send().await.map_err(|e| network_error("OpenAI-compatible", e))?;
        let resp = check_status("OpenAI-compatible", resp).await?;
        let v: Value = resp.json().await.map_err(|e| network_error("OpenAI-compatible", e))?;
        parse_response(&v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::GlossaryHint;
    use crate::types::GlossaryKind;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn provider(base: &str, key: Option<&str>) -> OpenAiProvider {
        let mut cfg = Map::new();
        cfg.insert("baseUrl".into(), json!(base));
        cfg.insert("model".into(), json!("local-gguf"));
        OpenAiProvider::from_config(&cfg, key.map(String::from)).unwrap()
    }

    fn req() -> ProviderRequest {
        ProviderRequest {
            text: "先輩、待って！".into(),
            source_language: Some("ja".into()),
            target_language: "es".into(),
            glossary: vec![GlossaryHint { source: "先輩".into(), target: "Senpai".into(), kind: GlossaryKind::Term }],
        }
    }

    #[tokio::test]
    async fn chat_completions_with_key() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(header("authorization", "Bearer sk-test"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "x", "object": "chat.completion",
                "choices": [{"index": 0, "message": {"role": "assistant", "content": "¡Senpai, espera!"}, "finish_reason": "stop"}]
            })))
            .expect(1)
            .mount(&server)
            .await;
        let p = provider(&format!("{}/v1", server.uri()), Some("sk-test"));
        assert!(!p.sends_data_remotely());
        assert_eq!(p.translate(&req()).await.unwrap(), "¡Senpai, espera!");
        let r = &server.received_requests().await.unwrap()[0];
        let body: Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(body["model"], "local-gguf");
        assert_eq!(body["stream"], false);
        assert!(body["messages"][0]["content"].as_str().unwrap().contains("先輩 → Senpai"));
    }

    #[tokio::test]
    async fn no_auth_header_without_key_and_error_body() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(401).set_body_json(json!({"error": {"message": "Invalid API key", "type": "auth"}})))
            .mount(&server)
            .await;
        let err = provider(&server.uri(), None).translate(&req()).await.unwrap_err();
        assert!(err.message.contains("Invalid API key"), "{}", err.message);
        let r = &server.received_requests().await.unwrap()[0];
        assert!(r.headers.get("authorization").is_none());
    }

    #[test]
    fn remote_flag_from_host() {
        assert!(provider("https://api.example.org/v1", None).sends_data_remotely());
        assert!(!provider("http://localhost:1234/v1", None).sends_data_remotely());
    }
}
