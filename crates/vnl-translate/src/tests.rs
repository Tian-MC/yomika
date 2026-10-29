//! Service-level tests: every mode combination, manual translation, status.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Map, json};
use vnl_core::events::RecordingSink;
use vnl_core::{AppEvent, CommandSpec, ErrorCode, ExtractionMode};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;
use crate::secrets::MemorySecretStore;
use crate::source::{SourceStatus, StatusCell, TextSender, TextSource};
use crate::types::TextSourceState;

struct NoPrefix;
impl PrefixCommandBuilder for NoPrefix {
    fn build(&self, exe: &Path, args: &[String], _cwd: Option<&Path>) -> Result<CommandSpec, AppError> {
        Ok(CommandSpec::new(exe).args(args.to_vec()))
    }
    fn prefix_path(&self) -> PathBuf {
        PathBuf::from("/nonexistent")
    }
}

/// Emits one line on start (or fails), records stop.
struct ScriptedSource {
    origin: TextOrigin,
    line: String,
    fail: bool,
    status: StatusCell,
    stopped: Arc<AtomicBool>,
}

#[async_trait]
impl TextSource for ScriptedSource {
    fn id(&self) -> &str {
        "scripted"
    }
    fn origin(&self) -> TextOrigin {
        self.origin
    }
    async fn start(&self, tx: TextSender) -> Result<(), AppError> {
        if self.fail {
            self.status.fail("scripted failure");
            return Err(AppError::new(ErrorCode::MissingDependency, "scripted failure"));
        }
        self.status.set_state(TextSourceState::Active);
        tx.send(TextEvent::new(self.origin, self.line.clone())).await.ok();
        Ok(())
    }
    async fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        self.status.set_state(TextSourceState::Inactive);
    }
    fn status(&self) -> SourceStatus {
        self.status.get()
    }
}

#[derive(Default)]
struct Factory {
    hook_fail: bool,
    ocr_fail: bool,
    hook_calls: AtomicUsize,
    ocr_calls: AtomicUsize,
    stopped: Arc<AtomicBool>,
}

#[async_trait]
impl SourceFactory for Factory {
    fn hook(&self, _ctx: &SessionContext) -> Result<Arc<dyn TextSource>, AppError> {
        self.hook_calls.fetch_add(1, Ordering::SeqCst);
        Ok(Arc::new(ScriptedSource {
            origin: TextOrigin::Hook,
            line: "フックの台詞".into(),
            fail: self.hook_fail,
            status: StatusCell::default(),
            stopped: self.stopped.clone(),
        }))
    }
    async fn ocr(&self, _c: &SessionContext, _u: &str) -> Result<Arc<dyn TextSource>, AppError> {
        self.ocr_calls.fetch_add(1, Ordering::SeqCst);
        if self.ocr_fail {
            return Err(AppError::new(ErrorCode::MissingDependency, "no OCR backend"));
        }
        Ok(Arc::new(ScriptedSource {
            origin: TextOrigin::Ocr,
            line: "OCRの台詞".into(),
            fail: false,
            status: StatusCell::default(),
            stopped: self.stopped.clone(),
        }))
    }
}

struct Env {
    _tmp: tempfile::TempDir,
    sink: RecordingSink,
    svc: TranslationService,
    factory: Arc<Factory>,
}

fn env(factory: Factory) -> Env {
    let tmp = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(tmp.path());
    paths.ensure().unwrap();
    let sink = RecordingSink::new();
    let factory = Arc::new(factory);
    let svc = TranslationService::with_options(
        &paths,
        Arc::new(sink.clone()),
        ServiceOptions { secrets: Some(Arc::new(MemorySecretStore::default())), sources: Some(factory.clone()) },
    )
    .unwrap();
    Env { _tmp: tmp, sink, svc, factory }
}

fn ctx(mode: ExtractionMode, enabled: bool) -> SessionContext {
    SessionContext {
        session_id: "s1".into(),
        game_id: 42,
        config: TranslationConfig {
            enabled,
            extraction: mode,
            source_language: Some("ja".into()),
            ..Default::default()
        },
        game_exe: PathBuf::from("/games/x/Game.exe"),
        prefix: Arc::new(NoPrefix),
    }
}

fn lines(sink: &RecordingSink) -> Vec<TranslationLine> {
    sink.events().into_iter().filter_map(|e| if let AppEvent::Translation(l) = e { Some(l) } else { None }).collect()
}

async fn wait_lines(sink: &RecordingSink, n: usize) -> Vec<TranslationLine> {
    for _ in 0..100 {
        let l = lines(sink);
        if l.len() >= n {
            return l;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    lines(sink)
}

/// Configures the LibreTranslate provider against a mock server.
async fn mock_provider(svc: &TranslationService) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/translate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"translatedText": "translated"})))
        .mount(&server)
        .await;
    let mut cfg = Map::new();
    cfg.insert("url".into(), json!(server.uri()));
    svc.configure_provider("libretranslate", cfg, None).await.unwrap();
    server
}

#[tokio::test]
async fn translation_disabled_starts_nothing() {
    let e = env(Factory::default());
    e.svc.start_session(ctx(ExtractionMode::HookAndOcr, false)).await.unwrap();
    assert_eq!(e.factory.hook_calls.load(Ordering::SeqCst) + e.factory.ocr_calls.load(Ordering::SeqCst), 0);
    let st = e.svc.session_status("s1").await;
    assert_eq!((st.hook, st.ocr), (TextSourceState::Inactive, TextSourceState::Inactive));
    assert!(st.diagnostics[0].contains("disabled"));
}

#[tokio::test]
async fn extraction_disabled_starts_nothing() {
    let e = env(Factory::default());
    e.svc.start_session(ctx(ExtractionMode::Disabled, true)).await.unwrap();
    assert_eq!(e.factory.hook_calls.load(Ordering::SeqCst) + e.factory.ocr_calls.load(Ordering::SeqCst), 0);
    assert!(lines(&e.sink).is_empty());
}

#[tokio::test]
async fn hook_only_extraction_without_provider() {
    let e = env(Factory::default());
    e.svc.start_session(ctx(ExtractionMode::Hook, true)).await.unwrap();
    let l = wait_lines(&e.sink, 1).await;
    assert_eq!(l[0].origin, TextOrigin::Hook);
    assert_eq!(l[0].source_text, "フックの台詞");
    assert!(l[0].translated_text.is_none());
    let st = e.svc.session_status("s1").await;
    assert_eq!((st.hook, st.ocr), (TextSourceState::Active, TextSourceState::Inactive));
    assert_eq!(e.factory.ocr_calls.load(Ordering::SeqCst), 0);
    e.svc.stop_session("s1").await;
    assert!(e.factory.stopped.load(Ordering::SeqCst));
    assert_eq!(e.svc.session_status("s1").await.hook, TextSourceState::Inactive);
}

#[tokio::test]
async fn ocr_only_with_translation() {
    let e = env(Factory::default());
    let _server = mock_provider(&e.svc).await;
    let mut c = ctx(ExtractionMode::Ocr, true);
    c.config.provider_id = Some("libretranslate".into());
    c.config.target_language = Some("en".into());
    e.svc.start_session(c).await.unwrap();
    let l = wait_lines(&e.sink, 2).await;
    assert_eq!(l.len(), 2);
    assert_eq!(l[0].origin, TextOrigin::Ocr);
    assert_eq!(l[1].translated_text.as_deref(), Some("translated"));
    assert_eq!(e.factory.hook_calls.load(Ordering::SeqCst), 0);
    // History is kept per game.
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(e.svc.history(Some(42), None).unwrap().len(), 1);
    e.svc.stop_session("s1").await;
}

#[tokio::test]
async fn hook_and_ocr_partial_failure_keeps_other_source() {
    let e = env(Factory { ocr_fail: true, ..Default::default() });
    e.svc.start_session(ctx(ExtractionMode::HookAndOcr, true)).await.unwrap();
    let st = e.svc.session_status("s1").await;
    assert_eq!(st.hook, TextSourceState::Active);
    assert_eq!(st.ocr, TextSourceState::Failed);
    assert!(st.diagnostics.iter().any(|d| d.contains("[ocr] no OCR backend")), "{:?}", st.diagnostics);
    assert_eq!(wait_lines(&e.sink, 1).await.len(), 1);
    e.svc.stop_session("s1").await;
}

#[tokio::test]
async fn all_sources_failing_returns_error_with_status() {
    let e = env(Factory { ocr_fail: true, hook_fail: true, ..Default::default() });
    let err = e.svc.start_session(ctx(ExtractionMode::HookAndOcr, true)).await.unwrap_err();
    assert!(err.message.contains("scripted failure") && err.message.contains("no OCR backend"), "{}", err.message);
    let st = e.svc.session_status("s1").await;
    assert_eq!((st.hook, st.ocr), (TextSourceState::Failed, TextSourceState::Failed));
    e.svc.stop_session("s1").await;
}

#[tokio::test]
async fn provider_without_target_is_invalid_input() {
    let e = env(Factory::default());
    let mut c = ctx(ExtractionMode::Hook, true);
    c.config.provider_id = Some("libretranslate".into());
    let err = e.svc.start_session(c).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidInput);
    assert!(err.message.contains("target language"));
    assert_eq!(e.factory.hook_calls.load(Ordering::SeqCst), 0);
    assert!(e.svc.session_status("s1").await.diagnostics[0].contains("target language"));
}

#[tokio::test]
async fn manual_translation_without_game_or_extraction() {
    let e = env(Factory::default());
    // No provider at all.
    let err =
        e.svc.translate(TranslateRequest { text: "猫".into(), ..Default::default() }, None, None).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidInput);
    let _server = mock_provider(&e.svc).await;
    // Provider but no target.
    let req = TranslateRequest { text: "猫".into(), provider_id: Some("libretranslate".into()), ..Default::default() };
    let err = e.svc.translate(req.clone(), None, Some(&TranslationDefaults::default())).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidInput);
    // Target from app defaults.
    let defaults = TranslationDefaults { target_language: Some("it".into()), ..Default::default() };
    let line = e.svc.translate(req, None, Some(&defaults)).await.unwrap();
    assert_eq!(line.origin, TextOrigin::Manual);
    assert_eq!(line.target_language.as_deref(), Some("it"));
    assert_eq!(line.source_language.as_deref(), Some("ja"));
    assert_eq!(line.translated_text.as_deref(), Some("translated"));
    assert_eq!(lines(&e.sink).len(), 2);
    // Test-provider is not persisted in history and not emitted.
    let before = e.svc.history(None, None).unwrap().len();
    let t = e.svc.test_provider("libretranslate", "fr").await.unwrap();
    assert_eq!(t.translated_text.as_deref(), Some("translated"));
    assert_eq!(e.svc.history(None, None).unwrap().len(), before);
}

#[tokio::test]
async fn provider_failure_is_reported_in_line() {
    let e = env(Factory::default());
    let mut cfg = Map::new();
    cfg.insert("url".into(), json!("http://127.0.0.1:9"));
    cfg.insert("timeoutSecs".into(), json!(2));
    e.svc.configure_provider("libretranslate", cfg, None).await.unwrap();
    let req = TranslateRequest {
        text: "猫".into(),
        provider_id: Some("libretranslate".into()),
        target_language: Some("en".into()),
        ..Default::default()
    };
    let line = e.svc.translate(req, None, None).await.unwrap();
    assert!(line.translated_text.is_none());
    assert!(line.error.unwrap().contains("LibreTranslate"));
}

#[tokio::test]
async fn glossary_applies_to_manual_translation() {
    let e = env(Factory::default());
    let server = mock_provider(&e.svc).await;
    e.svc
        .glossary_upsert(&GlossaryEntry {
            id: 0,
            game_id: Some(5),
            kind: types::GlossaryKind::CharacterName,
            source: "美咲".into(),
            target: "Misaki".into(),
            note: None,
            enabled: true,
        })
        .unwrap();
    let req = TranslateRequest {
        text: "美咲です".into(),
        provider_id: Some("libretranslate".into()),
        target_language: Some("en".into()),
        game_id: Some(5),
        ..Default::default()
    };
    e.svc.translate(req, None, None).await.unwrap();
    let r = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&r.last().unwrap().body).unwrap();
    assert_eq!(body["q"], "ZX0Zです", "character name replaced by placeholder before MT");
}

#[tokio::test]
async fn ocr_capture_from_file_requires_target_when_translating() {
    let e = env(Factory::default());
    let req = OcrCaptureRequest { image_path: Some(PathBuf::from("/nonexistent.png")), ..Default::default() };
    let err = e.svc.ocr_capture(req, None, None, None).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidInput, "{err:?}");
}

#[tokio::test]
async fn backends_listing() {
    let e = env(Factory::default());
    let hooks = e.svc.hook_backends();
    assert_eq!(hooks[0].id, "lunahost");
    assert!(!hooks[0].available, "no CLI in the temp tools dir");
    assert!(hooks.iter().any(|b| b.id == "clipboard"));
    let ocr = e.svc.ocr_backends().await;
    assert!(ocr.iter().any(|b| b.id == "tesseract"));
    assert!(ocr.iter().any(|b| b.id == "capture:file" && b.available));
    let providers = e.svc.providers().await;
    assert_eq!(providers.len(), 4);
    assert!(providers.iter().find(|p| p.id == "deepl").unwrap().sends_data_remotely);
}
