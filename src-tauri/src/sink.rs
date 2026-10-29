//! Bridges subsystem `AppEvent`s to Tauri events and keeps a log ring buffer.

use std::collections::{HashSet, VecDeque};
use std::sync::{Mutex, OnceLock};

use tauri::{AppHandle, Emitter};
use vnl_core::{AppEvent, EventSink, LogEntry, SessionInfo};

const LOG_CAPACITY: usize = 5000;

type SessionEndHandler = Box<dyn Fn(&SessionInfo) + Send + Sync>;

pub struct TauriSink {
    app: AppHandle,
    logs: Mutex<VecDeque<LogEntry>>,
    finished_sessions: Mutex<HashSet<String>>,
    on_session_end: OnceLock<SessionEndHandler>,
}

impl TauriSink {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            logs: Mutex::new(VecDeque::with_capacity(LOG_CAPACITY)),
            finished_sessions: Mutex::new(HashSet::new()),
            on_session_end: OnceLock::new(),
        }
    }

    /// Called exactly once per session when it reaches Exited/Failed.
    pub fn set_session_end_handler(&self, handler: SessionEndHandler) {
        let _ = self.on_session_end.set(handler);
    }

    pub fn recent_logs(&self, limit: usize, session_id: Option<&str>) -> Vec<LogEntry> {
        let logs = self.logs.lock().unwrap();
        let mut out: Vec<LogEntry> = logs
            .iter()
            .rev()
            .filter(|l| session_id.is_none_or(|id| l.session_id.as_deref() == Some(id)))
            .take(limit)
            .cloned()
            .collect();
        out.reverse();
        out
    }

    fn push_log(&self, entry: &LogEntry) {
        let mut logs = self.logs.lock().unwrap();
        if logs.len() == LOG_CAPACITY {
            logs.pop_front();
        }
        logs.push_back(entry.clone());
    }
}

impl EventSink for TauriSink {
    fn emit(&self, event: AppEvent) {
        let result = match &event {
            AppEvent::Session(info) => {
                if info.state.is_finished()
                    && self.finished_sessions.lock().unwrap().insert(info.id.clone())
                    && let Some(handler) = self.on_session_end.get()
                {
                    handler(info);
                }
                self.app.emit("vnl://session", info)
            }
            AppEvent::Log(entry) => {
                self.push_log(entry);
                self.app.emit("vnl://log", entry)
            }
            AppEvent::Translation(line) => self.app.emit("vnl://translation", line),
            AppEvent::Task(task) => self.app.emit("vnl://task", task),
        };
        if let Err(e) = result {
            tracing::warn!("failed to emit event: {e}");
        }
    }
}
