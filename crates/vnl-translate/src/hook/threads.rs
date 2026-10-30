//! CLI thread discovery and selection. Runtime PID/handle never enter saved settings.
use super::protocol::HostSentence;
use crate::source::TextEvent;
use crate::types::HookThread;
use std::collections::BTreeMap;
use vnl_core::{AppError, HookThreadSelection, TextOrigin};

#[derive(Default)]
pub(crate) struct ThreadRegistry {
    pub selection: Option<HookThreadSelection>,
    pub selections: Vec<HookThreadSelection>,
    pub preferred: Option<HookThreadSelection>,
    pub filters: vnl_core::HookFilters,
    pub extraction: vnl_core::HookExtraction,
    pub auto_select: bool,
    threads: BTreeMap<String, HookThread>,
    active_ids: BTreeMap<HookThreadSelection, String>,
    selected_id: Option<String>,
    generation: u64,
    last_text: String,
    last_emitted_time: Option<std::time::Instant>,
    closed: bool,
}

impl ThreadRegistry {
    pub fn begin(&mut self) {
        self.threads.clear();
        self.active_ids.clear();
        self.selected_id = None;
        self.last_text.clear();
        self.last_emitted_time = None;
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
                !self.closed
                    && self.threads.get(*id).is_some_and(|t| {
                        Some(&t.selector) == self.selection.as_ref() || self.selections.contains(&t.selector)
                    })
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
        if id.is_none() {
            self.auto_select = false;
        }
        self.selection = selection.clone();
        self.selections = selection.iter().cloned().collect();
        self.active_ids.clear();
        if let (Some(id), Some(sel)) = (id, &selection) {
            self.active_ids.insert(sel.clone(), id.to_string());
        }
        self.selected_id = id.map(str::to_string);
        self.generation += 1;
        self.last_text.clear();
        self.last_emitted_time = None;
        Ok(selection)
    }

    pub fn select_multiple(&mut self, ids: &[String]) -> Result<Vec<HookThreadSelection>, AppError> {
        if self.closed {
            return Err(AppError::invalid("The hook session has stopped"));
        }
        self.active_ids.clear();
        let mut out = Vec::new();
        for id in ids {
            let Some(t) = self.threads.get(id) else { continue };
            if !out.contains(&t.selector) {
                out.push(t.selector.clone());
                self.active_ids.insert(t.selector.clone(), id.clone());
            }
        }
        self.selections = out.clone();
        self.selection = out.first().cloned();
        self.selected_id = ids.first().cloned();
        self.generation += 1;
        self.last_text.clear();
        self.last_emitted_time = None;
        Ok(out)
    }

    pub fn accepts(&self, event: &TextEvent) -> bool {
        !self.closed
            && event.detail.as_deref().is_some_and(|detail| {
                let Some((id, gen_str)) = detail.split_once('@') else { return false };
                if gen_str != self.generation.to_string() {
                    return false;
                }
                self.threads.get(id).is_some_and(|t| {
                    Some(&t.selector) == self.selection.as_ref() || self.selections.contains(&t.selector)
                })
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
        let is_filtered = self.filters.hidden_names.iter().any(|h| h.eq_ignore_ascii_case(&selector.name));
        let is_preferred = self.preferred.as_ref().is_some_and(|p| {
            p.name.eq_ignore_ascii_case(&selector.name)
                && (p.hook_code.is_empty() || super::protocol::hook_code_matches(&p.hook_code, &selector.hook_code))
        });

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
            addr: format!("0x{:X}", sentence.addr),
            pid: sentence.pid,
            is_filtered,
            is_preferred,
        });
        thread.selector = selector.clone();
        thread.preview = sentence.text.chars().take(240).collect();
        thread.received = thread.received.saturating_add(1);
        thread.addr = format!("0x{:X}", sentence.addr);
        thread.pid = sentence.pid;
        thread.is_filtered = is_filtered;
        thread.is_preferred = is_preferred;

        // Auto-select preferred thread, or first non-filtered thread when auto_select is enabled
        if self.selection.is_none() && self.selections.is_empty() && !is_filtered && (is_preferred || self.auto_select)
        {
            self.selection = Some(selector.clone());
            self.selections = vec![selector.clone()];
        }

        let is_selected_selector = Some(&selector) == self.selection.as_ref() || self.selections.contains(&selector);

        if !is_selected_selector {
            return None;
        }

        let active_id = self.active_ids.entry(selector.clone()).or_insert_with(|| id.clone());
        if active_id != &id {
            // Another runtime thread already matches this selector; ignore duplicates.
            return None;
        }

        if self.selected_id.is_none() {
            self.selected_id = Some(id.clone());
        }

        let text = sentence.text.trim().to_string();
        if text.is_empty() || text.chars().count() > 8192 {
            return None;
        }

        if self.filters.min_chars > 0 && (text.chars().count() as u32) < self.filters.min_chars {
            return None;
        }

        if self.filters.require_cjk
            && !text.chars().any(|c| {
                ('\u{3040}'..='\u{30FF}').contains(&c)
                    || ('\u{4E00}'..='\u{9FFF}').contains(&c)
                    || ('\u{AC00}'..='\u{D7AF}').contains(&c)
            })
        {
            return None;
        }

        let now = std::time::Instant::now();
        let repeat_gap = if self.extraction.repeat_gap_ms > 0 {
            std::time::Duration::from_millis(self.extraction.repeat_gap_ms)
        } else {
            std::time::Duration::from_millis(3000)
        };

        if text == self.last_text {
            let last_time = self.last_emitted_time?;
            if now.duration_since(last_time) < repeat_gap {
                return None;
            }
        }

        self.last_text = text.clone();
        self.last_emitted_time = Some(now);
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
                "[#{handle:X}|{handle:X}:{pid:X}:7BE5B820:0:{context:X}:BGI:HS4@0:Game.exe] {text}"
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

    #[test]
    fn filters_cjk_min_chars_and_auto_selects_preferred() {
        let mut registry = ThreadRegistry::default();
        registry.filters.require_cjk = true;
        registry.filters.min_chars = 3;
        registry.preferred = Some(HookThreadSelection {
            name: "BGI".into(),
            hook_code: "HS4@0:Game.exe".into(),
            context: "0".into(),
            context2: "2".into(),
        });
        registry.begin();

        let make_sent = |name: &str, text: &str| HostSentence {
            handle: 1,
            pid: 100,
            addr: 0x401000,
            ctx: 0,
            ctx2: 2,
            name: name.to_string(),
            hook_code: "HS4@0:Game.exe".to_string(),
            text: text.to_string(),
        };

        // Non-CJK text is rejected by require_cjk filter even if matching preferred
        assert!(registry.observe(make_sent("BGI", "Hello world!"), 100).is_none());

        // Too short text is rejected by min_chars (3) filter
        assert!(registry.observe(make_sent("BGI", "はい"), 100).is_none());

        // Valid CJK dialogue with >= 3 chars matching preferred is auto-selected and accepted!
        let ev = registry.observe(make_sent("BGI", "こんにちは世界"), 100);
        assert!(ev.is_some());
        assert_eq!(registry.snapshot().1.as_deref(), Some("64:1"));

        // Threads list should have is_preferred = true
        let snapshot = registry.snapshot().0;
        assert_eq!(snapshot.len(), 1);
        assert!(snapshot[0].is_preferred);
        assert_eq!(snapshot[0].addr, "0x401000");
    }

    #[test]
    fn multi_selection_and_repeat_gap() {
        let mut registry = ThreadRegistry::default();
        registry.extraction.repeat_gap_ms = 1; // 1 ms repeat gap for fast testing
        registry.begin();

        let s1 = sentence(1, 1, "最初の台詞");
        let s2 = sentence(2, 2, "二番目の台詞");
        registry.observe(s1.clone(), 356);
        registry.observe(s2.clone(), 356);

        // Select both threads
        let ids = vec!["164:1".to_string(), "164:2".to_string()];
        let selected = registry.select_multiple(&ids).unwrap();
        assert_eq!(selected.len(), 2);

        // First emission after selection succeeds
        assert!(registry.observe(s1.clone(), 356).is_some());

        // Immediate duplicate is suppressed
        assert!(registry.observe(s1.clone(), 356).is_none());

        // Wait to exceed 1ms repeat gap
        std::thread::sleep(std::time::Duration::from_millis(50));

        // After repeat gap expired, repeated dialogue (e.g. user re-reading dialogue) is emitted again
        assert!(registry.observe(s1, 356).is_some());
    }

    #[test]
    fn auto_select_first_thread_when_enabled() {
        let mut registry = ThreadRegistry { auto_select: true, ..Default::default() };
        registry.begin();
        let s = sentence(1, 1, "最初の台詞");
        let ev = registry.observe(s.clone(), 356);
        assert!(ev.is_some());
        assert_eq!(registry.snapshot().1.as_deref(), Some("164:1"));
    }
}
