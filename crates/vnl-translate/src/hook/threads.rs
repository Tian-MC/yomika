//! CLI thread discovery and selection. Runtime PID/handle never enter saved settings.
use super::protocol::HostSentence;
use crate::source::TextEvent;
use crate::types::HookThread;
use std::collections::BTreeMap;
use vnl_core::{AppError, HookThreadSelection, TextOrigin};

#[derive(Default)]
pub(crate) struct ThreadRegistry {
    pub selection: Option<HookThreadSelection>,
    threads: BTreeMap<String, HookThread>,
    selected_id: Option<String>,
    generation: u64,
    last_text: String,
    closed: bool,
}

impl ThreadRegistry {
    pub fn begin(&mut self) {
        self.threads.clear();
        self.selected_id = None;
        self.last_text.clear();
        self.generation += 1;
        self.closed = false;
    }
    pub fn close(&mut self) {
        if !self.closed {
            self.closed = true;
            self.generation += 1;
        }
    }
    pub fn snapshot(&self) -> (Vec<HookThread>, Option<String>) {
        let selected = self
            .selected_id
            .as_ref()
            .filter(|id| {
                !self.closed && self.threads.get(*id).is_some_and(|t| Some(&t.selector) == self.selection.as_ref())
            })
            .cloned();
        (self.threads.values().cloned().collect(), selected)
    }

    pub fn select(&mut self, id: Option<&str>) -> Result<Option<HookThreadSelection>, AppError> {
        if self.closed {
            return Err(AppError::invalid("The hook session has stopped"));
        }
        let selection = match id {
            Some(id) => Some(
                self.threads
                    .get(id)
                    .ok_or_else(|| AppError::not_found("Text thread is no longer available"))?
                    .selector
                    .clone(),
            ),
            None => None,
        };
        self.selection = selection.clone();
        self.selected_id = id.map(str::to_string);
        self.generation += 1;
        self.last_text.clear();
        Ok(selection)
    }
    pub fn accepts(&self, event: &TextEvent) -> bool {
        !self.closed
            && self.selected_id.as_ref().is_some_and(|id| {
                self.threads.get(id).is_some_and(|t| Some(&t.selector) == self.selection.as_ref())
                    && event.detail.as_deref() == Some(format!("{id}@{}", self.generation).as_str())
            })
    }
    pub fn observe(&mut self, sentence: HostSentence, pid: u32) -> Option<TextEvent> {
        if self.closed || sentence.pid != pid || sentence.is_console() || sentence.is_host_clipboard() {
            return None;
        }
        let id = format!("{:X}:{:X}", sentence.pid, sentence.handle);
        let selector = HookThreadSelection {
            name: sentence.name,
            hook_code: sentence.hook_code,
            context: format!("{:X}", sentence.ctx),
            context2: format!("{:X}", sentence.ctx2),
        };
        if !self.threads.contains_key(&id) && self.threads.len() >= 64 {
            let evict =
                self.threads.iter().find(|(id, _)| Some(*id) != self.selected_id.as_ref()).map(|(id, _)| id.clone());
            if let Some(evict) = evict {
                self.threads.remove(&evict);
            }
        }
        let thread = self.threads.entry(id.clone()).or_insert_with(|| HookThread {
            id: id.clone(),
            selector: selector.clone(),
            preview: String::new(),
            received: 0,
        });
        thread.selector = selector.clone();
        thread.preview = sentence.text.chars().take(240).collect();
        thread.received = thread.received.saturating_add(1);
        let text = sentence.text.trim().to_string();
        if self.selected_id.is_none() && Some(&selector) == self.selection.as_ref() {
            self.selected_id = Some(id.clone());
        }
        if self.selected_id.as_ref() != Some(&id)
            || Some(&selector) != self.selection.as_ref()
            || text.is_empty()
            || text.chars().count() > 8192
            || text == self.last_text
        {
            return None;
        }
        self.last_text = text.clone();
        Some(TextEvent { origin: TextOrigin::Hook, text, detail: Some(format!("{id}@{}", self.generation)) })
    }
}

#[cfg(test)]
mod tests {
    use super::super::protocol::parse_header_line;
    use super::*;

    fn sentence(handle: u64, context: u64, text: &str) -> HostSentence {
        parse_header_line(&format!("[{handle:X}:164:ABC:{context:X}:0:Engine:HS4@0:Game.exe] {text}")).unwrap()
    }

    #[test]
    fn discovers_contexts_selects_one_thread_and_rejects_stale_queue() {
        let mut registry = ThreadRegistry::default();
        registry.begin();
        assert!(registry.observe(sentence(1, 10, "menu"), 356).is_none());
        assert!(registry.observe(sentence(2, 20, "dialogue"), 356).is_none());
        assert_eq!(registry.snapshot().0.len(), 2);
        registry.select(Some("164:2")).unwrap();
        assert!(registry.observe(sentence(1, 10, "noise"), 356).is_none());
        let accepted = registry.observe(sentence(2, 20, "dialogue"), 356).unwrap();
        assert!(registry.accepts(&accepted));
        assert!(registry.observe(sentence(2, 20, "dialogue"), 356).is_none());
        registry.select(Some("164:1")).unwrap();
        assert!(!registry.accepts(&accepted));
        let next = registry.observe(sentence(1, 10, "new line"), 356).unwrap();
        registry.close();
        registry.close();
        assert!(!registry.accepts(&next));
        assert!(registry.observe(sentence(1, 10, "after cancellation"), 356).is_none());
        assert!(registry.select(Some("164:1")).is_err());
    }

    #[test]
    fn restores_fingerprint_across_pid_handle_and_address_changes() {
        let mut registry = ThreadRegistry::default();
        registry.observe(sentence(1, u64::MAX, "original"), 356);
        let saved = registry.select(Some("164:1")).unwrap();
        let json = serde_json::to_string(&saved).unwrap();
        assert!(json.contains("FFFFFFFFFFFFFFFF"));
        assert!(!json.contains("pid"));
        registry.selection = serde_json::from_str(&json).unwrap();
        registry.begin();
        let mut next = sentence(99, u64::MAX, "next session");
        next.pid = 400;
        next.addr = 123;
        assert!(registry.observe(next, 400).is_some());
        assert_eq!(registry.snapshot().1.as_deref(), Some("190:63"));
        // Even identical fingerprints must not forward two runtime threads.
        let mut duplicate = sentence(100, u64::MAX, "other thread");
        duplicate.pid = 400;
        assert!(registry.observe(duplicate, 400).is_none());
        registry.select(None).unwrap();
        assert!(registry.observe(sentence(1, 0, "paused"), 356).is_none());
    }

    #[test]
    fn gsm_threads_keep_pid_filter_selection_and_saved_fingerprint() {
        let parse = |handle: u64, pid: u32, context: u64, text: &str| {
            parse_header_line(&format!(
                "[#{handle:X}|{handle:X}:{pid:X}:7BE5B820:0:{context:X}:BGI:HS4@0:Yorikure.exe] {text}"
            ))
            .unwrap()
        };
        let mut registry = ThreadRegistry::default();
        registry.begin();
        assert!(registry.observe(parse(0, 999, 1, "foreign process"), 336).is_none());
        assert!(registry.snapshot().0.is_empty());
        assert!(registry.observe(parse(0, 336, 1, "menu"), 336).is_none());
        assert!(registry.observe(parse(1, 336, 2, "dialogue"), 336).is_none());
        let saved = registry.select(Some("150:1")).unwrap();
        assert!(registry.observe(parse(0, 336, 1, "noise"), 336).is_none());
        let event = registry.observe(parse(1, 336, 2, "台詞"), 336).unwrap();
        assert!(registry.accepts(&event));
        assert!(registry.observe(parse(1, 336, 2, "台詞"), 336).is_none());
        registry.selection = serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap();
        registry.begin();
        assert!(!registry.accepts(&event));
        assert!(registry.observe(parse(9, 400, 2, "next session"), 400).is_some());
        assert_eq!(registry.snapshot().1.as_deref(), Some("190:9"));
    }

    #[test]
    fn ignores_foreign_pids_and_bounds_thread_previews() {
        let mut registry = ThreadRegistry::default();
        assert!(registry.observe(sentence(1, 0, "wrong game"), 999).is_none());
        assert!(registry.snapshot().0.is_empty());
        assert!(registry.select(Some("unknown")).is_err());
        for handle in 1..200 {
            registry.observe(sentence(handle, handle, &"字".repeat(1000)), 356);
        }
        let threads = registry.snapshot().0;
        assert_eq!(threads.len(), 64);
        assert!(threads.iter().all(|t| t.preview.chars().count() == 240));
    }
}
