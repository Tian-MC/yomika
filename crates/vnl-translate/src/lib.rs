//! Translation subsystem: TextSource / HookAdapter / OcrAdapter /
//! ScreenCapture / TranslationProvider / TranslationPipeline / OverlayController.
//! Owned by Agent 4.
//!
//! CONTRACT (used by src-tauri/src/commands/session.rs — keep signatures stable,
//! you may add methods/fields freely).
//!
//! Modes (all independent): translation disabled (`enabled = false`),
//! extraction disabled, hook only, OCR only, hook + OCR, and manual text
//! translation ([`TranslationService::translate`]) which needs neither a game
//! nor extraction. There is no default target language anywhere: when none
//! is configured an `InvalidInput` error asks the user to choose one.
//! Privacy: text only goes to a provider the user selected; providers report
//! `sendsDataRemotely`, Ollama cloud models need an explicit opt-in, and OCR
//! never uses remote servers.

pub mod capture;
pub mod clipboard;
pub mod hook;
pub mod languages;
pub mod ocr;
pub mod overlay;
pub mod pipeline;
pub mod providers;
pub mod secrets;
pub mod session;
pub mod source;
pub mod store;
pub mod types;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use vnl_core::{
    AppError, AppPaths, EventSink, GameId, LogEntry, LogLevel, PrefixCommandBuilder, SessionId, TextOrigin,
    TranslationConfig, TranslationDefaults, TranslationLine,
};

use crate::capture::{FileCapture, PortalCapture, ScreenCapture, X11Capture, X11Target};
use crate::languages::{ResolvedLanguages, resolve_languages};
use crate::pipeline::{PipelineConfig, TranslationPipeline};
use crate::providers::ProviderRegistry;
use crate::secrets::SecretStore;
use crate::session::{ActiveSession, Cancellation, DefaultSourceFactory, InitializationDone, SourceFactory};
use crate::source::TextEvent;
use crate::store::TranslationStore;
use crate::types::{
    BackendInfo, GlossaryEntry, Language, OcrCaptureRequest, ProviderInfo, TranslateRequest, TranslationSessionStatus,
};

/// Context for attaching text extraction to a running game session.
pub struct SessionContext {
    pub session_id: SessionId,
    pub game_id: GameId,
    /// Already merged with app defaults via [`resolve_config`].
    pub config: TranslationConfig,
    pub game_exe: PathBuf,
    /// Builds commands that run inside the game's prefix (hook hosts...).
    pub prefix: Arc<dyn PrefixCommandBuilder>,
}

/// Fill per-game `None` fields from the app-level defaults.
pub fn resolve_config(game: &TranslationConfig, defaults: &TranslationDefaults) -> TranslationConfig {
    let mut cfg = game.clone();
    if cfg.source_language.is_none() {
        cfg.source_language = Some(defaults.source_language.clone());
    }
    if cfg.target_language.is_none() {
        cfg.target_language = defaults.target_language.clone();
    }
    if cfg.provider_id.is_none() {
        cfg.provider_id = defaults.provider_id.clone();
    }
    cfg
}

/// Optional dependency overrides (tests, portable installs).
#[derive(Default)]
pub struct ServiceOptions {
    pub secrets: Option<Arc<dyn SecretStore>>,
    pub sources: Option<Arc<dyn SourceFactory>>,
}

/// A built provider plus the languages to translate between.
type ResolvedTranslation = (Arc<dyn providers::TranslationProvider>, ResolvedLanguages);

pub const NO_PROVIDER_MESSAGE: &str =
    "No translation provider selected. Choose one in Settings → Translation (or in this game's translation settings).";

struct SessionEntry {
    cancelled: Arc<Cancellation>,
    initialized: tokio::sync::watch::Receiver<bool>,
    active: Option<ActiveSession>,
}

pub struct TranslationService {
    paths: AppPaths,
    events: Arc<dyn EventSink>,
    store: Arc<TranslationStore>,
    registry: Arc<ProviderRegistry>,
    sources: Arc<dyn SourceFactory>,
    // Entries survive stop so a delayed launch cannot resurrect a finished game.
    sessions: Mutex<HashMap<SessionId, SessionEntry>>,
    shutting_down: AtomicBool,
    /// Status of the last start attempt, kept after failures/stops.
    last_status: Mutex<HashMap<SessionId, TranslationSessionStatus>>,
}

impl TranslationService {
    pub fn new(paths: &AppPaths, events: Arc<dyn EventSink>) -> Result<Self, AppError> {
        Self::with_options(paths, events, ServiceOptions::default())
    }

    pub fn with_options(paths: &AppPaths, events: Arc<dyn EventSink>, opts: ServiceOptions) -> Result<Self, AppError> {
        let dir = paths.translation_dir();
        std::fs::create_dir_all(&dir)?;
        let store = Arc::new(TranslationStore::open(&dir.join("translation.sqlite3"))?);
        let secrets = opts.secrets.unwrap_or_else(|| secrets::default_secret_store(&paths.config_dir));
        let registry = Arc::new(ProviderRegistry::load(dir.join("providers.json"), secrets)?);
        let sources = opts.sources.unwrap_or_else(|| Arc::new(DefaultSourceFactory { tools_dir: paths.tools_dir() }));
        Ok(Self {
            paths: paths.clone(),
            events,
            store,
            registry,
            sources,
            sessions: Mutex::new(HashMap::new()),
            shutting_down: AtomicBool::new(false),
            last_status: Mutex::new(HashMap::new()),
        })
    }

    pub fn store(&self) -> &Arc<TranslationStore> {
        &self.store
    }

    pub fn registry(&self) -> &Arc<ProviderRegistry> {
        &self.registry
    }

    fn ollama_base_url(&self) -> String {
        self.registry
            .config(providers::ollama::ID)
            .map(|c| providers::cfg_str(&c, "baseUrl", providers::ollama::DEFAULT_BASE_URL))
            .unwrap_or_else(|_| providers::ollama::DEFAULT_BASE_URL.to_string())
    }

    fn log(&self, level: LogLevel, session: Option<&str>, msg: String) {
        let mut e = LogEntry::new(level, "translation", msg);
        if let Some(s) = session {
            e = e.session(s.to_string());
        }
        self.events.log(e);
    }

    /// Provider + languages for a config; `Ok(None)` = extraction only.
    fn resolve_translation(
        &self,
        provider_id: Option<&str>,
        source: Option<&str>,
        target: Option<&str>,
        game: Option<&TranslationConfig>,
        defaults: Option<&TranslationDefaults>,
    ) -> Result<Option<ResolvedTranslation>, AppError> {
        let provider_id = provider_id
            .or_else(|| game.and_then(|g| g.provider_id.as_deref()))
            .or_else(|| defaults.and_then(|d| d.provider_id.as_deref()))
            .map(str::trim)
            .filter(|p| !p.is_empty());
        let Some(provider_id) = provider_id else { return Ok(None) };
        let langs = resolve_languages(source, target, game, defaults)?;
        let provider = self.registry.build(provider_id)?;
        Ok(Some((provider, langs)))
    }

    // ------------------------------------------------------------ sessions

    /// Start hook/OCR sources for a session. Called only when
    /// `config.enabled && config.extraction != Disabled`. Failures are
    /// reported to the user but never abort the game session.
    pub async fn start_session(&self, ctx: SessionContext) -> Result<(), AppError> {
        let session_id = ctx.session_id.clone();
        let (done_tx, initialized) = tokio::sync::watch::channel(false);
        let _done = InitializationDone(done_tx);
        let cancelled = {
            let mut sessions = self.sessions.lock().unwrap();
            if self.shutting_down.load(Ordering::SeqCst) {
                return Err(AppError::invalid("The launcher is shutting down"));
            }
            if let Some(entry) = sessions.get(&session_id) {
                return if entry.cancelled.is_cancelled() {
                    Ok(())
                } else {
                    Err(AppError::invalid("Text extraction already started for this session"))
                };
            }
            let cancelled = Arc::new(Cancellation::new(false));
            sessions
                .insert(session_id.clone(), SessionEntry { cancelled: cancelled.clone(), initialized, active: None });
            cancelled
        };
        let mut status = TranslationSessionStatus::inactive(&session_id);
        if !ctx.config.enabled || ctx.config.extraction == vnl_core::ExtractionMode::Disabled {
            status.diagnostics.push(if ctx.config.enabled {
                "Text extraction is disabled for this game.".into()
            } else {
                "Translation is disabled for this game.".into()
            });
            self.last_status.lock().unwrap().insert(session_id, status);
            return Ok(());
        }
        let mut diagnostics = Vec::new();
        let translation = match self.resolve_translation(None, None, None, Some(&ctx.config), None) {
            Ok(t) => t,
            Err(e) => {
                status.diagnostics.push(e.message.clone());
                self.last_status.lock().unwrap().insert(session_id, status);
                return Err(e);
            }
        };
        match &translation {
            None => diagnostics.push("No translation provider selected: showing extracted text only.".into()),
            Some((p, l)) => diagnostics.push(format!(
                "Translating {} → {} with {}{}.",
                l.source.as_deref().unwrap_or("auto"),
                l.target,
                p.id(),
                if p.sends_data_remotely() { " (online: text is sent to a remote server)" } else { " (local)" }
            )),
        }
        let glossary = self.store.effective_glossary(Some(ctx.game_id))?;
        let (provider, languages) = match translation {
            Some((p, l)) => (Some(p), Some(l)),
            None => (None, None),
        };
        let pipeline = Arc::new(TranslationPipeline::new(
            PipelineConfig {
                session_id: Some(session_id.clone()),
                game_id: Some(ctx.game_id),
                provider,
                languages,
                source_language: ctx.config.source_language.clone(),
                glossary,
                keep_history: ctx.config.keep_history,
                dedupe: true,
            },
            self.events.clone(),
            Some(self.store.clone()),
        ));
        let (active, result) = session::start_sources(
            &ctx,
            self.sources.as_ref(),
            pipeline,
            &self.ollama_base_url(),
            diagnostics,
            cancelled.clone(),
        )
        .await;
        let st = active.status(&session_id);
        for d in &st.diagnostics {
            self.log(LogLevel::Info, Some(&session_id), d.clone());
        }
        // Registration and cancellation share one short critical section. Never
        // hold it while stopping sources or awaiting their initialization.
        let mut active = Some(active);
        {
            let mut sessions = self.sessions.lock().unwrap();
            if !cancelled.is_cancelled() && result.is_ok() {
                sessions.get_mut(&session_id).unwrap().active = active.take();
                self.last_status.lock().unwrap().insert(session_id.clone(), st.clone());
            }
        }
        if let Some(active) = active {
            active.stop().await;
            let mut stopped = st;
            for state in [&mut stopped.hook, &mut stopped.ocr] {
                if *state != types::TextSourceState::Failed {
                    *state = types::TextSourceState::Inactive;
                }
            }
            self.last_status.lock().unwrap().insert(session_id, stopped);
        }
        result
    }

    pub async fn shutdown(&self) {
        let ids = {
            let sessions = self.sessions.lock().unwrap();
            self.shutting_down.store(true, Ordering::SeqCst);
            for entry in sessions.values() {
                entry.cancelled.cancel();
            }
            sessions.keys().cloned().collect::<Vec<_>>()
        };
        for id in ids {
            self.stop_session(&id).await;
        }
    }

    pub async fn stop_session(&self, session_id: &str) {
        let (active, mut initialized) = {
            let mut sessions = self.sessions.lock().unwrap();
            let entry = sessions.entry(session_id.to_string()).or_insert_with(|| SessionEntry {
                cancelled: Arc::new(Cancellation::new(true)),
                initialized: tokio::sync::watch::channel(true).1,
                active: None,
            });
            entry.cancelled.cancel();
            (entry.active.take(), entry.initialized.clone())
        };
        if let Some(active) = active {
            let mut st = active.status(session_id);
            active.stop().await;
            for s in [&mut st.hook, &mut st.ocr] {
                if *s != types::TextSourceState::Failed {
                    *s = types::TextSourceState::Inactive;
                }
            }
            st.diagnostics.push("Session stopped.".into());
            self.last_status.lock().unwrap().insert(session_id.to_string(), st);
        }
        // Pending starts own their partial resources. Cancellation wakes them;
        // wait for their cleanup, without holding the registry lock.
        let _ = initialized.wait_for(|done| *done).await;
    }

    pub async fn session_status(&self, session_id: &str) -> TranslationSessionStatus {
        {
            let sessions = self.sessions.lock().unwrap();
            if let Some(active) = sessions.get(session_id).and_then(|entry| entry.active.as_ref()) {
                return active.status(session_id);
            }
        }
        self.last_status
            .lock()
            .unwrap()
            .get(session_id)
            .cloned()
            .unwrap_or_else(|| TranslationSessionStatus::inactive(session_id))
    }

    // ----------------------------------------------------------- providers

    pub async fn providers(&self) -> Vec<ProviderInfo> {
        self.registry.list().await
    }

    pub async fn configure_provider(
        &self,
        provider_id: &str,
        config: serde_json::Map<String, serde_json::Value>,
        api_key: Option<Option<String>>,
    ) -> Result<ProviderInfo, AppError> {
        let registry = self.registry.clone();
        let id = provider_id.to_string();
        // Keyring calls block; keep them off the async worker.
        tokio::task::spawn_blocking(move || registry.configure(&id, config, api_key))
            .await
            .map_err(|e| AppError::internal(e.to_string()))??;
        self.registry.info(provider_id).await
    }

    /// Translates a fixed Japanese sample with the given provider.
    pub async fn test_provider(&self, provider_id: &str, target_language: &str) -> Result<TranslationLine, AppError> {
        let req = TranslateRequest {
            text: "今日はいい天気ですね。".into(),
            source_language: Some("ja".into()),
            target_language: Some(target_language.to_string()),
            provider_id: Some(provider_id.to_string()),
            game_id: None,
            session_id: None,
        };
        self.translate_inner(req, None, None, false, false).await
    }

    pub fn languages(&self) -> Vec<Language> {
        languages::languages()
    }

    /// Manual translation; works without a game and with extraction disabled.
    /// Config errors (no provider / no target) are `Err(InvalidInput)`;
    /// provider failures come back as `line.error`.
    pub async fn translate(
        &self,
        req: TranslateRequest,
        game: Option<&TranslationConfig>,
        defaults: Option<&TranslationDefaults>,
    ) -> Result<TranslationLine, AppError> {
        let keep = game.map(|g| g.keep_history).unwrap_or(true);
        self.translate_inner(req, game, defaults, keep, true).await
    }

    async fn translate_inner(
        &self,
        req: TranslateRequest,
        game: Option<&TranslationConfig>,
        defaults: Option<&TranslationDefaults>,
        keep_history: bool,
        emit: bool,
    ) -> Result<TranslationLine, AppError> {
        if req.text.trim().is_empty() {
            return Err(AppError::invalid("Nothing to translate: the text is empty"));
        }
        let (provider, langs) = self
            .resolve_translation(
                req.provider_id.as_deref(),
                req.source_language.as_deref(),
                req.target_language.as_deref(),
                game,
                defaults,
            )?
            .ok_or_else(|| AppError::invalid(NO_PROVIDER_MESSAGE))?;
        let glossary = self.store.effective_glossary(req.game_id)?;
        let sink: Arc<dyn EventSink> = if emit { self.events.clone() } else { Arc::new(vnl_core::NullSink) };
        let pipeline = TranslationPipeline::new(
            PipelineConfig {
                session_id: req.session_id.clone(),
                game_id: req.game_id,
                provider: Some(provider),
                languages: Some(langs),
                source_language: None,
                glossary,
                keep_history,
                dedupe: false,
            },
            sink,
            Some(self.store.clone()),
        );
        pipeline
            .process(TextEvent::new(TextOrigin::Manual, req.text))
            .await
            .ok_or_else(|| AppError::invalid("Nothing to translate after normalization"))
    }

    // ------------------------------------------------------ glossary/history

    pub fn glossary_list(&self, game_id: Option<GameId>) -> Result<Vec<GlossaryEntry>, AppError> {
        self.store.glossary_list(game_id)
    }

    pub fn glossary_upsert(&self, entry: &GlossaryEntry) -> Result<GlossaryEntry, AppError> {
        self.store.glossary_upsert(entry)
    }

    pub fn glossary_delete(&self, id: i64) -> Result<(), AppError> {
        self.store.glossary_delete(id)
    }

    pub fn history(&self, game_id: Option<GameId>, limit: Option<u32>) -> Result<Vec<TranslationLine>, AppError> {
        self.store.history_list(game_id, limit.unwrap_or(200).min(5000))
    }

    pub fn history_clear(&self, game_id: Option<GameId>) -> Result<(), AppError> {
        self.store.history_clear(game_id).map(|_| ())
    }

    // ---------------------------------------------------------- OCR / hooks

    /// OCR backends followed by screen-capture methods (ids `capture:*`).
    pub async fn ocr_backends(&self) -> Vec<BackendInfo> {
        let mut out = ocr::ocr_backends(&self.ollama_base_url()).await;
        out.extend(capture::capture_backends());
        out
    }

    pub fn hook_backends(&self) -> Vec<BackendInfo> {
        hook::hook_backends(&self.paths.tools_dir())
    }

    /// Capture → OCR → (optionally) translate.
    /// `game_exe`: used to find the game's X11 window when no window id is given.
    pub async fn ocr_capture(
        &self,
        req: OcrCaptureRequest,
        game: Option<&TranslationConfig>,
        defaults: Option<&TranslationDefaults>,
        game_exe: Option<&std::path::Path>,
    ) -> Result<TranslationLine, AppError> {
        // Validate translation settings before doing expensive work.
        let translation = if req.translate {
            Some(
                self.resolve_translation(None, None, None, game, defaults)?
                    .ok_or_else(|| AppError::invalid(NO_PROVIDER_MESSAGE))?,
            )
        } else {
            None
        };
        let region = req.region.or_else(|| game.and_then(|g| g.ocr.region));
        let capture: Box<dyn ScreenCapture> = match (&req.image_path, req.capture.as_deref()) {
            (Some(p), _) => Box::new(FileCapture { path: p.clone() }),
            (None, Some("portal")) => {
                Box::new(PortalCapture { interactive: true, timeout: std::time::Duration::from_secs(120) })
            }
            (None, Some("x11") | None) => {
                let target = match (&req.window_id, game_exe.and_then(|p| p.file_name())) {
                    (Some(id), _) => X11Target::WindowId(id.clone()),
                    (None, Some(name)) => X11Target::WindowClass(name.to_string_lossy().into_owned()),
                    (None, None) => X11Target::Root,
                };
                Box::new(X11Capture::detect(req.display.as_deref())?.with_target(target))
            }
            (None, Some(other)) => return Err(AppError::invalid(format!("unknown capture method '{other}'"))),
        };
        let backend = req.backend.clone().or_else(|| game.and_then(|g| g.ocr.backend.clone()));
        let adapter = ocr::build_adapter(backend.as_deref(), &self.ollama_base_url()).await?;
        let image = capture.capture(region).await?;
        let source_lang =
            game.and_then(|g| g.source_language.clone()).or_else(|| defaults.map(|d| d.source_language.clone()));
        let text = adapter.recognize(&image, source_lang.as_deref()).await?;
        if text.trim().is_empty() {
            return Err(AppError::not_found("No text was recognised in the captured area"));
        }
        let (provider, languages) = match translation {
            Some((p, l)) => (Some(p), Some(l)),
            None => (None, None),
        };
        let pipeline = TranslationPipeline::new(
            PipelineConfig {
                session_id: req.session_id.clone(),
                game_id: req.game_id,
                provider,
                languages,
                source_language: source_lang,
                glossary: self.store.effective_glossary(req.game_id)?,
                keep_history: game.map(|g| g.keep_history).unwrap_or(true),
                dedupe: false,
            },
            self.events.clone(),
            Some(self.store.clone()),
        );
        pipeline
            .process(TextEvent { origin: TextOrigin::Ocr, text, detail: Some(adapter.id()) })
            .await
            .ok_or_else(|| AppError::not_found("No usable text was recognised"))
    }
}

#[cfg(test)]
mod tests;
