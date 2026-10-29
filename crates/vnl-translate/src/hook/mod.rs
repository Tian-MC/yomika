//! Hook-based text extraction (`HookAdapter`s).
//!
//! Backends (`HookConfig.backend`):
//! - `lunahost` (default): [`lunahost::LunaHostAdapter`], an external CLI
//!   host run inside the game's prefix.
//! - `clipboard`: [`crate::clipboard::ClipboardSource`] — for users running
//!   any hooker (LunaTranslator, Textractor GUI, ...) that copies text.

pub mod lunahost;
pub mod pe;
pub mod process;
pub mod protocol;
mod threads;

use std::path::Path;

use crate::source::TextSource;
use crate::types::BackendInfo;

/// A [`TextSource`] that attaches to a game process.
pub trait HookAdapter: TextSource {
    fn backend_id(&self) -> &str;
}

pub fn hook_backends(tools_dir: &Path) -> Vec<BackendInfo> {
    vec![lunahost::backend_info(tools_dir), crate::clipboard::backend_info()]
}
