//! Text and path normalization helpers.

use std::path::{Component, Path, PathBuf};

use unicode_normalization::UnicodeNormalization;

/// Normalize text for case-insensitive search: NFKC (full-width -> ASCII,
/// half-width katakana -> full-width), Unicode lowercase, hiragana folded to
/// katakana so `えばー` matches `エバー`.
pub fn fold(s: &str) -> String {
    s.nfkc().flat_map(char::to_lowercase).map(hiragana_to_katakana).collect()
}

fn hiragana_to_katakana(c: char) -> char {
    match c {
        '\u{3041}'..='\u{3096}' | '\u{309D}' | '\u{309E}' => char::from_u32(c as u32 + 0x60).unwrap_or(c),
        _ => c,
    }
}

/// Fold and keep only letters/digits (any script). Used to compare folder
/// and executable names loosely: `"Ever17_the-out"` -> `"ever17theout"`.
pub fn compact(s: &str) -> String {
    fold(s).chars().filter(|c| c.is_alphanumeric()).collect()
}

/// Lexically normalize a path (resolve `.` and `..` without touching the FS).
pub fn clean_path(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in p.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Key used to detect duplicate executables: canonical path when the file
/// exists (resolves symlinks), lexically cleaned path otherwise.
pub fn path_key(p: &Path) -> String {
    let canonical = std::fs::canonicalize(p).unwrap_or_else(|_| clean_path(p));
    canonical.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_width_case_and_kana() {
        assert_eq!(fold("ＥＶＥＲ１７"), "ever17");
        assert_eq!(fold("えばー"), fold("エバー"));
        assert_eq!(fold("ｴﾊﾞｰ"), fold("エバー"));
        assert_eq!(compact("Ever17 -the out of infinity-"), "ever17theoutofinfinity");
        assert_eq!(compact("[KID] 夏色"), "kid夏色");
    }

    #[test]
    fn clean_path_resolves_dots() {
        assert_eq!(clean_path(Path::new("/a/./b/../c.exe")), PathBuf::from("/a/c.exe"));
    }
}
