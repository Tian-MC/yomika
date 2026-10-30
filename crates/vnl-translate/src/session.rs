//! Per-game-session extraction: starts the configured sources, forwards their
//! events into a pipeline and tracks status.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use vnl_core::{AppError, ErrorCode, ExtractionMode};

use crate::SessionContext;
use crate::capture::{X11Capture, X11Target};
use crate::clipboard::ClipboardSource;
use crate::hook::lunahost::{LunaHostAdapter, LunaHostOptions};
use crate::ocr::source::OcrTextSource;
use crate::pipeline::TranslationPipeline;
use crate::source::TextSource;
use crate::types::{TextSourceState, TranslationSessionStatus};

// Watch retains cancellation even when stop precedes start and wakes pending
// initialization without polling. Clones all observe the same irreversible flag.
pub(crate) struct Cancellation(tokio::sync::watch::Sender<bool>);

impl Cancellation {
    pub fn new(cancelled: bool) -> Self {
        Self(tokio::sync::watch::channel(cancelled).0)
    }
    pub fn cancel(&self) {
        self.0.send_replace(true);
    }
    pub fn is_cancelled(&self) -> bool {
        *self.0.borrow()
    }
    pub async fn cancelled(&self) {
        let mut rx = self.0.subscribe();
        let _ = rx.wait_for(|value| *value).await;
    }
}

// Signal completion on every return path, including initialization errors.
pub(crate) struct InitializationDone(pub tokio::sync::watch::Sender<bool>);
impl Drop for InitializationDone {
    fn drop(&mut self) {
        self.0.send_replace(true);
    }
}

/// Creates text sources; replaceable in tests.
#[async_trait]
pub trait SourceFactory: Send + Sync {
    fn hook(&self, ctx: &SessionContext) -> Result<Arc<dyn TextSource>, AppError>;
    async fn ocr(&self, ctx: &SessionContext, ollama_base_url: &str) -> Result<Arc<dyn TextSource>, AppError>;
}

pub struct DefaultSourceFactory {
    pub tools_dir: std::path::PathBuf,
}

#[async_trait]
impl SourceFactory for DefaultSourceFactory {
    fn hook(&self, ctx: &SessionContext) -> Result<Arc<dyn TextSource>, AppError> {
        match ctx.config.hook.backend.as_deref().map(str::trim).filter(|b| !b.is_empty()) {
            None | Some(crate::hook::lunahost::BACKEND_ID) => Ok(Arc::new(
                LunaHostAdapter::new(
                    self.tools_dir.clone(),
                    ctx.game_exe.clone(),
                    ctx.prefix.clone(),
                    ctx.config.hook.hook_code.clone(),
                    LunaHostOptions {
                        attach_delay: Duration::from_millis(ctx.config.hook.attach_delay_ms),
                        ..Default::default()
                    },
                )
                .with_hook_codes(ctx.config.hook.effective_hook_codes())
                .with_thread_selections(ctx.config.hook.selected_threads())
                .with_filters(ctx.config.hook.filters.clone())
                .with_preferred(ctx.config.hook.preferred.clone())
                .with_auto_select(ctx.config.hook.thread.is_none() && ctx.config.hook.preferred.is_none())
                .with_codepage(ctx.config.hook.extraction.codepage)
                .with_flush_delay_ms(ctx.config.hook.extraction.flush_delay_ms)
                .with_extraction(ctx.config.hook.extraction.clone()),
            )),
            Some(crate::clipboard::BACKEND_ID) => Ok(Arc::new(ClipboardSource::new(Duration::from_millis(400)))),
            Some(other) => {
                Err(AppError::invalid(format!("unknown hook backend '{other}' (use 'lunahost' or 'clipboard')")))
            }
        }
    }

    async fn ocr(&self, ctx: &SessionContext, ollama_base_url: &str) -> Result<Arc<dyn TextSource>, AppError> {
        let config = &ctx.config;
        // Capture the game's own window (Wine WM_CLASS = exe name); root
        // capture is black under GNOME's rootless XWayland.
        let target = match ctx.game_exe.file_name() {
            Some(n) => X11Target::WindowClass(n.to_string_lossy().into_owned()),
            None => X11Target::Root,
        };
        let capture = Arc::new(X11Capture::detect(None)?.with_target(target));
        let ocr = crate::ocr::build_adapter(config.ocr.backend.as_deref(), ollama_base_url).await?;
        Ok(Arc::new(OcrTextSource::new(
            capture,
            ocr,
            config.ocr.region,
            Duration::from_millis(config.ocr.interval_ms),
            config.source_language.clone(),
        )))
    }
}

pub(crate) struct ActiveSession {
    pub hook: Option<Arc<dyn TextSource>>,
    pub ocr: Option<Arc<dyn TextSource>>,
    pub hook_error: Option<String>,
    pub ocr_error: Option<String>,
    pub diagnostics: Vec<String>,
    pub forward: tokio::task::JoinHandle<()>,
}

impl ActiveSession {
    pub async fn stop(self) {
        self.forward.abort();
        let _ = self.forward.await;
        if let Some(h) = &self.hook {
            h.stop().await;
        }
        if let Some(o) = &self.ocr {
            o.stop().await;
        }
    }

    pub fn status(&self, session_id: &str) -> TranslationSessionStatus {
        let mut st = TranslationSessionStatus::inactive(session_id);
        st.diagnostics = self.diagnostics.clone();
        if let Some(hook) = &self.hook {
            let status = hook.status();
            st.hook_threads = status.hook_threads;
            st.selected_hook_thread = status.selected_hook_thread;
        }
        for (src, err, slot, label) in
            [(&self.hook, &self.hook_error, &mut st.hook, "hook"), (&self.ocr, &self.ocr_error, &mut st.ocr, "ocr")]
        {
            match (src, err) {
                (Some(s), _) => {
                    let s = s.status();
                    *slot = s.state;
                    st.diagnostics.extend(s.diagnostics.into_iter().map(|d| format!("[{label}] {d}")));
                }
                (None, Some(e)) => {
                    *slot = TextSourceState::Failed;
                    st.diagnostics.push(format!("[{label}] {e}"));
                }
                (None, None) => {}
            }
        }
        st
    }
}

pub(crate) fn wants_hook(mode: ExtractionMode) -> bool {
    matches!(mode, ExtractionMode::Hook | ExtractionMode::HookAndOcr)
}

pub(crate) fn wants_ocr(mode: ExtractionMode) -> bool {
    matches!(mode, ExtractionMode::Ocr | ExtractionMode::HookAndOcr)
}

/// Starts sources and the forwarding task. Errors of individual sources are
/// recorded; an error is returned only if every requested source failed.
pub(crate) async fn start_sources(
    ctx: &SessionContext,
    factory: &dyn SourceFactory,
    pipeline: Arc<TranslationPipeline>,
    ollama_base_url: &str,
    mut diagnostics: Vec<String>,
    cancelled: Arc<Cancellation>,
) -> (ActiveSession, Result<(), AppError>) {
    let (tx, mut rx) = tokio::sync::mpsc::channel(256);
    let mode = ctx.config.extraction;
    let (mut hook, mut ocr, mut hook_error, mut ocr_error) = (None, None, None, None);

    if wants_hook(mode) && !cancelled.is_cancelled() {
        match factory.hook(ctx) {
            Ok(src) => {
                let started = tokio::select! {
                    biased;
                    _ = cancelled.cancelled() => None,
                    result = src.start(tx.clone()) => Some(result),
                };
                match started {
                    Some(Ok(())) => hook = Some(src),
                    result => {
                        hook_error = result.and_then(Result::err).map(|e| e.message);
                        src.stop().await;
                    }
                }
            }
            Err(e) => hook_error = Some(e.message),
        }
    }
    if cancelled.is_cancelled()
        && let Some(src) = hook.take()
    {
        src.stop().await;
    }
    if wants_ocr(mode) && !cancelled.is_cancelled() {
        let built = tokio::select! {
            biased;
            _ = cancelled.cancelled() => None,
            result = factory.ocr(ctx, ollama_base_url) => Some(result),
        };
        match built {
            Some(Ok(src)) => {
                if cancelled.is_cancelled() {
                    src.stop().await;
                } else {
                    let started = tokio::select! {
                        biased;
                        _ = cancelled.cancelled() => None,
                        result = src.start(tx.clone()) => Some(result),
                    };
                    match started {
                        Some(Ok(())) => ocr = Some(src),
                        result => {
                            ocr_error = result.and_then(Result::err).map(|e| e.message);
                            src.stop().await;
                        }
                    }
                }
            }
            Some(Err(e)) => {
                ocr_error = Some(format!("{}{}", e.message, e.details.map(|d| format!(" — {d}")).unwrap_or_default()))
            }
            None => {}
        }
    }
    drop(tx);

    if cancelled.is_cancelled() {
        for src in [hook.take(), ocr.take()].into_iter().flatten() {
            src.stop().await;
        }
    }
    let hook_source = hook.clone();
    let forward = tokio::spawn(async move {
        // Own provider tasks so stopping extraction also cancels pending work.
        let mut translations = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                biased;
                _ = cancelled.cancelled() => break,
                ev = rx.recv() => {
                    let Some(ev) = ev else { break };
                    if cancelled.is_cancelled() { break; }
                    if ev.origin == vnl_core::TextOrigin::Hook && hook_source.as_ref().is_some_and(|h| !h.accepts(&ev)) { continue; }
                    if let Some(line) = pipeline.accept(&ev) {
                        let p = pipeline.clone();
                        let source = hook_source.clone();
                        translations.spawn(async move {
                            p.complete_if(line, || {
                                ev.origin != vnl_core::TextOrigin::Hook
                                    || source.as_ref().is_none_or(|h| h.accepts(&ev))
                            }).await;
                        });
                    }
                }
                Some(_) = translations.join_next(), if !translations.is_empty() => {}
            }
        }
        while translations.join_next().await.is_some() {}
    });

    let requested = wants_hook(mode) as usize + wants_ocr(mode) as usize;
    let failed = hook_error.is_some() as usize + ocr_error.is_some() as usize;
    let result = if requested > 0 && failed == requested {
        let msg = [hook_error.as_deref(), ocr_error.as_deref()].into_iter().flatten().collect::<Vec<_>>().join("; ");
        diagnostics.push("No text source could be started.".into());
        Err(AppError::new(ErrorCode::Unsupported, format!("Text extraction failed: {msg}")))
    } else {
        Ok(())
    };
    (ActiveSession { hook, ocr, hook_error, ocr_error, diagnostics, forward }, result)
}
