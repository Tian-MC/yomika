//! `TranslationPipeline`: TextEvent → normalized/deduplicated line →
//! `AppEvent::Translation` (source only) → glossary-aware translation →
//! `AppEvent::Translation` (same id, with translation or error) → history.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use vnl_core::{AppError, AppEvent, EventSink, GameId, SessionId, TextOrigin, TranslationLine};

use crate::languages::{ResolvedLanguages, is_cjk_like};
use crate::providers::{GlossaryHint, GlossaryStrategy, ProviderRequest, TranslationProvider};
use crate::source::TextEvent;
use crate::store::TranslationStore;
use crate::types::GlossaryEntry;

/// Lines longer than this (after normalization) are dropped as hook garbage.
pub const MAX_LINE_CHARS: usize = 1500;

// ---------------------------------------------------------- normalization

/// Collapses text where every character is repeated the same number of times
/// (a classic hook artefact: `ああいいうう` → `あいう`).
pub fn collapse_repeated_chars(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() < 6 {
        return s.to_string();
    }
    let mut runs: Vec<(char, usize)> = Vec::new();
    for &c in &chars {
        match runs.last_mut() {
            Some((p, n)) if *p == c => *n += 1,
            _ => runs.push((c, 1)),
        }
    }
    let k = runs[0].1;
    if k >= 2 && runs.len() >= 3 && runs.iter().all(|(_, n)| n % k == 0) {
        runs.iter().flat_map(|(c, n)| std::iter::repeat_n(*c, n / k)).collect()
    } else {
        s.to_string()
    }
}

/// Removes control/zero-width characters, joins wrapped lines (without spaces
/// for CJK sources), collapses whitespace and hook character repetition.
pub fn normalize(text: &str, source_language: Option<&str>) -> String {
    let cleaned: String = text
        .chars()
        .filter(|c| !matches!(c, '\u{200B}'..='\u{200D}' | '\u{FEFF}' | '\u{2060}'))
        .map(|c| if c == '\t' { ' ' } else { c })
        .filter(|c| *c == '\n' || !c.is_control())
        .collect();
    let cjk = is_cjk_like(source_language)
        || source_language.is_none() && cleaned.chars().any(|c| ('\u{3040}'..='\u{30FF}').contains(&c));
    let lines: Vec<&str> = cleaned.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let joined = lines.join(if cjk { "" } else { " " });
    let mut out = String::with_capacity(joined.len());
    for c in joined.chars() {
        if c == ' ' && out.ends_with(' ') {
            continue;
        }
        out.push(c);
    }
    collapse_repeated_chars(out.trim())
}

// ------------------------------------------------------------------ dedupe

/// Drops consecutive repeats from the same origin and the same text arriving
/// from another source (hook + OCR) within `window`.
#[derive(Debug)]
pub struct Deduper {
    window: Duration,
    last_by_origin: HashMap<&'static str, String>,
    recent: VecDeque<(String, Instant)>,
}

fn origin_key(o: TextOrigin) -> &'static str {
    match o {
        TextOrigin::Hook => "hook",
        TextOrigin::Ocr => "ocr",
        TextOrigin::Manual => "manual",
        TextOrigin::Clipboard => "clipboard",
    }
}

impl Deduper {
    pub fn new(window: Duration) -> Self {
        Self { window, last_by_origin: HashMap::new(), recent: VecDeque::new() }
    }

    /// True if the line should be processed.
    pub fn accept(&mut self, origin: TextOrigin, text: &str) -> bool {
        let now = Instant::now();
        while self.recent.front().is_some_and(|(_, t)| now.duration_since(*t) > self.window) {
            self.recent.pop_front();
        }
        let key = origin_key(origin);
        if self.last_by_origin.get(key).is_some_and(|l| l == text) {
            return false;
        }
        self.last_by_origin.insert(key, text.to_string());
        if self.recent.iter().any(|(t, _)| t == text) {
            return false;
        }
        self.recent.push_back((text.to_string(), now));
        if self.recent.len() > 64 {
            self.recent.pop_front();
        }
        true
    }
}

// ---------------------------------------------------------------- glossary

fn placeholder(i: usize) -> String {
    format!("ZX{i}Z")
}

/// Replaces glossary sources by placeholders (longest first). Returns the
/// rewritten text and the `(placeholder, target)` pairs actually used.
pub fn apply_placeholders(text: &str, glossary: &[GlossaryEntry]) -> (String, Vec<(String, String)>) {
    let mut entries: Vec<&GlossaryEntry> = glossary.iter().filter(|e| e.enabled && !e.source.is_empty()).collect();
    entries.sort_by_key(|e| std::cmp::Reverse(e.source.chars().count()));
    let mut out = text.to_string();
    let mut used = Vec::new();
    for e in entries {
        if out.contains(&e.source) {
            let ph = placeholder(used.len());
            out = out.replace(&e.source, &ph);
            used.push((ph, e.target.clone()));
        }
    }
    (out, used)
}

/// Restores placeholders (tolerating case changes and inserted spaces).
pub fn restore_placeholders(text: &str, used: &[(String, String)]) -> String {
    let mut out = text.to_string();
    for (ph, target) in used {
        let variants = [ph.clone(), ph.to_lowercase(), ph.replace("ZX", "ZX "), ph.replace("ZX", "Zx")];
        for v in variants {
            out = out.replace(&v, target);
        }
    }
    out
}

/// Glossary entries that occur in the text (for LLM prompt hints).
pub fn relevant_hints(text: &str, glossary: &[GlossaryEntry]) -> Vec<GlossaryHint> {
    glossary
        .iter()
        .filter(|e| e.enabled && !e.source.is_empty() && text.contains(&e.source))
        .map(|e| GlossaryHint { source: e.source.clone(), target: e.target.clone(), kind: e.kind })
        .collect()
}

/// Post-pass: source terms the provider left untranslated are replaced.
pub fn replace_leftovers(text: &str, glossary: &[GlossaryEntry]) -> String {
    let mut entries: Vec<&GlossaryEntry> = glossary.iter().filter(|e| e.enabled && !e.source.is_empty()).collect();
    entries.sort_by_key(|e| std::cmp::Reverse(e.source.chars().count()));
    let mut out = text.to_string();
    for e in entries {
        if out.contains(&e.source) {
            out = out.replace(&e.source, &e.target);
        }
    }
    out
}

/// Translates one (already normalized) text with glossary handling.
pub async fn translate_with_glossary(
    provider: &dyn TranslationProvider,
    text: &str,
    langs: &ResolvedLanguages,
    glossary: &[GlossaryEntry],
) -> Result<String, AppError> {
    let (input, used, hints) = match provider.glossary_strategy() {
        GlossaryStrategy::Placeholders => {
            let (t, used) = apply_placeholders(text, glossary);
            (t, used, Vec::new())
        }
        GlossaryStrategy::PromptHints => (text.to_string(), Vec::new(), relevant_hints(text, glossary)),
    };
    let req = ProviderRequest {
        text: input,
        source_language: langs.source.clone(),
        target_language: langs.target.clone(),
        glossary: hints,
    };
    let out = provider.translate(&req).await?;
    Ok(replace_leftovers(&restore_placeholders(&out, &used), glossary))
}

// ---------------------------------------------------------------- pipeline

pub struct PipelineConfig {
    pub session_id: Option<SessionId>,
    pub game_id: Option<GameId>,
    /// `None` = extraction only (no provider selected).
    pub provider: Option<Arc<dyn TranslationProvider>>,
    /// Required when `provider` is set.
    pub languages: Option<ResolvedLanguages>,
    /// Source language used for normalization when no translation happens.
    pub source_language: Option<String>,
    pub glossary: Vec<GlossaryEntry>,
    pub keep_history: bool,
    pub dedupe: bool,
}

pub struct TranslationPipeline {
    cfg: PipelineConfig,
    events: Arc<dyn EventSink>,
    store: Option<Arc<TranslationStore>>,
    dedupe: Mutex<Deduper>,
}

impl TranslationPipeline {
    pub fn new(cfg: PipelineConfig, events: Arc<dyn EventSink>, store: Option<Arc<TranslationStore>>) -> Self {
        Self { cfg, events, store, dedupe: Mutex::new(Deduper::new(Duration::from_secs(4))) }
    }

    /// Normalizes and dedupes; emits the source-only line. `None` = skipped.
    pub fn accept(&self, ev: &TextEvent) -> Option<TranslationLine> {
        let src_lang =
            self.cfg.languages.as_ref().and_then(|l| l.source.clone()).or_else(|| self.cfg.source_language.clone());
        let text = normalize(&ev.text, src_lang.as_deref());
        if text.is_empty() || text.chars().count() > MAX_LINE_CHARS {
            return None;
        }
        if self.cfg.dedupe && !self.dedupe.lock().unwrap().accept(ev.origin, &text) {
            return None;
        }
        let line = TranslationLine {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: self.cfg.session_id.clone(),
            game_id: self.cfg.game_id,
            origin: ev.origin,
            source_text: text,
            translated_text: None,
            source_language: src_lang,
            target_language: self.cfg.languages.as_ref().map(|l| l.target.clone()),
            provider_id: self.cfg.provider.as_ref().map(|p| p.id().to_string()),
            error: None,
            ts_ms: vnl_core::now_ms(),
        };
        self.events.emit(AppEvent::Translation(line.clone()));
        Some(line)
    }

    /// Translates an accepted line, emits the result and stores history.
    pub async fn complete(&self, mut line: TranslationLine) -> TranslationLine {
        if let (Some(provider), Some(langs)) = (&self.cfg.provider, &self.cfg.languages) {
            match translate_with_glossary(provider.as_ref(), &line.source_text, langs, &self.cfg.glossary).await {
                Ok(t) => line.translated_text = Some(t),
                Err(e) => line.error = Some(e.message),
            }
            self.events.emit(AppEvent::Translation(line.clone()));
        }
        if self.cfg.keep_history
            && let Some(store) = &self.store
            && let Err(e) = store.history_put(&line)
        {
            tracing::warn!(error = %e, "cannot store translation history");
        }
        line
    }

    pub async fn process(&self, ev: TextEvent) -> Option<TranslationLine> {
        let line = self.accept(&ev)?;
        Some(self.complete(line).await)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::types::{GlossaryKind, ProviderKind};
    use async_trait::async_trait;
    use vnl_core::events::RecordingSink;

    /// Test double: wraps the text so tests can see exactly what was sent.
    pub struct FakeProvider {
        pub strategy: GlossaryStrategy,
        pub requests: Mutex<Vec<ProviderRequest>>,
        pub fail: bool,
    }

    impl FakeProvider {
        pub fn new(strategy: GlossaryStrategy) -> Self {
            Self { strategy, requests: Mutex::new(Vec::new()), fail: false }
        }
    }

    #[async_trait]
    impl TranslationProvider for FakeProvider {
        fn id(&self) -> &str {
            "fake"
        }
        fn kind(&self) -> ProviderKind {
            ProviderKind::Local
        }
        fn sends_data_remotely(&self) -> bool {
            false
        }
        fn glossary_strategy(&self) -> GlossaryStrategy {
            self.strategy
        }
        async fn translate(&self, req: &ProviderRequest) -> Result<String, AppError> {
            self.requests.lock().unwrap().push(req.clone());
            if self.fail {
                return Err(AppError::new(vnl_core::ErrorCode::Network, "provider down"));
            }
            Ok(format!("<{}:{}>", req.target_language, req.text))
        }
    }

    fn g(src: &str, tgt: &str, kind: GlossaryKind) -> GlossaryEntry {
        GlossaryEntry { id: 1, game_id: None, kind, source: src.into(), target: tgt.into(), note: None, enabled: true }
    }

    fn langs() -> ResolvedLanguages {
        ResolvedLanguages { source: Some("ja".into()), target: "de".into() }
    }

    #[test]
    fn normalization() {
        assert_eq!(normalize("  「今日は\r\n いい天気」\u{200B}\t", Some("ja")), "「今日はいい天気」");
        assert_eq!(normalize("Hello\nworld  !", Some("en")), "Hello world !");
        assert_eq!(normalize("ああいいううええ", Some("ja")), "あいうえ");
        assert_eq!(normalize("ええ、そうね", Some("ja")), "ええ、そうね");
        assert_eq!(normalize("\u{0007}\n \n", Some("ja")), "");
    }

    #[test]
    fn dedupe_rules() {
        let mut d = Deduper::new(Duration::from_secs(4));
        assert!(d.accept(TextOrigin::Hook, "a"));
        assert!(!d.accept(TextOrigin::Hook, "a"));
        assert!(!d.accept(TextOrigin::Ocr, "a"), "cross-source duplicate within window");
        assert!(d.accept(TextOrigin::Hook, "b"));
        assert!(d.accept(TextOrigin::Ocr, "c"));
        assert!(!d.accept(TextOrigin::Ocr, "c"));
    }

    #[test]
    fn placeholders_roundtrip() {
        let gl = vec![g("先輩", "Senpai", GlossaryKind::Term), g("先輩方", "die Senpais", GlossaryKind::Term)];
        let (t, used) = apply_placeholders("先輩方と先輩", &gl);
        assert_eq!(t, "ZX0Zと ZX1Z".replace(' ', ""));
        assert_eq!(restore_placeholders("zx0z und ZX 1Z", &used), "die Senpais und Senpai");
    }

    #[tokio::test]
    async fn glossary_strategies() {
        let gl = vec![g("美咲", "Misaki", GlossaryKind::CharacterName), g("部長", "Präsidentin", GlossaryKind::Term)];
        let p = FakeProvider::new(GlossaryStrategy::Placeholders);
        let out = translate_with_glossary(&p, "美咲は部長だ", &langs(), &gl).await.unwrap();
        assert_eq!(out, "<de:MisakiはPräsidentinだ>");
        assert_eq!(p.requests.lock().unwrap()[0].text, "ZX0ZはZX1Zだ");

        let p = FakeProvider::new(GlossaryStrategy::PromptHints);
        let out = translate_with_glossary(&p, "美咲が来た", &langs(), &gl).await.unwrap();
        let req = p.requests.lock().unwrap()[0].clone();
        assert_eq!(req.text, "美咲が来た");
        assert_eq!(req.glossary.len(), 1, "only relevant entries are hinted");
        assert_eq!(req.glossary[0].kind, GlossaryKind::CharacterName);
        // Leftover source names are replaced after translation.
        assert_eq!(out, "<de:Misakiが来た>");
    }

    #[tokio::test]
    async fn pipeline_emits_twice_and_stores_history() {
        let sink = RecordingSink::new();
        let store = Arc::new(TranslationStore::open_in_memory().unwrap());
        let provider = Arc::new(FakeProvider::new(GlossaryStrategy::Placeholders));
        let pipe = TranslationPipeline::new(
            PipelineConfig {
                session_id: Some("s1".into()),
                game_id: Some(7),
                provider: Some(provider.clone()),
                languages: Some(langs()),
                source_language: None,
                glossary: vec![],
                keep_history: true,
                dedupe: true,
            },
            Arc::new(sink.clone()),
            Some(store.clone()),
        );
        let line = pipe.process(TextEvent::new(TextOrigin::Hook, "こんにちは\n世界")).await.unwrap();
        assert_eq!(line.translated_text.as_deref(), Some("<de:こんにちは世界>"));
        assert!(pipe.process(TextEvent::new(TextOrigin::Hook, "こんにちは世界")).await.is_none(), "duplicate");
        let events: Vec<TranslationLine> = sink
            .events()
            .into_iter()
            .filter_map(|e| if let AppEvent::Translation(l) = e { Some(l) } else { None })
            .collect();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].id, events[1].id);
        assert!(events[0].translated_text.is_none());
        assert_eq!(events[1].target_language.as_deref(), Some("de"));
        let h = store.history_list(Some(7), 10).unwrap();
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].translated_text.as_deref(), Some("<de:こんにちは世界>"));
    }

    #[tokio::test]
    async fn pipeline_errors_and_extraction_only_and_no_history() {
        let sink = RecordingSink::new();
        let store = Arc::new(TranslationStore::open_in_memory().unwrap());
        let mut failing = FakeProvider::new(GlossaryStrategy::PromptHints);
        failing.fail = true;
        let pipe = TranslationPipeline::new(
            PipelineConfig {
                session_id: None,
                game_id: Some(1),
                provider: Some(Arc::new(failing)),
                languages: Some(langs()),
                source_language: None,
                glossary: vec![],
                keep_history: false,
                dedupe: false,
            },
            Arc::new(sink.clone()),
            Some(store.clone()),
        );
        let line = pipe.process(TextEvent::new(TextOrigin::Ocr, "テスト")).await.unwrap();
        assert_eq!(line.error.as_deref(), Some("provider down"));
        assert!(store.history_list(None, 10).unwrap().is_empty(), "keepHistory=false");

        let sink2 = RecordingSink::new();
        let pipe = TranslationPipeline::new(
            PipelineConfig {
                session_id: None,
                game_id: None,
                provider: None,
                languages: None,
                source_language: Some("ja".into()),
                glossary: vec![],
                keep_history: true,
                dedupe: true,
            },
            Arc::new(sink2.clone()),
            Some(store.clone()),
        );
        let line = pipe.process(TextEvent::new(TextOrigin::Clipboard, "原文\nのみ")).await.unwrap();
        assert_eq!(line.source_text, "原文のみ");
        assert!(line.translated_text.is_none() && line.error.is_none());
        assert_eq!(sink2.events().len(), 1, "source-only emission");
    }
}
