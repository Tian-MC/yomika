//! Clipboard watcher [`TextSource`]: polls `wl-paste` (Wayland) or
//! `xclip`/`xsel` (X11) and emits new clipboard text. Useful with any external
//! hooker configured to copy extracted text to the clipboard. Verified on
//! GNOME 51 Wayland: `wl-paste --no-newline` returns in ~25 ms without
//! stealing focus. The clipboard content present at start is ignored.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use vnl_core::{AppError, CommandSpec, ErrorCode, TextOrigin};

use crate::hook::HookAdapter;
use crate::source::{SourceStatus, StatusCell, TextEvent, TextSender, TextSource};
use crate::types::{BackendInfo, TextSourceState};

pub const BACKEND_ID: &str = "clipboard";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardReader {
    pub spec: CommandSpec,
    pub label: &'static str,
}

/// Picks a reader for the current session (Wayland first).
pub fn detect_reader() -> Result<ClipboardReader, AppError> {
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty());
    let x11 = std::env::var_os("DISPLAY").is_some_and(|v| !v.is_empty());
    if wayland && let Ok(p) = which::which("wl-paste") {
        return Ok(ClipboardReader {
            spec: CommandSpec::new(p).args(["--no-newline", "--type", "text"]),
            label: "wl-paste",
        });
    }
    if x11 {
        if let Ok(p) = which::which("xclip") {
            return Ok(ClipboardReader {
                spec: CommandSpec::new(p).args(["-o", "-selection", "clipboard"]),
                label: "xclip",
            });
        }
        if let Ok(p) = which::which("xsel") {
            return Ok(ClipboardReader { spec: CommandSpec::new(p).args(["--clipboard", "--output"]), label: "xsel" });
        }
    }
    Err(AppError::new(ErrorCode::MissingDependency, "No clipboard tool found")
        .with_details("Install wl-clipboard (Wayland: `pacman -S wl-clipboard`) or xclip (X11: `pacman -S xclip`)."))
}

pub fn backend_info() -> BackendInfo {
    match detect_reader() {
        Ok(r) => BackendInfo {
            id: BACKEND_ID.into(),
            name: "Clipboard watcher".into(),
            available: true,
            diagnostics: vec![format!(
                "Polling {} ({}). Configure your hooker (e.g. LunaTranslator/Textractor) to copy text to the clipboard.",
                r.label,
                r.spec.program.display()
            )],
        },
        Err(e) => BackendInfo {
            id: BACKEND_ID.into(),
            name: "Clipboard watcher".into(),
            available: false,
            diagnostics: vec![e.message, e.details.unwrap_or_default()],
        },
    }
}

async fn read_clipboard(spec: &CommandSpec) -> Option<String> {
    let mut cmd = tokio::process::Command::from(spec.to_std_command());
    cmd.stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).kill_on_drop(true);
    let out = tokio::time::timeout(Duration::from_secs(2), cmd.output()).await.ok()?.ok()?;
    // Empty clipboard → non-zero exit ("Nothing is copied").
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub struct ClipboardSource {
    reader: Option<ClipboardReader>,
    interval: Duration,
    status: StatusCell,
    task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl ClipboardSource {
    pub fn new(interval: Duration) -> Self {
        Self { reader: None, interval, status: StatusCell::default(), task: Mutex::new(None) }
    }

    /// Uses a specific reader command (tests, custom tools).
    pub fn with_reader(program: PathBuf, args: Vec<String>, interval: Duration) -> Self {
        Self {
            reader: Some(ClipboardReader { spec: CommandSpec::new(program).args(args), label: "custom" }),
            interval,
            status: StatusCell::default(),
            task: Mutex::new(None),
        }
    }
}

#[async_trait]
impl TextSource for ClipboardSource {
    fn id(&self) -> &str {
        BACKEND_ID
    }

    fn origin(&self) -> TextOrigin {
        TextOrigin::Clipboard
    }

    async fn start(&self, tx: TextSender) -> Result<(), AppError> {
        self.stop().await;
        self.status.set_state(TextSourceState::Starting);
        let reader = match self.reader.clone().map(Ok).unwrap_or_else(detect_reader) {
            Ok(r) => r,
            Err(e) => {
                self.status.fail(e.message.clone());
                return Err(e);
            }
        };
        self.status.push(format!("Watching the clipboard with {}.", reader.label));
        let initial = read_clipboard(&reader.spec).await;
        self.status.set_state(TextSourceState::Active);
        let interval = self.interval.max(Duration::from_millis(100));
        let handle = tokio::spawn(async move {
            let mut last = initial;
            loop {
                tokio::time::sleep(interval).await;
                if tx.is_closed() {
                    break;
                }
                let Some(now) = read_clipboard(&reader.spec).await else { continue };
                if last.as_deref() == Some(now.as_str()) {
                    continue;
                }
                last = Some(now.clone());
                if !now.trim().is_empty() && tx.send(TextEvent::new(TextOrigin::Clipboard, now)).await.is_err() {
                    break;
                }
            }
        });
        *self.task.lock().unwrap() = Some(handle);
        Ok(())
    }

    async fn stop(&self) {
        if let Some(h) = self.task.lock().unwrap().take() {
            h.abort();
            if self.status.get().state != TextSourceState::Failed {
                self.status.set_state(TextSourceState::Inactive);
            }
        }
    }

    fn status(&self) -> SourceStatus {
        self.status.get()
    }
}

impl HookAdapter for ClipboardSource {
    fn backend_id(&self) -> &str {
        BACKEND_ID
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn emits_changes_only() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("clip.txt");
        std::fs::write(&file, "already there").unwrap();
        let src = ClipboardSource::with_reader(
            "/bin/cat".into(),
            vec![file.to_string_lossy().into()],
            Duration::from_millis(100),
        );
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        src.start(tx).await.unwrap();
        assert_eq!(src.status().state, TextSourceState::Active);
        tokio::time::sleep(Duration::from_millis(250)).await;
        std::fs::write(&file, "「新しい台詞」").unwrap();
        let ev = tokio::time::timeout(Duration::from_secs(3), rx.recv()).await.unwrap().unwrap();
        assert_eq!(ev.text, "「新しい台詞」");
        assert_eq!(ev.origin, TextOrigin::Clipboard);
        assert!(tokio::time::timeout(Duration::from_millis(350), rx.recv()).await.is_err());
        src.stop().await;
        assert_eq!(src.status().state, TextSourceState::Inactive);
    }
}
