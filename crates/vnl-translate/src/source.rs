//! `TextSource`: anything producing lines of game text (hook host, OCR loop,
//! clipboard watcher). Sources push [`TextEvent`]s into an mpsc channel that
//! the session forwards into the [`crate::pipeline::TranslationPipeline`].

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tokio::sync::mpsc;
use vnl_core::{AppError, TextOrigin};

use crate::types::TextSourceState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEvent {
    pub origin: TextOrigin,
    pub text: String,
    /// Source-specific detail (hook name/code, OCR backend...).
    pub detail: Option<String>,
}

impl TextEvent {
    pub fn new(origin: TextOrigin, text: impl Into<String>) -> Self {
        Self { origin, text: text.into(), detail: None }
    }
}

pub type TextSender = mpsc::Sender<TextEvent>;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceStatus {
    pub state: TextSourceState,
    pub diagnostics: Vec<String>,
    pub hook_threads: Vec<crate::types::HookThread>,
    pub selected_hook_thread: Option<String>,
}

/// Shared, cheaply clonable status cell used by source implementations.
#[derive(Debug, Clone, Default)]
pub struct StatusCell(Arc<Mutex<SourceStatus>>);

impl StatusCell {
    pub fn get(&self) -> SourceStatus {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    pub fn set_state(&self, state: TextSourceState) {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).state = state;
    }

    pub fn push(&self, diagnostic: impl Into<String>) {
        let mut s = self.0.lock().unwrap_or_else(|p| p.into_inner());
        let d = diagnostic.into();
        if s.diagnostics.last() != Some(&d) {
            s.diagnostics.push(d);
            // Bounded: keep the most recent 50.
            let excess = s.diagnostics.len().saturating_sub(50);
            s.diagnostics.drain(..excess);
        }
    }

    pub fn fail(&self, diagnostic: impl Into<String>) {
        self.push(diagnostic);
        self.set_state(TextSourceState::Failed);
    }
}

#[async_trait]
pub trait TextSource: Send + Sync {
    /// Stable id, e.g. `"lunahost"`, `"clipboard"`, `"ocr"`.
    fn id(&self) -> &str;
    fn origin(&self) -> TextOrigin;
    /// Starts producing events into `tx`. Returns once the source is running
    /// (or failed); production continues in background tasks.
    async fn start(&self, tx: TextSender) -> Result<(), AppError>;
    /// Stops background work and kills helper processes. Idempotent.
    async fn stop(&self);
    fn status(&self) -> SourceStatus;
    fn select_thread(&self, _id: Option<&str>) -> Result<Option<vnl_core::HookThreadSelection>, AppError> {
        Err(AppError::invalid("This source does not expose CLI text threads"))
    }
    fn select_threads(&self, _ids: &[String]) -> Result<Vec<vnl_core::HookThreadSelection>, AppError> {
        Err(AppError::invalid("This source does not expose CLI text threads"))
    }
    async fn insert_hook_code(&self, _code: &str) -> Result<(), AppError> {
        Err(AppError::invalid("This source does not support inserting hook codes"))
    }
    async fn remove_hook_code(&self, _code: &str) -> Result<(), AppError> {
        Err(AppError::invalid("This source does not support removing hook codes"))
    }
    async fn search_hooks(&self) -> Result<(), AppError> {
        Err(AppError::invalid("This source does not support hook search"))
    }
    /// Reject queued events after a thread switch or stop.
    fn accepts(&self, _event: &TextEvent) -> bool {
        true
    }
}
