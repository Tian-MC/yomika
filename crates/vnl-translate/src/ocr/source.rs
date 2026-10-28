//! Periodic OCR as a [`TextSource`]: capture → (skip if the image did not
//! change) → OCR → event. With `interval == 0` the source only validates the
//! setup and waits for manual captures (`translation_ocr_capture`).

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use vnl_core::{AppError, CaptureRegion, TextOrigin};

use super::OcrAdapter;
use crate::capture::ScreenCapture;
use crate::source::{SourceStatus, StatusCell, TextEvent, TextSender, TextSource};
use crate::types::TextSourceState;

/// Consecutive failures after which the loop gives up.
pub const MAX_CONSECUTIVE_FAILURES: u32 = 5;

pub struct OcrTextSource {
    capture: Arc<dyn ScreenCapture>,
    ocr: Arc<dyn OcrAdapter>,
    region: Option<CaptureRegion>,
    interval: Duration,
    language: Option<String>,
    status: StatusCell,
    task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl OcrTextSource {
    pub fn new(
        capture: Arc<dyn ScreenCapture>,
        ocr: Arc<dyn OcrAdapter>,
        region: Option<CaptureRegion>,
        interval: Duration,
        language: Option<String>,
    ) -> Self {
        Self { capture, ocr, region, interval, language, status: StatusCell::default(), task: Mutex::new(None) }
    }
}

fn hash_bytes(b: &[u8]) -> u64 {
    let mut h = DefaultHasher::new();
    b.hash(&mut h);
    h.finish()
}

#[async_trait]
impl TextSource for OcrTextSource {
    fn id(&self) -> &str {
        "ocr"
    }

    fn origin(&self) -> TextOrigin {
        TextOrigin::Ocr
    }

    async fn start(&self, tx: TextSender) -> Result<(), AppError> {
        self.status.set_state(TextSourceState::Starting);
        self.status.push(format!("OCR backend {} via {} capture", self.ocr.id(), self.capture.id()));
        if self.interval.is_zero() {
            self.status.push("Automatic capture disabled (interval 0): use manual capture.");
            self.status.set_state(TextSourceState::Active);
            return Ok(());
        }
        // One synchronous capture to surface setup errors immediately.
        if let Err(e) = self.capture.capture(self.region).await {
            self.status.fail(format!("Screen capture failed: {}", e.message));
            return Err(e);
        }
        self.status.set_state(TextSourceState::Active);
        let (capture, ocr, region, interval, language, status) = (
            self.capture.clone(),
            self.ocr.clone(),
            self.region,
            self.interval.max(Duration::from_millis(250)),
            self.language.clone(),
            self.status.clone(),
        );
        let handle = tokio::spawn(async move {
            let mut last_hash = None;
            let mut failures = 0u32;
            let mut ticker = tokio::time::interval(interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                if tx.is_closed() {
                    break;
                }
                let result = async {
                    let img = capture.capture(region).await?;
                    let h = hash_bytes(&img.bytes);
                    if last_hash == Some(h) {
                        return Ok(None);
                    }
                    last_hash = Some(h);
                    ocr.recognize(&img, language.as_deref()).await.map(Some)
                }
                .await;
                match result {
                    Ok(Some(text)) => {
                        failures = 0;
                        if !text.trim().is_empty() {
                            let ev = TextEvent { origin: TextOrigin::Ocr, text, detail: Some(ocr.id()) };
                            if tx.send(ev).await.is_err() {
                                break;
                            }
                        }
                    }
                    Ok(None) => failures = 0,
                    Err(e) => {
                        failures += 1;
                        status.push(format!("OCR cycle failed: {}", e.message));
                        if failures >= MAX_CONSECUTIVE_FAILURES {
                            status.fail(format!("Stopped OCR after {failures} consecutive failures"));
                            break;
                        }
                    }
                }
            }
        });
        *self.task.lock().unwrap() = Some(handle);
        Ok(())
    }

    async fn stop(&self) {
        if let Some(h) = self.task.lock().unwrap().take() {
            h.abort();
        }
        if self.status.get().state != TextSourceState::Failed {
            self.status.set_state(TextSourceState::Inactive);
        }
    }

    fn status(&self) -> SourceStatus {
        self.status.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::CapturedImage;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct FrameCapture(AtomicU32);
    #[async_trait]
    impl ScreenCapture for FrameCapture {
        fn id(&self) -> &str {
            "fake"
        }
        async fn capture(&self, _r: Option<CaptureRegion>) -> Result<CapturedImage, AppError> {
            // Frames: 0,0,1,1,2,2,... (each frame captured twice).
            let n = self.0.fetch_add(1, Ordering::SeqCst) / 2;
            Ok(CapturedImage { bytes: vec![n as u8], source: "fake".into() })
        }
    }

    struct EchoOcr(AtomicU32);
    #[async_trait]
    impl OcrAdapter for EchoOcr {
        fn id(&self) -> String {
            "fake-ocr".into()
        }
        async fn recognize(&self, image: &CapturedImage, _l: Option<&str>) -> Result<String, AppError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(format!("frame {}", image.bytes[0]))
        }
    }

    #[tokio::test(start_paused = true)]
    async fn skips_unchanged_frames() {
        let ocr = Arc::new(EchoOcr(AtomicU32::new(0)));
        let src = OcrTextSource::new(
            Arc::new(FrameCapture(AtomicU32::new(0))),
            ocr.clone(),
            None,
            Duration::from_millis(500),
            Some("ja".into()),
        );
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        src.start(tx).await.unwrap();
        assert_eq!(src.status().state, TextSourceState::Active);
        let mut got = Vec::new();
        for _ in 0..3 {
            got.push(rx.recv().await.unwrap().text);
        }
        src.stop().await;
        // The startup probe consumed one capture (frame 0, first half).
        assert_eq!(got, vec!["frame 0", "frame 1", "frame 2"]);
        assert_eq!(src.status().state, TextSourceState::Inactive);
    }

    struct BrokenCapture;
    #[async_trait]
    impl ScreenCapture for BrokenCapture {
        fn id(&self) -> &str {
            "broken"
        }
        async fn capture(&self, _r: Option<CaptureRegion>) -> Result<CapturedImage, AppError> {
            Err(AppError::invalid("no display"))
        }
    }

    #[tokio::test]
    async fn capture_failure_is_reported() {
        let src = OcrTextSource::new(
            Arc::new(BrokenCapture),
            Arc::new(EchoOcr(AtomicU32::new(0))),
            None,
            Duration::from_secs(1),
            None,
        );
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        assert!(src.start(tx).await.is_err());
        let st = src.status();
        assert_eq!(st.state, TextSourceState::Failed);
        assert!(st.diagnostics.iter().any(|d| d.contains("no display")));
    }
}
