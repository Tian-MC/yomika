//! Events flowing from subsystems to the UI.
//!
//! Subsystem crates never depend on Tauri. They receive an `Arc<dyn EventSink>`
//! and emit `AppEvent`s; `src-tauri` forwards them as Tauri events:
//!
//! | variant       | Tauri event name    | payload           |
//! |---------------|---------------------|-------------------|
//! | `Session`     | `vnl://session`     | `SessionInfo`     |
//! | `Log`         | `vnl://log`         | `LogEntry`        |
//! | `Translation` | `vnl://translation` | `TranslationLine` |
//! | `Task`        | `vnl://task`        | `TaskProgress`    |

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::game::GameId;
use crate::runtime::{SessionId, SessionInfo};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub ts_ms: i64,
    pub level: LogLevel,
    /// Subsystem or stream: `"runtime"`, `"game.stdout"`, `"game.stderr"`,
    /// `"gamescope"`, `"translation"`, `"library"`, ...
    pub source: String,
    pub session_id: Option<SessionId>,
    pub message: String,
}

impl LogEntry {
    pub fn new(level: LogLevel, source: impl Into<String>, message: impl Into<String>) -> Self {
        Self { ts_ms: crate::now_ms(), level, source: source.into(), session_id: None, message: message.into() }
    }

    pub fn session(mut self, id: impl Into<SessionId>) -> Self {
        self.session_id = Some(id.into());
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TextOrigin {
    Hook,
    Ocr,
    Manual,
    Clipboard,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationLine {
    pub id: String,
    pub session_id: Option<SessionId>,
    pub game_id: Option<GameId>,
    pub origin: TextOrigin,
    pub source_text: String,
    pub translated_text: Option<String>,
    pub source_language: Option<String>,
    pub target_language: Option<String>,
    pub provider_id: Option<String>,
    pub error: Option<String>,
    pub ts_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TaskState {
    Running,
    Done,
    Failed,
}

/// Progress of a long-running background job (prefix creation, downloads...).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskProgress {
    pub task_id: String,
    pub label: String,
    pub state: TaskState,
    /// 0.0–1.0 when known.
    pub progress: Option<f32>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload", rename_all = "camelCase")]
pub enum AppEvent {
    Session(SessionInfo),
    Log(LogEntry),
    Translation(TranslationLine),
    Task(TaskProgress),
}

pub trait EventSink: Send + Sync + 'static {
    fn emit(&self, event: AppEvent);

    fn log(&self, entry: LogEntry) {
        self.emit(AppEvent::Log(entry));
    }
}

/// Discards all events.
pub struct NullSink;

impl EventSink for NullSink {
    fn emit(&self, _event: AppEvent) {}
}

/// Records events in memory. Intended for tests.
#[derive(Default, Clone)]
pub struct RecordingSink {
    events: Arc<Mutex<Vec<AppEvent>>>,
}

impl RecordingSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn events(&self) -> Vec<AppEvent> {
        self.events.lock().unwrap().clone()
    }
}

impl EventSink for RecordingSink {
    fn emit(&self, event: AppEvent) {
        self.events.lock().unwrap().push(event);
    }
}
