use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
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
        let mut environment: BTreeMap<OsString, OsString> = std::env::vars_os().collect();
        for key in &self.env_remove {
            environment.remove(OsStr::new(key));
        }
        environment.extend(self.env.iter().map(|(k, v)| (k.into(), v.into())));
        let mut cmd = external_command(&self.program, environment);
        cmd.args(&self.args);
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

/// Build an external process with its own environment snapshot. AppImage's
/// private loader paths belong to Yomika, not host Python, Wine or Gamescope.
/// The current process environment is never modified.
pub fn external_command(
    program: impl AsRef<OsStr>,
    environment: impl IntoIterator<Item = (OsString, OsString)>,
) -> std::process::Command {
    let mut environment: BTreeMap<OsString, OsString> = environment.into_iter().collect();
    if let Some(root) = environment.get(OsStr::new("APPDIR")).map(PathBuf::from)
        && root.is_absolute()
        && root.parent().is_some()
    {
        let root = normalize_path(&root);
        if let Some(value) = environment.get(OsStr::new("LD_LIBRARY_PATH")) {
            let paths: Vec<_> = std::env::split_paths(value).filter(|p| !is_bundled(p, &root)).collect();
            if paths.is_empty() {
                environment.remove(OsStr::new("LD_LIBRARY_PATH"));
            } else if let Ok(value) = std::env::join_paths(paths) {
                environment.insert("LD_LIBRARY_PATH".into(), value);
            }
        }
        if let Some(value) = environment.get(OsStr::new("LD_PRELOAD")) {
            // The ELF loader accepts both colons and spaces in LD_PRELOAD.
            // Work on bytes so non-UTF-8 user paths survive unchanged.
            use std::os::unix::ffi::{OsStrExt, OsStringExt};
            let entries: Vec<_> = value
                .as_bytes()
                .split(|b| *b == b':' || b.is_ascii_whitespace())
                .filter(|p| !p.is_empty() && !is_bundled(Path::new(OsStr::from_bytes(p)), &root))
                .collect();
            let cleaned = OsString::from_vec(entries.join(&b' '));
            if cleaned.is_empty() {
                environment.remove(OsStr::new("LD_PRELOAD"));
            } else {
                environment.insert("LD_PRELOAD".into(), cleaned);
            }
        }
    }
    let mut cmd = std::process::Command::new(program);
    cmd.env_clear().envs(environment);
    cmd
}

fn is_bundled(path: &Path, root: &Path) -> bool {
    path.is_absolute() && normalize_path(path).starts_with(root)
}

fn normalize_path(path: &Path) -> PathBuf {
    if let Ok(path) = std::fs::canonicalize(path) {
        return path;
    }
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                result.pop();
            }
            other => result.push(other.as_os_str()),
        }
    }
    result
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

    fn snapshot(command: &std::process::Command) -> BTreeMap<OsString, OsString> {
        command.get_envs().filter_map(|(k, v)| v.map(|v| (k.into(), v.into()))).collect()
    }

    #[test]
    fn removes_bundle_libraries_and_preserves_user_environment() {
        let env = BTreeMap::from([
            ("APPDIR".into(), "/tmp/Yomika.AppDir".into()),
            (
                "LD_LIBRARY_PATH".into(),
                "/tmp/Yomika.AppDir/usr/lib:/opt/user/lib:/tmp/Yomika.AppDir-other/lib::$ORIGIN/lib".into(),
            ),
            (
                "LD_PRELOAD".into(),
                "/tmp/Yomika.AppDir/usr/lib/injected.so:/opt/mangohud.so gameoverlayrenderer.so".into(),
            ),
            ("WINEPREFIX".into(), "/home/user/prefix with spaces".into()),
            ("PROTONPATH".into(), "/opt/GE-Proton".into()),
            ("LC_ALL".into(), "ja_JP.UTF-8".into()),
        ]);
        let original = env.clone();
        let cmd = external_command("umu-run", env);
        let actual = snapshot(&cmd);
        assert_eq!(
            actual.get(OsStr::new("LD_LIBRARY_PATH")).unwrap(),
            "/opt/user/lib:/tmp/Yomika.AppDir-other/lib::$ORIGIN/lib"
        );
        assert_eq!(actual.get(OsStr::new("LD_PRELOAD")).unwrap(), "/opt/mangohud.so gameoverlayrenderer.so");
        for key in ["WINEPREFIX", "PROTONPATH", "LC_ALL"] {
            assert_eq!(actual.get(OsStr::new(key)), original.get(OsStr::new(key)));
        }
    }

    #[test]
    fn ordinary_or_invalid_appdir_environment_is_unchanged() {
        for appdir in [None, Some("relative/AppDir"), Some("/")] {
            let mut env = BTreeMap::from([
                ("LD_LIBRARY_PATH".into(), "/opt/user/lib".into()),
                ("LD_PRELOAD".into(), "one.so:two.so".into()),
            ]);
            if let Some(root) = appdir {
                env.insert("APPDIR".into(), root.into());
            }
            assert_eq!(snapshot(&external_command("wine", env.clone())), env);
        }
    }

    #[test]
    fn removes_empty_bundle_only_variables_and_normalizes_path_components() {
        let cmd = external_command(
            "gamescope",
            BTreeMap::from([
                ("APPDIR".into(), "/tmp/app".into()),
                ("LD_LIBRARY_PATH".into(), "/tmp/app/usr/../lib".into()),
                ("LD_PRELOAD".into(), "/tmp/app/lib/a.so /tmp/app/lib/b.so".into()),
            ]),
        );
        let env = snapshot(&cmd);
        assert!(!env.contains_key(OsStr::new("LD_LIBRARY_PATH")));
        assert!(!env.contains_key(OsStr::new("LD_PRELOAD")));
    }

    #[test]
    fn recognizes_symlinks_into_bundle_without_removing_sibling_directories() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let bundle = root.path().join("AppDir");
        let library = bundle.join("usr/lib");
        std::fs::create_dir_all(&library).unwrap();
        let alias = root.path().join("library-alias");
        symlink(&library, &alias).unwrap();
        let env = BTreeMap::from([
            ("APPDIR".into(), bundle.into_os_string()),
            ("LD_LIBRARY_PATH".into(), alias.into_os_string()),
        ]);
        assert!(!snapshot(&external_command("umu-run", env)).contains_key(OsStr::new("LD_LIBRARY_PATH")));
    }

    #[test]
    fn preserves_non_utf8_user_loader_paths() {
        use std::os::unix::ffi::OsStringExt;
        let user = OsString::from_vec(b"/opt/user-\xff.so".to_vec());
        let env = BTreeMap::from([("APPDIR".into(), "/tmp/app".into()), ("LD_PRELOAD".into(), user.clone())]);
        assert_eq!(snapshot(&external_command("umu-run", env)).get(OsStr::new("LD_PRELOAD")), Some(&user));
    }

    #[test]
    fn command_overrides_and_removals_are_applied_before_sanitizing() {
        let mut spec = CommandSpec::new("/usr/bin/env")
            .env("APPDIR", "/tmp/app")
            .env("LD_LIBRARY_PATH", "/tmp/app/lib:/opt/user/lib")
            .env("LD_PRELOAD", "user-preload.so");
        spec.env_remove.push("LD_PRELOAD".into());
        let actual = snapshot(&spec.to_std_command());
        assert_eq!(actual.get(OsStr::new("LD_LIBRARY_PATH")).unwrap(), "/opt/user/lib");
        // Explicit per-command overrides keep their existing precedence.
        assert_eq!(actual.get(OsStr::new("LD_PRELOAD")).unwrap(), "user-preload.so");
        spec.env.remove("LD_PRELOAD");
        assert!(!snapshot(&spec.to_std_command()).contains_key(OsStr::new("LD_PRELOAD")));
    }

    #[test]
    fn child_receives_clean_environment_without_changing_parent() {
        let before: BTreeMap<_, _> = std::env::vars_os().collect();
        let spec = CommandSpec::new("/usr/bin/env")
            .env("APPDIR", "/tmp/app")
            .env("LD_LIBRARY_PATH", "/tmp/app/lib:/opt/user/lib");
        let output = spec.to_std_command().output().unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).lines().any(|line| line == "LD_LIBRARY_PATH=/opt/user/lib"));
        assert_eq!(before, std::env::vars_os().collect());
    }

    #[test]
    #[ignore = "requires extracted AppImage libraries and system Python/umu/Gamescope"]
    fn real_appimage_libraries_do_not_break_system_tools() {
        let root = std::env::var("YOMIKA_APPIMAGE_TEST_DIR").expect("Set the extracted AppDir path");
        for (program, args) in [
            ("/usr/bin/python3", vec!["-c", "from compression import zstd"]),
            ("/usr/bin/umu-run", vec!["--version"]),
            ("/usr/bin/gamescope", vec!["--version"]),
        ] {
            let output = CommandSpec::new(program)
                .args(args)
                .env("APPDIR", &root)
                .env("LD_LIBRARY_PATH", format!("{root}/usr/lib"))
                .to_std_command()
                .output()
                .unwrap();
            assert!(output.status.success(), "{program}: {}", String::from_utf8_lossy(&output.stderr));
        }
    }

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
