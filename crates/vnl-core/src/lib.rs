//! Shared contract for every VN Launcher subsystem.
//!
//! This crate is owned by the orchestrator. Subsystem crates (`vnl-runtime`,
//! `vnl-library`, `vnl-graphics`, `vnl-translate`) depend on it and must not
//! depend on each other; cross-subsystem wiring happens in `src-tauri`.
//!
//! Serialization conventions (mirrored in `src/contract/*.ts`):
//! - structs use `camelCase` field names;
//! - unit enums serialize as `camelCase` strings;
//! - data-carrying enums are internally tagged with `"type"`;
//! - timestamps are Unix epoch **seconds** (`i64`) unless the field name ends in `Ms`.

pub mod error;
pub mod events;
pub mod game;
pub mod paths;
pub mod process;
pub mod runtime;
pub mod settings;

pub use error::{AppError, ErrorCode};
pub use events::{
    AppEvent, EventSink, LogEntry, LogLevel, NullSink, TaskProgress, TaskState, TextOrigin, TranslationLine,
};
pub use game::{Game, GameId, GameMetadata, GameQuery, GameSort, PlayStatus};
pub use paths::AppPaths;
pub use process::{CommandSpec, PrefixCommandBuilder};
pub use runtime::{
    AuxProcessInfo, PrefixArch, PrefixInfo, PrefixKind, Runner, RunnerKind, RunnerSource, SessionId, SessionInfo,
    SessionState,
};
pub use settings::*;

/// Current Unix time in seconds.
pub fn now_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Current Unix time in milliseconds.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}
