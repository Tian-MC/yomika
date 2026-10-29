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
            None | Some(crate::hook::lunahost::BACKEND_ID) => Ok(Arc::new(LunaHostAdapter::new(
                self.tools_dir.clone(),
                ctx.game_exe.clone(),
                ctx.prefix.clone(),
                ctx.config.hook.hook_code.clone(),
                LunaHostOptions {
                    attach_delay: Duration::from_millis(ctx.config.hook.attach_delay_ms),
                    ..Default::default()
                },
            ))),
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
        if let Some(h) = &self.hook {
            h.stop().await;
        }
        if let Some(o) = &self.ocr {
            o.stop().await;
        }
        self.forward.abort();
    }

    pub fn status(&self, session_id: &str) -> TranslationSessionStatus {
        let mut st = TranslationSessionStatus::inactive(session_id);
        st.diagnostics = self.diagnostics.clone();
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
) -> (ActiveSession, Result<(), AppError>) {
    let (tx, mut rx) = tokio::sync::mpsc::channel(256);
    let mode = ctx.config.extraction;
    let (mut hook, mut ocr, mut hook_error, mut ocr_error) = (None, None, None, None);

    if wants_hook(mode) {
        match factory.hook(ctx) {
            Ok(src) => match src.start(tx.clone()).await {
                Ok(()) => hook = Some(src),
                Err(e) => {
                    hook_error = Some(e.message.clone());
                    hook = Some(src);
                }
            },
            Err(e) => hook_error = Some(e.message),
        }
    }
    if wants_ocr(mode) {
        match factory.ocr(ctx, ollama_base_url).await {
            Ok(src) => match src.start(tx.clone()).await {
                Ok(()) => ocr = Some(src),
                Err(e) => {
                    ocr_error = Some(e.message.clone());
                    ocr = Some(src);
                }
            },
            Err(e) => {
                ocr_error = Some(format!("{}{}", e.message, e.details.map(|d| format!(" — {d}")).unwrap_or_default()))
            }
        }
    }
    drop(tx);

    let forward = tokio::spawn(async move {
        while let Some(ev) = rx.recv().await {
            if let Some(line) = pipeline.accept(&ev) {
                let p = pipeline.clone();
                // Translation runs concurrently so a slow provider never blocks extraction.
                tokio::spawn(async move {
                    p.complete(line).await;
                });
            }
        }
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
