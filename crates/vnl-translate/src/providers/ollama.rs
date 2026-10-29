//! Ollama (<https://github.com/ollama/ollama/blob/main/docs/api.md>):
//! `POST /api/chat` with `{model, messages, stream:false, think, options}` →
//! `{message:{role,content}, done}`. `GET /api/tags` lists installed models;
//! cloud models (`…:cloud`, `…-cloud`) carry `remote_host` and run on
//! ollama.com, so they are refused unless `allowRemoteModels` is enabled.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use vnl_core::AppError;

use super::{
    GlossaryStrategy, ProviderRequest, TranslationProvider, cfg_bool, cfg_f64, cfg_str, check_status, clean_llm_output,
    http_client, is_local_url, join_url, llm_system_prompt, network_error, validate_url,
};
use crate::types::{FieldKind, ProviderConfigField, ProviderKind};

pub const ID: &str = "ollama";
pub const DEFAULT_BASE_URL: &str = "http://127.0.0.1:11434";

pub fn fields() -> Vec<ProviderConfigField> {
    vec![
        ProviderConfigField::new("baseUrl", "Server URL", FieldKind::Url, true, json!(DEFAULT_BASE_URL)),
        ProviderConfigField::new("model", "Model", FieldKind::Select, true, json!("")),
        ProviderConfigField::new("temperature", "Temperature", FieldKind::Number, false, json!(0.2)),
        ProviderConfigField::new(
            "disableThinking",
            "Disable reasoning (faster)",
            FieldKind::Boolean,
            false,
            json!(true),
        ),
        ProviderConfigField::new(
            "allowRemoteModels",
            "Allow Ollama cloud models (sends text to ollama.com)",
            FieldKind::Boolean,
            false,
            json!(false),
        ),
        ProviderConfigField::new("timeoutSecs", "Timeout (seconds)", FieldKind::Number, false, json!(120)),
    ]
}

pub fn is_cloud_model_name(model: &str) -> bool {
    let tag = model.rsplit_once(':').map(|(_, t)| t).unwrap_or("");
    tag == "cloud" || tag.ends_with("-cloud")
}

#[derive(Debug, Clone, Deserialize)]
pub struct OllamaModel {
    pub name: String,
    #[serde(default)]
    pub remote_host: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

impl OllamaModel {
    pub fn is_remote(&self) -> bool {
        self.remote_host.is_some() || is_cloud_model_name(&self.name)
    }
}

#[derive(Deserialize)]
struct TagsResponse {
    #[serde(default)]
    models: Vec<OllamaModel>,
}

/// `GET /api/tags`.
pub async fn list_models(base_url: &str) -> Result<Vec<OllamaModel>, AppError> {
    let client = http_client(3.0)?;
    let resp = client.get(join_url(base_url, "api/tags")).send().await.map_err(|e| network_error("Ollama", e))?;
    let resp = check_status("Ollama", resp).await?;
    let tags: TagsResponse = resp.json().await.map_err(|e| network_error("Ollama", e))?;
    Ok(tags.models)
}

pub struct OllamaProbe {
    pub models: Option<Vec<OllamaModel>>,
    pub model_ok: bool,
    pub remote: bool,
    pub notes: Vec<String>,
}

/// Availability check. Only contacts loopback servers.
pub async fn probe(cfg: &Map<String, Value>) -> OllamaProbe {
    let base = cfg_str(cfg, "baseUrl", DEFAULT_BASE_URL);
    let model = cfg_str(cfg, "model", "");
    let local = is_local_url(&base);
    let mut p =
        OllamaProbe { models: None, model_ok: false, remote: !local || is_cloud_model_name(&model), notes: vec![] };
    if !local {
        p.notes.push(format!("Server {base} is not on this machine; not probed."));
        p.model_ok = !model.is_empty();
        return p;
    }
    match list_models(&base).await {
        Ok(models) => {
            match models.iter().find(|m| m.name == model || m.name == format!("{model}:latest")) {
                Some(m) => {
                    p.model_ok = true;
                    if m.is_remote() {
                        p.remote = true;
                        p.notes.push(format!(
                            "'{}' is an Ollama cloud model (runs on {}).",
                            m.name,
                            m.remote_host.as_deref().unwrap_or("ollama.com")
                        ));
                        if !cfg_bool(cfg, "allowRemoteModels", false) {
                            p.model_ok = false;
                            p.notes.push("Enable 'Allow Ollama cloud models' to use it.".into());
                        }
                    }
                }
                None if model.is_empty() => p.notes.push("Choose a model.".into()),
                None => p.notes.push(format!("Model '{model}' is not installed (run `ollama pull {model}`).")),
            }
            p.models = Some(models);
        }
        Err(e) => p.notes.push(format!("Ollama is not reachable at {base}: {}", e.message)),
    }
    p
}

pub struct OllamaProvider {
    base_url: String,
    model: String,
    temperature: f64,
    disable_thinking: bool,
    allow_remote_models: bool,
    client: reqwest::Client,
}

impl OllamaProvider {
    pub fn from_config(cfg: &Map<String, Value>) -> Result<Self, AppError> {
        let base_url = cfg_str(cfg, "baseUrl", DEFAULT_BASE_URL);
        validate_url(&base_url, "Server URL")?;
        let model = cfg_str(cfg, "model", "");
        if model.is_empty() {
            return Err(AppError::invalid("Ollama: choose a model in the provider settings"));
        }
        Ok(Self {
            base_url,
            model,
            temperature: cfg_f64(cfg, "temperature", 0.2),
            disable_thinking: cfg_bool(cfg, "disableThinking", true),
            allow_remote_models: cfg_bool(cfg, "allowRemoteModels", false),
            client: http_client(cfg_f64(cfg, "timeoutSecs", 120.0))?,
        })
    }

    pub fn request_body(&self, req: &ProviderRequest, think: Option<bool>) -> Value {
        let mut body = json!({
            "model": self.model,
            "stream": false,
            "messages": [
                {"role": "system", "content": llm_system_prompt(req)},
                {"role": "user", "content": req.text},
            ],
            "options": {"temperature": self.temperature},
        });
        if let Some(t) = think {
            body["think"] = json!(t);
        }
        body
    }

    async fn ensure_not_cloud(&self) -> Result<(), AppError> {
        if self.allow_remote_models {
            return Ok(());
        }
        let remote = if is_cloud_model_name(&self.model) {
            true
        } else if is_local_url(&self.base_url) {
            // Best effort: if listing fails, the chat call reports the real error.
            list_models(&self.base_url)
                .await
                .map(|ms| {
                    ms.iter()
                        .any(|m| (m.name == self.model || m.name == format!("{}:latest", self.model)) && m.is_remote())
                })
                .unwrap_or(false)
        } else {
            false
        };
        if remote {
            return Err(AppError::invalid(format!(
                "Ollama model '{}' is a cloud model that sends text to ollama.com. Enable 'Allow Ollama cloud models' in the provider settings to use it.",
                self.model
            )));
        }
        Ok(())
    }

    async fn chat(&self, body: &Value) -> Result<String, AppError> {
        let resp = self
            .client
            .post(join_url(&self.base_url, "api/chat"))
            .json(body)
            .send()
            .await
            .map_err(|e| network_error("Ollama", e))?;
        let resp = check_status("Ollama", resp).await?;
        let v: Value = resp.json().await.map_err(|e| network_error("Ollama", e))?;
        parse_chat_response(&v)
    }
}

pub fn parse_chat_response(v: &Value) -> Result<String, AppError> {
    if let Some(err) = v.get("error").and_then(Value::as_str) {
        return Err(AppError::new(vnl_core::ErrorCode::Network, format!("Ollama: {err}")));
    }
    let content = v
        .pointer("/message/content")
        .and_then(Value::as_str)
        .or_else(|| v.get("response").and_then(Value::as_str))
        .ok_or_else(|| AppError::internal("Ollama: unexpected response (no message.content)"))?;
    let out = clean_llm_output(content);
    if out.is_empty() {
        return Err(AppError::internal("Ollama returned an empty translation"));
    }
    Ok(out)
}

#[async_trait]
impl TranslationProvider for OllamaProvider {
    fn id(&self) -> &str {
        ID
    }

    fn kind(&self) -> ProviderKind {
        if self.sends_data_remotely() { ProviderKind::Online } else { ProviderKind::Local }
    }

    fn sends_data_remotely(&self) -> bool {
        !is_local_url(&self.base_url) || is_cloud_model_name(&self.model)
    }

    fn glossary_strategy(&self) -> GlossaryStrategy {
        GlossaryStrategy::PromptHints
    }

    async fn translate(&self, req: &ProviderRequest) -> Result<String, AppError> {
        self.ensure_not_cloud().await?;
        let think = if self.disable_thinking { Some(false) } else { None };
        match self.chat(&self.request_body(req, think)).await {
            // Models without reasoning support may reject the `think` flag.
            Err(e) if think.is_some() && e.message.to_lowercase().contains("think") => {
                self.chat(&self.request_body(req, None)).await
            }
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn provider(base: &str, model: &str) -> OllamaProvider {
        let mut cfg = Map::new();
        cfg.insert("baseUrl".into(), json!(base));
        cfg.insert("model".into(), json!(model));
        OllamaProvider::from_config(&cfg).unwrap()
    }

    fn req() -> ProviderRequest {
        ProviderRequest {
            text: "「おはよう」".into(),
            source_language: Some("ja".into()),
            target_language: "fr".into(),
            glossary: vec![],
        }
    }

    #[test]
    fn cloud_names() {
        assert!(is_cloud_model_name("glm-5.3:cloud"));
        assert!(is_cloud_model_name("gpt-oss:120b-cloud"));
        assert!(!is_cloud_model_name("qwen3:8b"));
        assert!(!is_cloud_model_name("cloudy"));
    }

    #[tokio::test]
    async fn chat_request_and_response() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/tags"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"models": [{"name": "qwen3:4b"}]})))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .and(body_partial_json(json!({"model": "qwen3:4b", "stream": false, "think": false})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "model": "qwen3:4b", "message": {"role": "assistant", "content": "<think></think>« Bonjour »"}, "done": true
            })))
            .expect(1)
            .mount(&server)
            .await;
        let p = provider(&server.uri(), "qwen3:4b");
        assert!(!p.sends_data_remotely());
        assert_eq!(p.translate(&req()).await.unwrap(), "« Bonjour »");
        let reqs = server.received_requests().await.unwrap();
        let chat = reqs.iter().find(|r| r.url.path() == "/api/chat").unwrap();
        let body: Value = serde_json::from_slice(&chat.body).unwrap();
        assert_eq!(body["messages"][0]["role"], "system");
        assert!(body["messages"][0]["content"].as_str().unwrap().contains("into French (fr)"));
        assert_eq!(body["messages"][1]["content"], "「おはよう」");
    }

    #[tokio::test]
    async fn retries_without_think_flag() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/tags"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"models": []})))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .and(body_partial_json(json!({"think": false})))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({"error": "\"m\" does not support thinking"})))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"message": {"content": "Salut"}})))
            .mount(&server)
            .await;
        assert_eq!(provider(&server.uri(), "m").translate(&req()).await.unwrap(), "Salut");
    }

    #[tokio::test]
    async fn refuses_cloud_models_by_default() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/tags"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"models": [
                {"name": "big:latest", "remote_host": "https://ollama.com", "remote_model": "big"}
            ]})))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;
        let err = provider(&server.uri(), "big").translate(&req()).await.unwrap_err();
        assert!(err.message.contains("cloud model"));
        let err = provider(&server.uri(), "x:cloud").translate(&req()).await.unwrap_err();
        assert!(err.message.contains("cloud model"));
        let mut cfg = Map::new();
        cfg.insert("baseUrl".into(), json!(server.uri()));
        cfg.insert("model".into(), json!("big"));
        let p = probe(&cfg).await;
        assert!(p.remote && !p.model_ok);
    }

    #[tokio::test]
    async fn http_errors_are_reported() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/tags"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"models": []})))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(404).set_body_json(json!({"error": "model 'nope' not found"})))
            .mount(&server)
            .await;
        let mut cfg = Map::new();
        cfg.insert("baseUrl".into(), json!(server.uri()));
        cfg.insert("model".into(), json!("nope"));
        cfg.insert("disableThinking".into(), json!(false));
        let err = OllamaProvider::from_config(&cfg).unwrap().translate(&req()).await.unwrap_err();
        assert_eq!(err.code, vnl_core::ErrorCode::Network);
        assert!(err.message.contains("not found"), "{}", err.message);
    }

    /// Real translation against the local Ollama install. Run with
    /// `VNL_OLLAMA_TEST_MODEL=qwen3:4b cargo test -p vnl-translate -- --ignored real_ollama`.
    #[tokio::test]
    #[ignore]
    async fn real_ollama_translation() {
        let model = std::env::var("VNL_OLLAMA_TEST_MODEL").unwrap_or_else(|_| "qwen3:4b".into());
        let p = provider(DEFAULT_BASE_URL, &model);
        let req = ProviderRequest {
            text: "「今日はいい天気ですね。散歩に行きましょう」".into(),
            source_language: Some("ja".into()),
            target_language: "en".into(),
            glossary: vec![],
        };
        let started = std::time::Instant::now();
        let out = p.translate(&req).await.expect("real Ollama translation");
        eprintln!("model={model} elapsed={:?} output={out:?}", started.elapsed());
        assert!(!out.is_empty());
        assert!(out.chars().any(|c| c.is_ascii_alphabetic()));
    }
}
