use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A fully-resolved process invocation. Built by subsystems, composed by
/// wrappers (e.g. gamescope) and finally spawned by `vnl-runtime`.
///
/// Arguments are passed as an argv vector — never through a shell — so paths
/// with spaces or non-ASCII characters are safe.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    /// Variables set on top of the inherited environment.
    pub env: BTreeMap<String, String>,
    /// Variables removed from the inherited environment.
    pub env_remove: Vec<String>,
    pub cwd: Option<PathBuf>,
}

impl CommandSpec {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self { program: program.into(), ..Default::default() }
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    pub fn cwd(mut self, dir: impl Into<PathBuf>) -> Self {
        self.cwd = Some(dir.into());
        self
    }

    /// Wrap this command: `wrapper [wrapper_args...] <self.program> <self.args...>`.
    /// Environment and cwd are preserved.
    pub fn wrapped_by(self, wrapper: impl Into<PathBuf>, wrapper_args: Vec<String>) -> Self {
        let mut args = wrapper_args;
        args.push(self.program.to_string_lossy().into_owned());
        args.extend(self.args);
        Self { program: wrapper.into(), args, env: self.env, env_remove: self.env_remove, cwd: self.cwd }
    }

    /// Build a `std::process::Command` (no shell involved).
    pub fn to_std_command(&self) -> std::process::Command {
        let mut cmd = std::process::Command::new(&self.program);
        cmd.args(&self.args);
        for key in &self.env_remove {
            cmd.env_remove(key);
        }
        cmd.envs(&self.env);
        if let Some(cwd) = &self.cwd {
            cmd.current_dir(cwd);
        }
        cmd
    }

    /// Shell-quoted representation for logs and the UI. Display only.
    pub fn preview(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(cwd) = &self.cwd {
            parts.push(format!("cd {} &&", quote(&cwd.to_string_lossy())));
        }
        for (k, v) in &self.env {
            parts.push(format!("{k}={}", quote(v)));
        }
        parts.push(quote(&self.program.to_string_lossy()));
        parts.extend(self.args.iter().map(|a| quote(a)));
        parts.join(" ")
    }
}

fn quote(s: &str) -> String {
    shlex::try_quote(s).map(|c| c.into_owned()).unwrap_or_else(|_| format!("{s:?}"))
}

/// Builds commands that run a Windows executable inside a specific game's
/// prefix with the same runner/environment as the game (e.g. a text-hook host).
/// Implemented in `src-tauri` on top of `vnl-runtime`; consumed by
/// `vnl-translate` so that crate does not depend on the runtime crate.
pub trait PrefixCommandBuilder: Send + Sync {
    fn build(&self, exe: &Path, args: &[String], cwd: Option<&Path>) -> Result<CommandSpec, crate::AppError>;

    /// The prefix path (WINEPREFIX, or Proton compat-data dir).
    fn prefix_path(&self) -> PathBuf;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapping_keeps_argv_boundaries() {
        let spec = CommandSpec::new("/opt/proton/proton")
            .args(["run", "/games/My Game/ゲーム.exe"])
            .env("WINEPREFIX", "/p")
            .cwd("/games/My Game");
        let wrapped = spec.wrapped_by("gamescope", vec!["-w".into(), "1280".into(), "--".into()]);
        assert_eq!(wrapped.program, PathBuf::from("gamescope"));
        assert_eq!(wrapped.args, vec!["-w", "1280", "--", "/opt/proton/proton", "run", "/games/My Game/ゲーム.exe"]);
        assert_eq!(wrapped.env.get("WINEPREFIX").unwrap(), "/p");
        assert!(wrapped.preview().contains("'/games/My Game/ゲーム.exe'"));
    }
}
