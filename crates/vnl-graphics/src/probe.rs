//! System access abstraction used by every detector in this crate.
//!
//! All filesystem lookups of *system* paths (`/sys`, `/proc`, `/etc`, `/usr`)
//! go through [`Probe::sys_path`], which re-roots them under [`Probe::root`] so
//! tests can point the detectors at a fixture tree. Environment variables are
//! a snapshot map, and executables are resolved only through the snapshot's
//! `PATH` (plus explicit extra candidates), so tests can use a fake `PATH`
//! directory full of shell scripts.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Where a tool was found and which version it reported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolInfo {
    pub path: PathBuf,
    pub version: Option<String>,
}

/// Captured output of a helper command.
#[derive(Debug, Clone, Default)]
pub struct CmdOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

impl CmdOutput {
    /// stdout followed by stderr (many tools print versions on stderr).
    pub fn combined(&self) -> String {
        let mut s = self.stdout.clone();
        if !self.stderr.is_empty() {
            if !s.is_empty() && !s.ends_with('\n') {
                s.push('\n');
            }
            s.push_str(&self.stderr);
        }
        s
    }
}

#[derive(Debug, Clone)]
pub struct Probe {
    /// Root for system paths. `/` in production.
    pub root: PathBuf,
    /// Environment snapshot.
    pub env: BTreeMap<String, String>,
    /// Allow running helper programs (`--version`, `lspci`, ...).
    pub run_commands: bool,
    /// Timeout for each helper program.
    pub command_timeout: Duration,
}

impl Probe {
    /// Probe the real system with the current process environment.
    pub fn system() -> Self {
        Self {
            root: PathBuf::from("/"),
            env: std::env::vars().collect(),
            run_commands: true,
            command_timeout: Duration::from_secs(5),
        }
    }

    /// Probe a fixture tree with an explicit environment (tests).
    pub fn fixture(root: impl Into<PathBuf>, env: BTreeMap<String, String>) -> Self {
        Self { root: root.into(), env, run_commands: true, command_timeout: Duration::from_secs(5) }
    }

    /// Non-empty environment variable.
    pub fn var(&self, key: &str) -> Option<&str> {
        self.env.get(key).map(String::as_str).filter(|v| !v.trim().is_empty())
    }

    pub fn home(&self) -> Option<PathBuf> {
        self.var("HOME").map(PathBuf::from)
    }

    /// `$XDG_CONFIG_HOME` or `~/.config`.
    pub fn config_home(&self) -> Option<PathBuf> {
        self.var("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| self.home().map(|h| h.join(".config")))
    }

    /// `$XDG_DATA_HOME` or `~/.local/share`.
    pub fn data_home(&self) -> Option<PathBuf> {
        self.var("XDG_DATA_HOME").map(PathBuf::from).or_else(|| self.home().map(|h| h.join(".local/share")))
    }

    /// Re-root an absolute system path (e.g. `/sys/class/drm`) under `root`.
    pub fn sys_path(&self, abs: impl AsRef<Path>) -> PathBuf {
        let abs = abs.as_ref();
        let rel = abs.strip_prefix("/").unwrap_or(abs);
        self.root.join(rel)
    }

    pub fn read_sys(&self, abs: impl AsRef<Path>) -> Option<String> {
        std::fs::read_to_string(self.sys_path(abs)).ok()
    }

    pub fn inside_flatpak(&self) -> bool {
        self.sys_path("/.flatpak-info").is_file()
    }

    pub fn path_dirs(&self) -> Vec<PathBuf> {
        self.var("PATH").map(|p| std::env::split_paths(p).collect()).unwrap_or_default()
    }

    /// Resolve an executable through the snapshot `PATH`, then through
    /// `extra` candidate paths (absolute; checked as-is).
    pub fn find_tool(&self, name: &str, extra: &[PathBuf]) -> Option<PathBuf> {
        if let Some(path) = self.var("PATH")
            && let Ok(found) = which::which_in(name, Some(path), &self.root)
        {
            return Some(found);
        }
        extra.iter().find(|p| is_executable(p)).cloned()
    }

    /// Run a helper program with a timeout. Returns `None` when commands are
    /// disabled, the program cannot be spawned, or it timed out.
    pub fn run(&self, program: &Path, args: &[&str]) -> Option<CmdOutput> {
        if !self.run_commands {
            return None;
        }
        let mut cmd = Command::new(program);
        cmd.args(args)
            .env_clear()
            .envs(&self.env)
            .env("LC_ALL", "C")
            .env("LANG", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        run_with_timeout(cmd, self.command_timeout)
    }
}

/// Spawn `cmd`, collect stdout/stderr, kill it after `timeout`.
pub fn run_with_timeout(mut cmd: Command, timeout: Duration) -> Option<CmdOutput> {
    let mut child = cmd.spawn().ok()?;
    let mut out = child.stdout.take()?;
    let mut err = child.stderr.take()?;
    let t_out = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = out.read_to_end(&mut buf);
        buf
    });
    let t_err = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = err.read_to_end(&mut buf);
        buf
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(_) => break None,
        }
    };
    let stdout = t_out.join().unwrap_or_default();
    let stderr = t_err.join().unwrap_or_default();
    let status = status?;
    Some(CmdOutput {
        success: status.success(),
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    })
}

pub fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
}

/// Pull a version number out of typical `--version` output:
/// `wine-11.20` -> `11.20`, `gamescope version 3.17.0 (gcc ...)` -> `3.17.0`,
/// `protontricks (1.14.1)` -> `1.14.1`, `20260125 - sha256sum: ...` -> `20260125`.
pub fn extract_version(text: &str) -> Option<String> {
    for line in text.lines() {
        for raw in line.split_whitespace() {
            let token = raw.trim_matches(|c: char| "()[],;:'\"".contains(c));
            let token =
                token.strip_prefix('v').filter(|t| t.starts_with(|c: char| c.is_ascii_digit())).unwrap_or(token);
            if looks_like_version(token) {
                return Some(token.to_string());
            }
            // `wine-11.20`, `umu-launcher-1.2.6`
            if let Some(idx) = token.find(|c: char| c.is_ascii_digit())
                && idx > 0
                && token[..idx].ends_with('-')
                && looks_like_version(&token[idx..])
            {
                return Some(token[idx..].to_string());
            }
        }
    }
    None
}

fn looks_like_version(t: &str) -> bool {
    let starts_digit = t.starts_with(|c: char| c.is_ascii_digit());
    let ok_chars = t.chars().all(|c| c.is_ascii_alphanumeric() || ".-_+~".contains(c));
    starts_digit && ok_chars && (t.contains('.') || (t.len() >= 6 && t.chars().all(|c| c.is_ascii_digit())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_extraction() {
        assert_eq!(extract_version("wine-11.20").as_deref(), Some("11.20"));
        assert_eq!(extract_version("gamescope version 3.17.0 (gcc 16.2.1)").as_deref(), Some("3.17.0"));
        assert_eq!(extract_version("protontricks (1.14.1)").as_deref(), Some("1.14.1"));
        assert_eq!(extract_version("Executing cd /usr/bin\n20260125 - sha256sum: 43").as_deref(), Some("20260125"));
        assert_eq!(extract_version("ollama version is 0.40.11").as_deref(), Some("0.40.11"));
        assert_eq!(extract_version("Version: ImageMagick 7.1.2-34 Q16-HDRI x86_64").as_deref(), Some("7.1.2-34"));
        assert_eq!(extract_version("wl-clipboard 2.3.0").as_deref(), Some("2.3.0"));
        assert_eq!(extract_version("upscale v1.1.4").as_deref(), Some("1.1.4"));
        assert_eq!(extract_version("no digits here"), None);
    }

    #[test]
    fn timeout_kills_slow_command() {
        let mut cmd = Command::new("sleep");
        cmd.arg("5").stdout(Stdio::piped()).stderr(Stdio::piped());
        let start = Instant::now();
        assert!(run_with_timeout(cmd, Duration::from_millis(200)).is_none());
        assert!(start.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn sys_path_reroots() {
        let p = Probe::fixture("/tmp/fx", BTreeMap::new());
        assert_eq!(p.sys_path("/sys/class/drm"), PathBuf::from("/tmp/fx/sys/class/drm"));
    }
}
