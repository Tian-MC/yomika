//! Small helpers shared by the runtime modules.

use std::io::Write;
use std::path::Path;

use vnl_core::AppError;

/// Write a file atomically (temp file + rename) creating parent directories.
pub fn write_atomic(path: &Path, data: &[u8]) -> Result<(), AppError> {
    let parent = path.parent().ok_or_else(|| AppError::invalid(format!("invalid path {}", path.display())))?;
    std::fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(
        ".{}.tmp-{}",
        path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        std::process::id()
    ));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Filesystem/id-friendly slug: ASCII alphanumerics kept (lowercased), other
/// characters collapsed to `-`. Falls back to `"prefix"` when nothing is left.
pub fn slugify(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if c == '.' || c == '_' {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches(|c| c == '-' || c == '.').to_string();
    if out.is_empty() { "prefix".to_string() } else { out }
}

/// Eight hex characters from a random UUID.
pub fn short_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..8].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs() {
        assert_eq!(slugify("My Game: ゲーム 2"), "my-game-2");
        assert_eq!(slugify("ゲーム"), "prefix");
        assert_eq!(slugify("..GE_Proton.9"), "ge_proton.9");
    }

    #[test]
    fn atomic_write_creates_parents() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("a/b/c.json");
        write_atomic(&p, b"{}").unwrap();
        write_atomic(&p, b"[1]").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "[1]");
        assert_eq!(std::fs::read_dir(p.parent().unwrap()).unwrap().count(), 1);
    }
}
