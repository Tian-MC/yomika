//! Language list and source/target resolution.
//!
//! There is deliberately **no** built-in target language: when nothing is
//! configured the caller gets an `InvalidInput` error asking the user to pick
//! one.

use vnl_core::{AppError, TranslationConfig, TranslationDefaults};

use crate::types::Language;

const LANGUAGES: &[(&str, &str, &str)] = &[
    ("ja", "Japanese", "日本語"),
    ("en", "English", "English"),
    ("en-US", "English (US)", "English (US)"),
    ("en-GB", "English (UK)", "English (UK)"),
    ("zh-Hans", "Chinese (Simplified)", "简体中文"),
    ("zh-Hant", "Chinese (Traditional)", "繁體中文"),
    ("ko", "Korean", "한국어"),
    ("fr", "French", "Français"),
    ("de", "German", "Deutsch"),
    ("es", "Spanish", "Español"),
    ("it", "Italian", "Italiano"),
    ("pt-BR", "Portuguese (Brazil)", "Português (Brasil)"),
    ("pt-PT", "Portuguese (Portugal)", "Português (Portugal)"),
    ("ru", "Russian", "Русский"),
    ("uk", "Ukrainian", "Українська"),
    ("pl", "Polish", "Polski"),
    ("cs", "Czech", "Čeština"),
    ("nl", "Dutch", "Nederlands"),
    ("sv", "Swedish", "Svenska"),
    ("fi", "Finnish", "Suomi"),
    ("da", "Danish", "Dansk"),
    ("nb", "Norwegian Bokmål", "Norsk bokmål"),
    ("hu", "Hungarian", "Magyar"),
    ("ro", "Romanian", "Română"),
    ("el", "Greek", "Ελληνικά"),
    ("tr", "Turkish", "Türkçe"),
    ("ar", "Arabic", "العربية"),
    ("he", "Hebrew", "עברית"),
    ("vi", "Vietnamese", "Tiếng Việt"),
    ("th", "Thai", "ไทย"),
    ("id", "Indonesian", "Bahasa Indonesia"),
    ("ms", "Malay", "Bahasa Melayu"),
    ("hi", "Hindi", "हिन्दी"),
];

pub fn languages() -> Vec<Language> {
    LANGUAGES
        .iter()
        .map(|(code, name, native)| Language {
            code: code.to_string(),
            name: name.to_string(),
            native_name: native.to_string(),
        })
        .collect()
}

/// English name for a code (used in LLM prompts); falls back to the code.
pub fn language_name(code: &str) -> String {
    LANGUAGES
        .iter()
        .find(|(c, _, _)| c.eq_ignore_ascii_case(code))
        .or_else(|| {
            let primary = primary_subtag(code);
            LANGUAGES.iter().find(|(c, _, _)| c.eq_ignore_ascii_case(primary))
        })
        .map(|(_, name, _)| name.to_string())
        .unwrap_or_else(|| code.to_string())
}

pub fn primary_subtag(code: &str) -> &str {
    code.split(['-', '_']).next().unwrap_or(code)
}

/// Light BCP-47 shape check: `ll`, `lll`, `ll-Xxxx`, `ll-RR`, `ll-Xxxx-RR`, or `auto` (source only).
pub fn validate_code(code: &str) -> Result<String, AppError> {
    let code = code.trim();
    let ok = !code.is_empty()
        && code.len() <= 20
        && code.split('-').enumerate().all(|(i, part)| {
            let len_ok = if i == 0 { (2..=3).contains(&part.len()) } else { (2..=8).contains(&part.len()) };
            len_ok && part.chars().all(|c| c.is_ascii_alphanumeric())
        });
    if ok { Ok(code.to_string()) } else { Err(AppError::invalid(format!("'{code}' is not a valid language code"))) }
}

pub const NO_TARGET_MESSAGE: &str = "No target language selected. Choose the language to translate into in Settings → Translation (or in this game's translation settings).";

/// Resolved languages for one translation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedLanguages {
    /// `None` = let the provider auto-detect.
    pub source: Option<String>,
    pub target: String,
}

/// Resolution order: explicit request → per-game config → app defaults.
pub fn resolve_languages(
    requested_source: Option<&str>,
    requested_target: Option<&str>,
    game: Option<&TranslationConfig>,
    defaults: Option<&TranslationDefaults>,
) -> Result<ResolvedLanguages, AppError> {
    let non_empty = |s: Option<&str>| s.map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    let target = non_empty(requested_target)
        .or_else(|| non_empty(game.and_then(|g| g.target_language.as_deref())))
        .or_else(|| non_empty(defaults.and_then(|d| d.target_language.as_deref())))
        .ok_or_else(|| AppError::invalid(NO_TARGET_MESSAGE))?;
    let target = validate_code(&target)?;
    if target.eq_ignore_ascii_case("auto") {
        return Err(AppError::invalid("'auto' cannot be used as the target language"));
    }
    let source = non_empty(requested_source)
        .or_else(|| non_empty(game.and_then(|g| g.source_language.as_deref())))
        .or_else(|| non_empty(defaults.map(|d| d.source_language.as_str())));
    let source = match source {
        Some(s) if s.eq_ignore_ascii_case("auto") => None,
        Some(s) => Some(validate_code(&s)?),
        None => None,
    };
    Ok(ResolvedLanguages { source, target })
}

/// True for scripts written without spaces between words (line joins use "").
pub fn is_cjk_like(code: Option<&str>) -> bool {
    matches!(code.map(primary_subtag), Some("ja" | "zh" | "th" | "ko" | "yue"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_target_is_invalid_input() {
        let err =
            resolve_languages(None, None, Some(&TranslationConfig::default()), Some(&TranslationDefaults::default()))
                .unwrap_err();
        assert_eq!(err.code, vnl_core::ErrorCode::InvalidInput);
        assert!(err.message.contains("target language"));
        // Empty strings count as unset.
        assert!(resolve_languages(None, Some("  "), None, None).is_err());
    }

    #[test]
    fn precedence_request_game_defaults() {
        let defaults =
            TranslationDefaults { source_language: "ja".into(), target_language: Some("de".into()), provider_id: None };
        let game = TranslationConfig { target_language: Some("fr".into()), ..Default::default() };
        let r = resolve_languages(None, None, Some(&game), Some(&defaults)).unwrap();
        assert_eq!(r, ResolvedLanguages { source: Some("ja".into()), target: "fr".into() });
        let r = resolve_languages(Some("auto"), Some("pt-BR"), Some(&game), Some(&defaults)).unwrap();
        assert_eq!(r, ResolvedLanguages { source: None, target: "pt-BR".into() });
        let r = resolve_languages(None, None, None, Some(&defaults)).unwrap();
        assert_eq!(r.target, "de");
    }

    #[test]
    fn rejects_garbage_codes() {
        assert!(resolve_languages(None, Some("english please"), None, None).is_err());
        assert!(resolve_languages(None, Some("auto"), None, None).is_err());
        assert!(validate_code("zh-Hant").is_ok());
        assert!(validate_code("x").is_err());
    }

    #[test]
    fn names() {
        assert_eq!(language_name("ja"), "Japanese");
        assert_eq!(language_name("en-AU"), "English");
        assert_eq!(language_name("tlh"), "tlh");
        assert!(languages().iter().any(|l| l.code == "zh-Hans"));
    }
}
