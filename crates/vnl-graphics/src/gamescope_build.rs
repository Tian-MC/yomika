//! Guided, optional build of the gamescope-anime4k fork.
//!
//! The fork (<https://github.com/Et3rnos/gamescope-anime4k>, upstream PR
//! ValveSoftware/gamescope#2072) adds `-F anime4k`, an Anime4K 2x CNN UL
//! upscaler implemented as a Vulkan compute shader. Build instructions of the
//! repository (README): `git submodule update --init`, `meson setup build/`,
//! `ninja -C build/`, `meson install -C build/ --skip-subprojects`.
//!
//! This module follows them with these constraints:
//! * started only on request (`GamescopeBuilder::start`), never automatically;
//! * no root: the sources live in the cache dir, the result is installed into
//!   the private `tools_dir/gamescope-anime4k` prefix (`meson install
//!   --destdir`, then moved into place). `/usr/bin/gamescope` is never touched;
//! * the previous managed build is replaced only after the new one has been
//!   built and verified to list `anime4k` among its `-F` filters;
//! * progress goes out as `vnl://task` events, output as `gamescope-build`
//!   log entries (also saved to `logs/gamescope-build.log`), and the build can
//!   be cancelled (its whole process group is terminated).
//!
//! OpenVR, PipeWire, AVIF screenshots, the SDL2 backend and the benchmark tool
//! are disabled: they are unnecessary here and OpenVR needs CMake plus a
//! large submodule.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::Command;
use vnl_core::{AppError, AppPaths, ErrorCode, EventSink, LogEntry, LogLevel, TaskProgress, TaskState, now_secs};

use crate::deps::InstallHint;
use crate::gamescope_bin::{self, GamescopeBinary, GamescopeSource};
use crate::probe::Probe;

pub const REPO_URL: &str = "https://github.com/Et3rnos/gamescope-anime4k.git";
pub const BRANCH: &str = "master";
pub const TASK_ID: &str = "gamescope-build";
pub const LOG_SOURCE: &str = "gamescope-build";
const MANIFEST_FILE: &str = "installed.json";
/// Submodules the build needs (openvr is skipped on purpose).
const SUBMODULES: &[&str] = &[
    "subprojects/wlroots",
    "subprojects/libliftoff",
    "subprojects/vkroots",
    "subprojects/libdisplay-info",
    "src/reshade",
    "thirdparty/SPIRV-Headers",
];
const SUBMODULE_ATTEMPTS: usize = 3;

/// Id recorded in the manifest when [`UBO_ALIGNMENT_PATCH`] is part of the build.
pub const UBO_ALIGNMENT_PATCH_ID: &str = "ubo-alignment";
/// Small fix for the fork: `uploadBufferData` aligned uniform-buffer offsets to a
/// hard-coded 16 bytes, but descriptors must be bound at multiples of
/// `minUniformBufferOffsetAlignment` (64 on NVIDIA). The 16-byte Anime4K constants
/// then landed on invalid offsets, the shaders read zeros as image size and returned
/// early: a black image. Found with the Khronos validation layer
/// (VUID-VkWriteDescriptorSet-descriptorType-00327), see docs/GAMESCOPE_ANIME4K.md.
pub const UBO_ALIGNMENT_PATCH: &str = include_str!("../patches/gamescope-anime4k-ubo-alignment.patch");

// ------------------------------------------------------------ requirements

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RequirementKind {
    Tool,
    Library,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Requirement {
    /// Program name or pkg-config module.
    pub id: String,
    pub name: String,
    pub kind: RequirementKind,
    pub found: bool,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Requirements {
    pub items: Vec<Requirement>,
    pub missing: Vec<String>,
    /// Package-manager commands to install what is missing (shown, never run).
    pub hints: Vec<InstallHint>,
    pub ok: bool,
}

struct Req {
    id: &'static str,
    kind: RequirementKind,
    /// Minimum version for pkg-config modules.
    min: Option<&'static str>,
    arch: &'static str,
    debian: &'static str,
    fedora: &'static str,
    /// Alternative program names (tools only).
    alts: &'static [&'static str],
}

const fn tool(id: &'static str, arch: &'static str, debian: &'static str, fedora: &'static str) -> Req {
    Req { id, kind: RequirementKind::Tool, min: None, arch, debian, fedora, alts: &[] }
}

const fn lib(
    id: &'static str,
    min: Option<&'static str>,
    arch: &'static str,
    debian: &'static str,
    fedora: &'static str,
) -> Req {
    Req { id, kind: RequirementKind::Library, min, arch, debian, fedora, alts: &[] }
}

/// From gamescope's and wlroots' `meson.build` files (checked against the repository).
const REQUIREMENTS: &[Req] = &[
    tool("git", "git", "git", "git"),
    tool("meson", "meson", "meson", "meson"),
    tool("ninja", "ninja", "ninja-build", "ninja-build"),
    Req { alts: &["c++", "clang++"], ..tool("g++", "gcc", "g++", "gcc-c++") },
    tool("pkg-config", "pkgconf", "pkg-config", "pkgconf-pkg-config"),
    Req { alts: &["glslangValidator"], ..tool("glslang", "glslang", "glslang-tools", "glslang") },
    tool("wayland-scanner", "wayland", "libwayland-bin", "wayland-devel"),
    lib("vulkan", None, "vulkan-headers vulkan-icd-loader", "libvulkan-dev", "vulkan-headers vulkan-loader-devel"),
    lib("wayland-client", None, "wayland", "libwayland-dev", "wayland-devel"),
    lib("wayland-server", Some("1.21"), "wayland", "libwayland-dev", "wayland-devel"),
    lib("wayland-protocols", Some("1.17"), "wayland-protocols", "wayland-protocols", "wayland-protocols-devel"),
    lib("libdrm", Some("2.4.113"), "libdrm", "libdrm-dev", "libdrm-devel"),
    lib("xkbcommon", None, "libxkbcommon", "libxkbcommon-dev", "libxkbcommon-devel"),
    lib("pixman-1", None, "pixman", "libpixman-1-dev", "pixman-devel"),
    lib("libinput", None, "libinput", "libinput-dev", "libinput-devel"),
    lib("libseat", None, "seatd", "libseat-dev", "libseat-devel"),
    lib("libudev", None, "systemd-libs", "libudev-dev", "systemd-devel"),
    lib("libdecor-0", None, "libdecor", "libdecor-0-dev", "libdecor-devel"),
    lib("luajit", None, "luajit", "libluajit-5.1-dev", "luajit-devel"),
    lib("libcap", None, "libcap", "libcap-dev", "libcap-devel"),
    lib("x11", None, "libx11", "libx11-dev", "libX11-devel"),
    lib("xdamage", None, "libxdamage", "libxdamage-dev", "libXdamage-devel"),
    lib("xcomposite", None, "libxcomposite", "libxcomposite-dev", "libXcomposite-devel"),
    lib("xcursor", None, "libxcursor", "libxcursor-dev", "libXcursor-devel"),
    lib("xrender", None, "libxrender", "libxrender-dev", "libXrender-devel"),
    lib("xext", None, "libxext", "libxext-dev", "libXext-devel"),
    lib("xfixes", None, "libxfixes", "libxfixes-dev", "libXfixes-devel"),
    lib("xxf86vm", None, "libxxf86vm", "libxxf86vm-dev", "libXxf86vm-devel"),
    lib("xtst", None, "libxtst", "libxtst-dev", "libXtst-devel"),
    lib("xres", None, "libxres", "libxres-dev", "libXres-devel"),
    lib("xmu", None, "libxmu", "libxmu-dev", "libXmu-devel"),
    lib("xi", None, "libxi", "libxi-dev", "libXi-devel"),
    lib("xcb", None, "libxcb", "libxcb1-dev", "libxcb-devel"),
    lib("xcb-composite", None, "libxcb", "libxcb-composite0-dev", "libxcb-devel"),
    lib("xcb-render", None, "libxcb", "libxcb-render0-dev", "libxcb-devel"),
    lib("xcb-res", None, "libxcb", "libxcb-res0-dev", "libxcb-devel"),
    lib("xcb-xfixes", None, "libxcb", "libxcb-xfixes0-dev", "libxcb-devel"),
    lib("xcb-ewmh", None, "xcb-util-wm", "libxcb-ewmh-dev", "xcb-util-wm-devel"),
    lib("xcb-icccm", None, "xcb-util-wm", "libxcb-icccm4-dev", "xcb-util-wm-devel"),
];

/// Check build tools and development libraries. Nothing is installed.
pub fn check_requirements(probe: &Probe) -> Requirements {
    let pkg_config = probe.find_tool("pkg-config", &[]).or_else(|| probe.find_tool("pkgconf", &[]));
    let mut items = Vec::new();
    for r in REQUIREMENTS {
        let (found, detail) = match r.kind {
            RequirementKind::Tool => {
                let hit = std::iter::once(r.id).chain(r.alts.iter().copied()).find_map(|n| probe.find_tool(n, &[]));
                (hit.is_some(), hit.map(|p| p.display().to_string()))
            }
            RequirementKind::Library => match &pkg_config {
                None => (false, Some("pkg-config is missing".to_string())),
                Some(pc) => {
                    let version = probe
                        .run(pc, &["--modversion", r.id])
                        .filter(|o| o.success)
                        .map(|o| o.stdout.trim().to_string());
                    match (&version, r.min) {
                        (None, _) => (false, None),
                        (Some(v), Some(min)) => {
                            let ok = probe.run(pc, &["--atleast-version", min, r.id]).is_some_and(|o| o.success);
                            (ok, Some(if ok { v.clone() } else { format!("{v} found, {min}+ required") }))
                        }
                        (Some(v), None) => (true, Some(v.clone())),
                    }
                }
            },
        };
        items.push(Requirement { id: r.id.into(), name: r.id.into(), kind: r.kind, found, detail });
    }
    let missing: Vec<String> = items.iter().filter(|i| !i.found).map(|i| i.id.clone()).collect();
    let hints = if missing.is_empty() { Vec::new() } else { install_hints(&missing) };
    Requirements { ok: missing.is_empty(), items, missing, hints }
}

fn install_hints(missing: &[String]) -> Vec<InstallHint> {
    let pick = |f: fn(&Req) -> &'static str| -> String {
        let mut pkgs: Vec<&str> = Vec::new();
        for r in REQUIREMENTS.iter().filter(|r| missing.iter().any(|m| m == r.id)) {
            for p in f(r).split_whitespace() {
                if !pkgs.contains(&p) {
                    pkgs.push(p);
                }
            }
        }
        pkgs.join(" ")
    };
    vec![
        InstallHint { distro: "arch".into(), command: format!("sudo pacman -S --needed {}", pick(|r| r.arch)) },
        InstallHint { distro: "debian".into(), command: format!("sudo apt install {}", pick(|r| r.debian)) },
        InstallHint { distro: "fedora".into(), command: format!("sudo dnf install {}", pick(|r| r.fedora)) },
    ]
}

// ---------------------------------------------------------------- manifest

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallManifest {
    pub repo: String,
    pub branch: String,
    pub commit: String,
    pub built_at: i64,
    pub version: Option<String>,
    pub filters: Vec<String>,
    /// Source patches applied before building (e.g. [`UBO_ALIGNMENT_PATCH_ID`]).
    #[serde(default)]
    pub patches: Vec<String>,
}

impl InstallManifest {
    pub fn has_ubo_alignment_fix(&self) -> bool {
        self.patches.iter().any(|p| p == UBO_ALIGNMENT_PATCH_ID)
    }
}

/// Manifest of the managed build, if one is installed.
pub fn read_manifest(paths: &AppPaths) -> Option<InstallManifest> {
    let text = std::fs::read_to_string(gamescope_bin::managed_dir(paths).join(MANIFEST_FILE)).ok()?;
    serde_json::from_str(&text).ok()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedBuild {
    pub installed: bool,
    pub dir: PathBuf,
    pub binary: Option<GamescopeBinary>,
    pub manifest: Option<InstallManifest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildStatus {
    pub running: bool,
    pub task_id: Option<String>,
    /// State of the last (or current) build.
    pub state: Option<TaskState>,
    pub message: Option<String>,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub log_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildOptions {
    pub repo_url: String,
    pub branch: String,
    pub jobs: Option<usize>,
}

impl Default for BuildOptions {
    fn default() -> Self {
        Self { repo_url: REPO_URL.into(), branch: BRANCH.into(), jobs: None }
    }
}

/// Programs used by the build; overridable for tests.
#[derive(Debug, Clone)]
pub struct BuildTools {
    pub git: PathBuf,
    pub meson: PathBuf,
    pub ninja: PathBuf,
}

impl BuildTools {
    pub fn from_probe(probe: &Probe) -> Option<Self> {
        Some(Self {
            git: probe.find_tool("git", &[])?,
            meson: probe.find_tool("meson", &[])?,
            ninja: probe.find_tool("ninja", &[])?,
        })
    }
}

// ----------------------------------------------------------------- builder

enum BuildError {
    Cancelled,
    Failed(String),
}

impl From<std::io::Error> for BuildError {
    fn from(e: std::io::Error) -> Self {
        BuildError::Failed(e.to_string())
    }
}

fn fail<T>(msg: impl Into<String>) -> Result<T, BuildError> {
    Err(BuildError::Failed(msg.into()))
}

struct Active {
    cancel: Arc<AtomicBool>,
    pgid: Arc<AtomicI32>,
}

pub struct GamescopeBuilder {
    paths: AppPaths,
    events: Arc<dyn EventSink>,
    probe: Probe,
    tools: Mutex<Option<BuildTools>>,
    check_requirements: bool,
    active: Mutex<Option<Active>>,
    status: Mutex<BuildStatus>,
}

impl GamescopeBuilder {
    pub fn new(paths: &AppPaths, events: Arc<dyn EventSink>) -> Self {
        Self::with_probe(paths, events, Probe::system())
    }

    pub fn with_probe(paths: &AppPaths, events: Arc<dyn EventSink>, probe: Probe) -> Self {
        Self {
            paths: paths.clone(),
            events,
            probe,
            tools: Mutex::new(None),
            check_requirements: true,
            active: Mutex::new(None),
            status: Mutex::new(BuildStatus::default()),
        }
    }

    /// Tests: use these programs and skip the system library check.
    pub fn with_fake_tools(mut self, tools: BuildTools) -> Self {
        *self.tools.lock().unwrap() = Some(tools);
        self.check_requirements = false;
        self
    }

    pub fn requirements(&self) -> Requirements {
        check_requirements(&self.probe)
    }

    pub fn status(&self) -> BuildStatus {
        self.status.lock().unwrap().clone()
    }

    pub fn managed(&self) -> ManagedBuild {
        let dir = gamescope_bin::managed_dir(&self.paths);
        let manifest = read_manifest(&self.paths);
        let binary =
            gamescope_bin::inspect(&self.probe, &gamescope_bin::managed_binary(&self.paths), GamescopeSource::Managed);
        ManagedBuild { installed: binary.is_some(), dir, binary, manifest }
    }

    /// Delete the managed build (the launcher's own private directory only).
    pub fn remove_managed(&self) -> Result<(), AppError> {
        if self.active.lock().unwrap().is_some() {
            return Err(AppError::invalid("A gamescope build is running; cancel it first"));
        }
        let dir = gamescope_bin::managed_dir(&self.paths);
        if dir.exists() {
            if !dir.starts_with(self.paths.tools_dir()) {
                return Err(AppError::internal("refusing to delete a directory outside the tools dir"));
            }
            std::fs::remove_dir_all(&dir)?;
        }
        Ok(())
    }

    /// Request cancellation of the running build. Returns false when none runs.
    pub fn cancel(&self) -> bool {
        match self.active.lock().unwrap().as_ref() {
            Some(a) => {
                a.cancel.store(true, Ordering::SeqCst);
                let pgid = a.pgid.load(Ordering::SeqCst);
                if pgid > 0 {
                    let _ = killpg(Pid::from_raw(pgid), Signal::SIGTERM);
                }
                true
            }
            None => false,
        }
    }

    /// Start a build in the background and return its task id. Fails when a
    /// build already runs, a tool is missing or build dependencies are missing.
    pub fn start(self: &Arc<Self>, options: BuildOptions) -> Result<String, AppError> {
        let mut active = self.active.lock().unwrap();
        if active.is_some() {
            return Err(AppError::invalid("A gamescope build is already running"));
        }
        if self.check_requirements {
            let req = self.requirements();
            if !req.ok {
                return Err(AppError::new(
                    ErrorCode::MissingDependency,
                    format!("Missing build dependencies: {}", req.missing.join(", ")),
                )
                .with_details(
                    req.hints.iter().map(|h| format!("{}: {}", h.distro, h.command)).collect::<Vec<_>>().join("\n"),
                ));
            }
        }
        let tools = match self.tools.lock().unwrap().clone().or_else(|| BuildTools::from_probe(&self.probe)) {
            Some(t) => t,
            None => return Err(AppError::new(ErrorCode::MissingDependency, "git, meson and ninja are required")),
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let pgid = Arc::new(AtomicI32::new(0));
        *active = Some(Active { cancel: cancel.clone(), pgid: pgid.clone() });
        drop(active);

        let log_path = self.paths.logs_dir().join("gamescope-build.log");
        let _ = std::fs::create_dir_all(self.paths.logs_dir());
        let _ = std::fs::write(&log_path, "");
        *self.status.lock().unwrap() = BuildStatus {
            running: true,
            task_id: Some(TASK_ID.into()),
            state: Some(TaskState::Running),
            message: Some("Starting".into()),
            started_at: Some(now_secs()),
            finished_at: None,
            log_path: Some(log_path.clone()),
        };
        self.task(TaskState::Running, Some(0.0), "Starting");

        let this = self.clone();
        tokio::spawn(async move {
            let ctx = Ctx { cancel, pgid, log_path, last_emit: Mutex::new(Instant::now() - Duration::from_secs(1)) };
            let result = this.run(&ctx, &tools, &options).await;
            let (state, message) = match result {
                Ok(m) => {
                    (TaskState::Done, format!("Installed gamescope-anime4k {}", &m.commit[..m.commit.len().min(10)]))
                }
                Err(BuildError::Cancelled) => (TaskState::Failed, "Build cancelled".to_string()),
                Err(BuildError::Failed(e)) => (TaskState::Failed, e),
            };
            let level = if state == TaskState::Done { LogLevel::Info } else { LogLevel::Error };
            this.log(&ctx, level, &message);
            this.finish(state, &message);
        });
        Ok(TASK_ID.to_string())
    }

    // ------------------------------------------------------------ internals

    fn finish(&self, state: TaskState, message: &str) {
        {
            let mut s = self.status.lock().unwrap();
            s.running = false;
            s.state = Some(state);
            s.message = Some(message.to_string());
            s.finished_at = Some(now_secs());
        }
        self.task(state, if state == TaskState::Done { Some(1.0) } else { None }, message);
        *self.active.lock().unwrap() = None;
    }

    fn task(&self, state: TaskState, progress: Option<f32>, message: &str) {
        self.events.emit(vnl_core::AppEvent::Task(TaskProgress {
            task_id: TASK_ID.into(),
            label: "Build gamescope (Anime4K)".into(),
            state,
            progress,
            message: Some(message.to_string()),
        }));
    }

    fn log(&self, ctx: &Ctx, level: LogLevel, line: &str) {
        self.events.log(LogEntry::new(level, LOG_SOURCE, line));
        if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(&ctx.log_path) {
            use std::io::Write;
            let _ = writeln!(f, "{line}");
        }
    }

    /// Emit progress, at most ~4 times per second unless `force`.
    fn progress(&self, ctx: &Ctx, fraction: f32, message: &str, force: bool) {
        let mut last = ctx.last_emit.lock().unwrap();
        if force || last.elapsed() >= Duration::from_millis(250) {
            *last = Instant::now();
            self.status.lock().unwrap().message = Some(message.to_string());
            self.task(TaskState::Running, Some(fraction.clamp(0.0, 0.99)), message);
        }
    }

    fn check_cancel(ctx: &Ctx) -> Result<(), BuildError> {
        if ctx.cancel.load(Ordering::SeqCst) { Err(BuildError::Cancelled) } else { Ok(()) }
    }

    async fn run(&self, ctx: &Ctx, tools: &BuildTools, opts: &BuildOptions) -> Result<InstallManifest, BuildError> {
        let work = self.paths.cache_dir.join(gamescope_bin::MANAGED_DIR_NAME);
        let src = work.join("src");
        let build = work.join("build");
        let stage = work.join("stage");
        let final_dir = gamescope_bin::managed_dir(&self.paths);
        std::fs::create_dir_all(&work)?;
        if stage.exists() {
            std::fs::remove_dir_all(&stage)?;
        }
        let env: Vec<(String, String)> = vec![
            ("GIT_TERMINAL_PROMPT".into(), "0".into()),
            ("LC_ALL".into(), "C".into()),
            ("NINJA_STATUS".into(), "[%f/%t] ".into()),
        ];
        let s = |p: &Path| p.to_string_lossy().into_owned();

        // 1. sources (0 - 0.10)
        self.progress(ctx, 0.01, "Fetching the gamescope-anime4k sources", true);
        if src.join(".git").exists() {
            self.exec(
                ctx,
                &tools.git,
                &["-C", &s(&src), "fetch", "--depth", "1", "origin", &opts.branch],
                &work,
                &env,
                |_| {},
            )
            .await?;
            self.exec(ctx, &tools.git, &["-C", &s(&src), "reset", "--hard", "FETCH_HEAD"], &work, &env, |_| {}).await?;
        } else {
            if src.exists() {
                // Our own cache directory, left over from an interrupted clone.
                std::fs::remove_dir_all(&src)?;
            }
            self.exec(
                ctx,
                &tools.git,
                &["clone", "--depth", "1", "--branch", &opts.branch, &opts.repo_url, &s(&src)],
                &work,
                &env,
                |_| {},
            )
            .await?;
        }
        // 2. submodules (0.10 - 0.30)
        self.progress(ctx, 0.10, "Fetching submodules", true);
        let mut attempt = 0;
        loop {
            attempt += 1;
            let mut args: Vec<&str> = vec!["-C"];
            let src_s = s(&src);
            args.push(&src_s);
            args.extend(["submodule", "update", "--init", "--depth", "1", "--jobs", "4", "--"]);
            args.extend(SUBMODULES.iter().copied());
            match self.exec(ctx, &tools.git, &args, &work, &env, |_| {}).await {
                Ok(()) => break,
                Err(BuildError::Failed(e)) if attempt < SUBMODULE_ATTEMPTS => {
                    self.log(
                        ctx,
                        LogLevel::Warn,
                        &format!("submodule fetch failed ({e}); retrying ({attempt}/{SUBMODULE_ATTEMPTS})"),
                    );
                    Self::check_cancel(ctx)?;
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
                Err(e) => return Err(e),
            }
        }
        let mut commit = String::new();
        self.exec(ctx, &tools.git, &["-C", &s(&src), "rev-parse", "HEAD"], &work, &env, |l| {
            commit = l.trim().to_string()
        })
        .await?;
        if commit.is_empty() {
            return fail("cannot determine the commit that was fetched");
        }

        // 2b. the UBO alignment fix; skipped when the sources already contain it
        let patch_file = work.join("anime4k-ubo-alignment.patch");
        std::fs::write(&patch_file, UBO_ALIGNMENT_PATCH)?;
        let mut patches: Vec<String> = Vec::new();
        let src_s = s(&src);
        let patch_s = s(&patch_file);
        match self.exec(ctx, &tools.git, &["-C", &src_s, "apply", "--check", &patch_s], &work, &env, |_| {}).await {
            Ok(()) => {
                self.exec(ctx, &tools.git, &["-C", &src_s, "apply", &patch_s], &work, &env, |_| {}).await?;
                self.log(ctx, LogLevel::Info, "applied the uniform-buffer alignment fix (needed on NVIDIA)");
                patches.push(UBO_ALIGNMENT_PATCH_ID.into());
            }
            Err(BuildError::Cancelled) => return Err(BuildError::Cancelled),
            Err(_) => {
                let reverse = ["-C", &src_s, "apply", "--reverse", "--check", &patch_s];
                if self.exec(ctx, &tools.git, &reverse, &work, &env, |_| {}).await.is_ok() {
                    self.log(ctx, LogLevel::Info, "the sources already contain the uniform-buffer alignment fix");
                    patches.push(UBO_ALIGNMENT_PATCH_ID.into());
                } else {
                    self.log(
                        ctx,
                        LogLevel::Warn,
                        "the alignment fix does not apply to these sources; building without it (Anime4K may render black on NVIDIA)",
                    );
                }
            }
        }

        // 3. configure (0.30 - 0.35)
        self.progress(ctx, 0.30, "Configuring (meson setup)", true);
        let prefix = s(&final_dir);
        let mut setup: Vec<String> =
            vec!["setup".into(), s(&build), s(&src), format!("--prefix={prefix}"), "--buildtype=release".into()];
        for opt in [
            "-Denable_openvr_support=false",
            "-Dbenchmark=disabled",
            "-Dpipewire=disabled",
            "-Davif_screenshots=disabled",
            "-Dsdl2_backend=disabled",
            // wlroots and friends build with -Werror, which breaks as soon as a
            // newer system library (e.g. libinput) adds an enum value.
            "-Dwerror=false",
            "-Dwlroots:werror=false",
            "-Dlibliftoff:werror=false",
            "-Dlibdisplay-info:werror=false",
        ] {
            setup.push(opt.into());
        }
        if build.join("meson-private").exists() {
            setup.push("--reconfigure".into());
        }
        let setup_refs: Vec<&str> = setup.iter().map(String::as_str).collect();
        self.exec(ctx, &tools.meson, &setup_refs, &work, &env, |_| {}).await?;

        // 4. compile (0.35 - 0.90)
        self.progress(ctx, 0.35, "Compiling", true);
        let jobs = opts.jobs.map(|j| j.to_string());
        let mut ninja: Vec<&str> = vec!["-C"];
        let build_s = s(&build);
        ninja.push(&build_s);
        if let Some(j) = &jobs {
            ninja.extend(["-j", j]);
        }
        self.exec(ctx, &tools.ninja, &ninja, &work, &env, |line| {
            if let Some((done, total)) = parse_ninja_progress(line) {
                let f = 0.35 + 0.55 * (done as f32 / total.max(1) as f32);
                self.progress(ctx, f, &format!("Compiling ({done}/{total})"), false);
            }
        })
        .await?;

        // 5. install into a staging DESTDIR (0.90 - 0.95)
        self.progress(ctx, 0.90, "Installing into the private directory", true);
        self.exec(
            ctx,
            &tools.meson,
            &["install", "-C", &build_s, "--skip-subprojects", "--destdir", &s(&stage)],
            &work,
            &env,
            |_| {},
        )
        .await?;
        let staged_root = stage.join(final_dir.strip_prefix("/").unwrap_or(&final_dir));
        let staged_bin = staged_root.join("bin/gamescope");

        // 6. verify before replacing anything (0.95)
        self.progress(ctx, 0.95, "Verifying the new build", true);
        let built = gamescope_bin::inspect(&self.probe, &staged_bin, GamescopeSource::Managed);
        let Some(built) = built else {
            return fail("the built gamescope does not run (`--help` failed)");
        };
        if !built.supports_anime4k {
            return fail(
                "the built gamescope does not list `anime4k` among its -F filters; the repository may have changed",
            );
        }
        let manifest = InstallManifest {
            repo: opts.repo_url.clone(),
            branch: opts.branch.clone(),
            commit,
            built_at: now_secs(),
            version: built.version.clone(),
            filters: built.filters.clone(),
            patches,
        };
        std::fs::write(staged_root.join(MANIFEST_FILE), serde_json::to_string_pretty(&manifest).unwrap_or_default())?;

        // 7. swap into place (previous build kept until the move succeeded)
        Self::check_cancel(ctx)?;
        self.swap_in(&staged_root, &final_dir)?;
        let _ = std::fs::remove_dir_all(&stage);
        Ok(manifest)
    }

    fn swap_in(&self, staged: &Path, final_dir: &Path) -> Result<(), BuildError> {
        if let Some(parent) = final_dir.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let backup = final_dir.with_extension("old");
        if backup.exists() {
            std::fs::remove_dir_all(&backup)?;
        }
        let had_old = final_dir.exists();
        if had_old {
            std::fs::rename(final_dir, &backup)?;
        }
        if let Err(e) = move_dir(staged, final_dir) {
            if had_old {
                let _ = std::fs::remove_dir_all(final_dir);
                let _ = std::fs::rename(&backup, final_dir);
            }
            return fail(format!("cannot install the new build: {e}"));
        }
        if had_old {
            let _ = std::fs::remove_dir_all(&backup);
        }
        Ok(())
    }

    /// Run one command, streaming its output to the log, honouring cancellation.
    async fn exec(
        &self,
        ctx: &Ctx,
        program: &Path,
        args: &[&str],
        cwd: &Path,
        env: &[(String, String)],
        mut on_line: impl FnMut(&str),
    ) -> Result<(), BuildError> {
        Self::check_cancel(ctx)?;
        let shown = format!("$ {} {}", program.display(), args.join(" "));
        self.log(ctx, LogLevel::Info, &shown);
        let mut cmd = Command::new(program);
        cmd.args(args)
            .current_dir(cwd)
            .envs(env.iter().cloned())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .process_group(0);
        let mut child =
            cmd.spawn().map_err(|e| BuildError::Failed(format!("cannot start {}: {e}", program.display())))?;
        let pid = child.id().unwrap_or(0) as i32;
        ctx.pgid.store(pid, Ordering::SeqCst);

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(bool, String)>();
        fn pump(
            r: impl AsyncRead + Unpin + Send + 'static,
            is_err: bool,
            tx: tokio::sync::mpsc::UnboundedSender<(bool, String)>,
        ) {
            tokio::spawn(async move {
                let mut lines = BufReader::new(r).split(b'\n');
                while let Ok(Some(raw)) = lines.next_segment().await {
                    // ninja redraws its status line with `\r`; keep the last piece.
                    let text = String::from_utf8_lossy(&raw);
                    let line = text.rsplit('\r').next().unwrap_or("").trim_end().to_string();
                    if !line.is_empty() && tx.send((is_err, line)).is_err() {
                        break;
                    }
                }
            });
        }
        pump(child.stdout.take().expect("piped"), false, tx.clone());
        pump(child.stderr.take().expect("piped"), true, tx);

        let mut tail: std::collections::VecDeque<String> = std::collections::VecDeque::with_capacity(25);
        let mut tick = tokio::time::interval(Duration::from_millis(150));
        let mut status = None;
        let mut drain_deadline: Option<Instant> = None;
        loop {
            tokio::select! {
                line = rx.recv() => match line {
                    Some((is_err, line)) => {
                        let is_progress = parse_ninja_progress(&line).is_some();
                        if !is_progress {
                            // Keep what explains a failure; ninja's progress lines would push it out.
                            if tail.len() == 25 { tail.pop_front(); }
                            tail.push_back(line.chars().take(400).collect());
                        }
                        // Progress lines would flood the log: keep only every 25th.
                        if !is_progress || line.starts_with("[1/") || parse_ninja_progress(&line).is_some_and(|(d, t)| d % 25 == 0 || d == t) {
                            self.log(ctx, if is_err && !is_progress { LogLevel::Warn } else { LogLevel::Info }, &line);
                        }
                        on_line(&line);
                    }
                    None => break,
                },
                _ = tick.tick() => {
                    if ctx.cancel.load(Ordering::SeqCst) {
                        let _ = killpg(Pid::from_raw(pid), Signal::SIGTERM);
                        if tokio::time::timeout(Duration::from_secs(3), child.wait()).await.is_err() {
                            let _ = killpg(Pid::from_raw(pid), Signal::SIGKILL);
                            let _ = child.wait().await;
                        }
                        ctx.pgid.store(0, Ordering::SeqCst);
                        return Err(BuildError::Cancelled);
                    }
                    if status.is_none() {
                        if let Ok(Some(st)) = child.try_wait() {
                            status = Some(st);
                            drain_deadline = Some(Instant::now() + Duration::from_secs(2));
                        }
                    } else if drain_deadline.is_some_and(|d| Instant::now() >= d) {
                        break; // a grandchild keeps the pipes open
                    }
                }
            }
        }
        let status = match status {
            Some(s) => s,
            None => child.wait().await?,
        };
        ctx.pgid.store(0, Ordering::SeqCst);
        Self::check_cancel(ctx)?;
        if !status.success() {
            let code = status.code().map(|c| c.to_string()).unwrap_or_else(|| "a signal".into());
            let context = tail.iter().cloned().collect::<Vec<_>>().join("\n");
            return fail(format!(
                "`{} {}` failed (exit {code})\n{context}{}",
                program.file_name().and_then(|n| n.to_str()).unwrap_or("command"),
                args.first().copied().unwrap_or(""),
                missing_dependency_hint(&context)
            ));
        }
        Ok(())
    }
}

struct Ctx {
    cancel: Arc<AtomicBool>,
    pgid: Arc<AtomicI32>,
    log_path: PathBuf,
    last_emit: Mutex<Instant>,
}

/// `[12/345] Compiling ...` -> (12, 345).
pub fn parse_ninja_progress(line: &str) -> Option<(u32, u32)> {
    let rest = line.strip_prefix('[')?;
    let (counts, _) = rest.split_once(']')?;
    let (done, total) = counts.split_once('/')?;
    Some((done.trim().parse().ok()?, total.trim().parse().ok()?))
}

/// Turn meson's "Dependency X found: NO" into a pointer to the missing package.
fn missing_dependency_hint(output: &str) -> String {
    let missing: Vec<&str> = output
        .lines()
        .filter_map(|l| l.split("ERROR: Dependency \"").nth(1).and_then(|r| r.split('"').next()))
        .collect();
    if missing.is_empty() {
        return String::new();
    }
    format!("\nMissing development package(s) for: {}. Install them and try again.", missing.join(", "))
}

/// Move a directory; fall back to copy + delete across filesystems.
fn move_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) => {
            copy_dir(from, to)?;
            std::fs::remove_dir_all(from)
        }
    }
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else if ty.is_symlink() {
            std::os::unix::fs::symlink(std::fs::read_link(entry.path())?, &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use vnl_core::events::RecordingSink;
    use vnl_core::{AppEvent, TaskState};

    use super::*;

    #[test]
    fn parses_ninja_progress() {
        assert_eq!(parse_ninja_progress("[12/345] Compiling C++ object x.o"), Some((12, 345)));
        assert_eq!(parse_ninja_progress("[ 3/10] link"), Some((3, 10)));
        assert_eq!(parse_ninja_progress("ninja: build stopped"), None);
        assert_eq!(parse_ninja_progress("[a/b] x"), None);
    }

    #[test]
    fn requirements_report_missing_items_with_hints() {
        let tmp = tempfile::tempdir().unwrap();
        // Empty PATH: everything is missing.
        let probe = Probe::fixture(tmp.path(), [("PATH".to_string(), tmp.path().display().to_string())].into());
        let req = check_requirements(&probe);
        assert!(!req.ok);
        assert!(req.missing.contains(&"meson".to_string()) && req.missing.contains(&"wayland-server".to_string()));
        let arch = req.hints.iter().find(|h| h.distro == "arch").unwrap();
        assert!(arch.command.starts_with("sudo pacman -S --needed ") && arch.command.contains("meson"));
        assert!(req.hints.iter().find(|h| h.distro == "debian").unwrap().command.contains("ninja-build"));
        // Each package is listed once even when several modules need it.
        assert_eq!(arch.command.split_whitespace().filter(|w| *w == "libxcb").count(), 1);
    }

    #[test]
    fn requirements_pass_with_fake_tools_and_pkg_config() {
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("bin");
        for t in ["git", "meson", "ninja", "g++", "glslang", "wayland-scanner"] {
            crate::test_support::script(&bin, t, "exit 0");
        }
        crate::test_support::script(
            &bin,
            "pkg-config",
            "case \"$1\" in --modversion) echo 9.9;; --atleast-version) exit 0;; esac",
        );
        let probe = Probe::fixture(tmp.path(), [("PATH".to_string(), bin.display().to_string())].into());
        let req = check_requirements(&probe);
        assert!(req.ok, "missing: {:?}", req.missing);
        assert!(req.hints.is_empty());
    }

    #[test]
    fn meson_missing_dependency_is_explained() {
        let out =
            "Run-time dependency libdecor-0 found: NO\nERROR: Dependency \"libdecor-0\" not found, tried pkgconfig";
        assert!(missing_dependency_hint(out).contains("libdecor-0"));
        assert_eq!(missing_dependency_hint("all fine"), "");
    }

    /// One script plays git, meson and ninja. `$FAKE_MODE` selects behaviour.
    fn fake_tools(dir: &Path) -> BuildTools {
        let script = r#"
tool=$(basename "$0")
case "$tool" in
git)
  case "$*" in
    *clone*) for last; do :; done; mkdir -p "$last/.git"; echo cloned;;
    *rev-parse*) echo 0123456789abcdef0123456789abcdef01234567;;
    *apply*) [ "$FAKE_MODE" = nopatch ] && { echo "error: patch failed" >&2; exit 1; }; echo applied;;
    *submodule*) [ "$FAKE_MODE" = flaky ] && [ ! -f "$FAKE_STATE/sub-ok" ] && { touch "$FAKE_STATE/sub-ok"; echo "fatal: network" >&2; exit 1; }; echo submodules;;
    *) echo git "$@";;
  esac;;
meson)
  case "$1" in
    setup) mkdir -p "$2/meson-private"; echo "Configuring";;
    install)
      [ "$FAKE_MODE" = badbuild ] && exit 0
      dest=$6; prefix=$(cat "$FAKE_STATE/prefix"); mkdir -p "$dest$prefix/bin"
      printf '#!/bin/sh\ncase "$1" in --version) echo "gamescope version 3.16.0" >&2;; --help) echo "gamescope -F, --filter upscaler filter (linear, fsr, anime4k)";; esac\n' > "$dest$prefix/bin/gamescope"
      chmod +x "$dest$prefix/bin/gamescope";;
  esac;;
ninja)
  [ "$FAKE_MODE" = hang ] && { sleep 60 & wait; }
  for i in 1 2 3 4; do echo "[$i/4] Compiling file$i.cpp"; done;;
esac
"#;
        for t in ["git", "meson", "ninja"] {
            crate::test_support::script(dir, t, script);
        }
        BuildTools { git: dir.join("git"), meson: dir.join("meson"), ninja: dir.join("ninja") }
    }

    struct Env {
        tmp: tempfile::TempDir,
        builder: Arc<GamescopeBuilder>,
        sink: RecordingSink,
        paths: AppPaths,
    }

    fn env(mode: &str) -> Env {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::under(tmp.path());
        paths.ensure().unwrap();
        let bin = tmp.path().join("fakebin");
        std::fs::create_dir_all(&bin).unwrap();
        let state = tmp.path().join("state");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(state.join("prefix"), gamescope_bin::managed_dir(&paths).display().to_string()).unwrap();
        // Scripts inherit these through the environment of the test process.
        // SAFETY: tests of this module run under a lock; see `ENV_LOCK`.
        unsafe {
            std::env::set_var("FAKE_MODE", mode);
            std::env::set_var("FAKE_STATE", &state);
        }
        let sink = RecordingSink::new();
        let builder = GamescopeBuilder::with_probe(&paths, Arc::new(sink.clone()), Probe::system())
            .with_fake_tools(fake_tools(&bin));
        Env { tmp, builder: Arc::new(builder), sink, paths }
    }

    static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    async fn wait_finished(b: &GamescopeBuilder) -> BuildStatus {
        for _ in 0..200 {
            let s = b.status();
            if !s.running {
                return s;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("build did not finish: {:?}", b.status());
    }

    fn task_states(sink: &RecordingSink) -> Vec<(TaskState, Option<f32>)> {
        sink.events()
            .into_iter()
            .filter_map(|e| match e {
                AppEvent::Task(t) if t.task_id == TASK_ID => Some((t.state, t.progress)),
                _ => None,
            })
            .collect()
    }

    #[tokio::test]
    async fn successful_build_installs_privately_and_reports_progress() {
        let _g = ENV_LOCK.lock().await;
        let e = env("ok");
        assert!(!e.builder.managed().installed);
        let id = e.builder.start(BuildOptions::default()).unwrap();
        assert_eq!(id, TASK_ID);
        let status = wait_finished(&e.builder).await;
        assert_eq!(status.state, Some(TaskState::Done), "{status:?}");

        let managed = e.builder.managed();
        assert!(managed.installed);
        let bin = managed.binary.unwrap();
        assert!(bin.supports_anime4k && bin.source == GamescopeSource::Managed);
        assert!(bin.path.starts_with(e.paths.tools_dir()), "installed outside the private dir");
        let m = managed.manifest.unwrap();
        assert_eq!(m.commit, "0123456789abcdef0123456789abcdef01234567");
        assert!(m.filters.contains(&"anime4k".to_string()));
        assert!(m.has_ubo_alignment_fix(), "the NVIDIA fix is part of the build: {:?}", m.patches);
        // Staging area cleaned up, log file written.
        assert!(!e.paths.cache_dir.join("gamescope-anime4k/stage").exists());
        let log = std::fs::read_to_string(status.log_path.unwrap()).unwrap();
        assert!(log.contains("meson") && log.contains("[4/4]"));

        let states = task_states(&e.sink);
        assert_eq!(states.first().unwrap().0, TaskState::Running);
        assert_eq!(states.last().unwrap(), &(TaskState::Done, Some(1.0)));
        let progress: Vec<f32> = states.iter().filter_map(|s| s.1).collect();
        assert!(progress.windows(2).all(|w| w[0] <= w[1]), "progress went backwards: {progress:?}");
        assert!(e.sink.events().iter().any(|ev| matches!(ev, AppEvent::Log(l) if l.source == LOG_SOURCE)));
        drop(e.tmp);
    }

    #[tokio::test]
    async fn patch_that_does_not_apply_is_reported_but_does_not_break_the_build() {
        let _g = ENV_LOCK.lock().await;
        let e = env("nopatch");
        e.builder.start(BuildOptions::default()).unwrap();
        let status = wait_finished(&e.builder).await;
        assert_eq!(status.state, Some(TaskState::Done), "{status:?}");
        let m = e.builder.managed().manifest.unwrap();
        assert!(m.patches.is_empty() && !m.has_ubo_alignment_fix());
        let log = std::fs::read_to_string(status.log_path.unwrap()).unwrap();
        assert!(log.contains("does not apply to these sources"));
    }

    #[test]
    fn bundled_patch_targets_the_uniform_alignment() {
        // Regression guard for the NVIDIA black-image bug: offsets must follow the device limit.
        assert!(UBO_ALIGNMENT_PATCH.contains("minUniformBufferOffsetAlignment"));
        assert!(UBO_ALIGNMENT_PATCH.contains("-		m_uploadBufferOffset = align(m_uploadBufferOffset, 16);"));
        assert!(
            UBO_ALIGNMENT_PATCH
                .contains("+		m_uploadBufferOffset = align(m_uploadBufferOffset, m_uniformOffsetAlignment);")
        );
        // Only the two renderer files are touched; the FSR/NIS/linear shaders are not.
        assert!(UBO_ALIGNMENT_PATCH.lines().filter(|l| l.starts_with("diff --git")).count() == 2);
        assert!(!UBO_ALIGNMENT_PATCH.contains("shaders/"));
    }

    #[tokio::test]
    async fn flaky_submodule_fetch_is_retried() {
        let _g = ENV_LOCK.lock().await;
        let e = env("flaky");
        e.builder.start(BuildOptions::default()).unwrap();
        let status = wait_finished(&e.builder).await;
        assert_eq!(status.state, Some(TaskState::Done), "{status:?}");
        let log = std::fs::read_to_string(status.log_path.unwrap()).unwrap();
        assert!(log.contains("retrying (1/3)"));
    }

    #[tokio::test]
    async fn build_that_lacks_anime4k_is_rejected_and_keeps_existing_install() {
        let _g = ENV_LOCK.lock().await;
        let e = env("ok");
        // An existing managed install must survive a failing rebuild.
        let dir = gamescope_bin::managed_dir(&e.paths);
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        crate::test_support::script(
            &dir.join("bin"),
            "gamescope",
            "echo gamescope; echo '  -F, --filter  x (linear, anime4k)'",
        );
        std::fs::write(dir.join("marker"), "keep").unwrap();
        unsafe { std::env::set_var("FAKE_MODE", "badbuild") };
        e.builder.start(BuildOptions::default()).unwrap();
        let status = wait_finished(&e.builder).await;
        assert_eq!(status.state, Some(TaskState::Failed));
        assert!(status.message.unwrap().contains("does not run"), "unexpected failure message");
        assert_eq!(std::fs::read_to_string(dir.join("marker")).unwrap(), "keep");
    }

    #[tokio::test]
    async fn cancellation_stops_the_build_and_its_children() {
        let _g = ENV_LOCK.lock().await;
        let e = env("hang");
        e.builder.start(BuildOptions::default()).unwrap();
        // Wait until ninja (the hanging step) runs.
        for _ in 0..100 {
            if e.builder.status().message.as_deref().is_some_and(|m| m.starts_with("Compiling")) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(e.builder.cancel());
        let status = wait_finished(&e.builder).await;
        assert_eq!(status.state, Some(TaskState::Failed));
        assert_eq!(status.message.as_deref(), Some("Build cancelled"));
        assert!(!e.builder.managed().installed);
        assert!(!e.builder.cancel(), "nothing left to cancel");
        // No stray fake ninja/sleep processes of this build remain.
        let out =
            std::process::Command::new("pgrep").args(["-f", &e.tmp.path().display().to_string()]).output().unwrap();
        assert!(String::from_utf8_lossy(&out.stdout).trim().is_empty(), "leftover processes");
    }

    #[tokio::test]
    async fn second_start_is_refused_while_running_and_remove_needs_idle() {
        let _g = ENV_LOCK.lock().await;
        let e = env("hang");
        e.builder.start(BuildOptions::default()).unwrap();
        assert!(e.builder.start(BuildOptions::default()).is_err());
        assert!(e.builder.remove_managed().is_err());
        e.builder.cancel();
        wait_finished(&e.builder).await;
        e.builder.remove_managed().unwrap();
    }

    #[test]
    fn start_fails_with_install_hints_when_dependencies_are_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::under(tmp.path());
        let probe = Probe::fixture(tmp.path(), [("PATH".to_string(), tmp.path().display().to_string())].into());
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let _enter = rt.enter();
        let b = Arc::new(GamescopeBuilder::with_probe(&paths, Arc::new(vnl_core::NullSink), probe));
        let err = b.start(BuildOptions::default()).unwrap_err();
        assert_eq!(err.code, ErrorCode::MissingDependency);
        assert!(err.details.unwrap().contains("pacman"));
        assert!(!b.status().running);
    }

    /// Real build of the fork with the system toolchain (several minutes, needs
    /// network). The result goes to `$VNL_TEST_BUILD_HOME` (default: a tempdir),
    /// never to the user's own data. Run with:
    /// `cargo test -p vnl-graphics real_build -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "clones and compiles gamescope-anime4k (network, minutes)"]
    async fn real_build_of_the_anime4k_fork() {
        let tmp;
        let root = match std::env::var_os("VNL_TEST_BUILD_HOME") {
            Some(p) => PathBuf::from(p),
            None => {
                tmp = tempfile::tempdir().unwrap();
                tmp.path().to_path_buf()
            }
        };
        let paths = AppPaths::under(&root);
        paths.ensure().unwrap();
        let sink = RecordingSink::new();
        let builder = Arc::new(GamescopeBuilder::with_probe(&paths, Arc::new(sink.clone()), Probe::system()));
        let req = builder.requirements();
        assert!(req.ok, "missing build dependencies: {:?}\n{:?}", req.missing, req.hints);
        builder.start(BuildOptions::default()).unwrap();
        let started = Instant::now();
        let mut last = String::new();
        loop {
            let s = builder.status();
            if s.message.as_deref() != Some(last.as_str()) {
                last = s.message.clone().unwrap_or_default();
                eprintln!("[{:>4}s] {last}", started.elapsed().as_secs());
            }
            if !s.running {
                assert_eq!(s.state, Some(TaskState::Done), "{s:?}");
                break;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        let managed = builder.managed();
        let bin = managed.binary.expect("managed binary");
        assert!(bin.supports_anime4k, "filters: {:?}", bin.filters);
        let manifest = managed.manifest.expect("manifest");
        assert!(
            manifest.has_ubo_alignment_fix(),
            "the fix did not apply to the current sources: {:?}",
            manifest.patches
        );
        eprintln!("built {} {:?} filters {:?}", bin.path.display(), bin.version, bin.filters);
    }

    /// Runs `vkcube` inside gamescope with the managed fork and checks that `-F anime4k`
    /// is accepted and what it renders. Needs a built fork at `$VNL_TEST_BUILD_HOME`
    /// (see `real_build_of_the_anime4k_fork`), `vkcube`, ImageMagick and an X11
    /// display for gamescope's headless backend. The final picture is read with
    /// gamescope's own screenshot request (root property `GAMESCOPECTRL_REQUEST_SCREENSHOT`,
    /// type 4 = what is on screen).
    ///
    /// Regression test for the NVIDIA black-image bug (see `UBO_ALIGNMENT_PATCH`): the
    /// build made by this module must render, like the `linear` control.
    #[test]
    #[ignore = "needs the built fork, vkcube and ImageMagick"]
    fn real_anime4k_filter_renders_a_picture() {
        use std::os::unix::process::CommandExt;
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::{AtomEnum, ConnectionExt, PropMode};
        use x11rb::wrapper::ConnectionExt as _;

        let root = PathBuf::from(std::env::var("VNL_TEST_BUILD_HOME").expect("VNL_TEST_BUILD_HOME with a built fork"));
        let bin = gamescope_bin::managed_binary(&AppPaths::under(&root));
        let probe = Probe::system();
        let info = gamescope_bin::inspect(&probe, &bin, GamescopeSource::Managed).expect("built fork");
        assert!(info.supports_anime4k);

        let capture = |filter: &str| -> f64 {
            let out = tempfile::tempdir().unwrap();
            let log = std::fs::File::create(out.path().join("gamescope.log")).unwrap();
            let mut child = std::process::Command::new(&bin)
                .args([
                    "--backend",
                    "headless",
                    "-w",
                    "640",
                    "-h",
                    "360",
                    "-W",
                    "1280",
                    "-H",
                    "720",
                    "-F",
                    filter,
                    "--",
                ])
                .args(["env", "ENABLE_GAMESCOPE_WSI=0", "vkcube", "--wsi", "xcb", "--c", "0"])
                .stdout(log.try_clone().unwrap())
                .stderr(log)
                .process_group(0)
                .spawn()
                .unwrap();
            let log_path = out.path().join("gamescope.log");
            let display = (0..40)
                .find_map(|_| {
                    std::thread::sleep(Duration::from_millis(500));
                    crate::fullscreen::parse_xwayland_display(&std::fs::read_to_string(&log_path).unwrap_or_default())
                })
                .expect("gamescope's Xwayland display");
            std::thread::sleep(Duration::from_secs(6));
            let (conn, screen) = x11rb::connect(Some(&display)).unwrap();
            let rootw = conn.setup().roots[screen].root;
            let atom = conn.intern_atom(false, b"GAMESCOPECTRL_REQUEST_SCREENSHOT").unwrap().reply().unwrap().atom;
            conn.change_property32(PropMode::REPLACE, rootw, atom, AtomEnum::CARDINAL, &[4]).unwrap();
            conn.flush().unwrap();
            let shot = Path::new("/tmp/gamescope.png"); // fixed path hard-coded in gamescope
            let _ = std::fs::remove_file(shot);
            for _ in 0..20 {
                if shot.exists() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(250));
            }
            std::thread::sleep(Duration::from_millis(500));
            let png = out.path().join("shot.png");
            let _ = std::fs::rename(shot, &png).or_else(|_| std::fs::copy(shot, &png).map(|_| ()));
            let alive = child.try_wait().unwrap().is_none();
            let _ = killpg(Pid::from_raw(child.id() as i32), Signal::SIGKILL);
            let _ = child.wait();
            assert!(
                alive,
                "gamescope exited early with -F {filter}:\n{}",
                std::fs::read_to_string(&log_path).unwrap_or_default()
            );
            let mean = std::process::Command::new("magick")
                .arg(&png)
                .args(["-alpha", "off", "-format", "%[fx:mean]", "info:"])
                .output()
                .expect("ImageMagick (magick)");
            String::from_utf8_lossy(&mean.stdout).trim().parse().expect("mean brightness")
        };

        let linear = capture("linear");
        assert!(linear > 0.01, "control run with -F linear rendered nothing (mean {linear})");
        let anime4k = capture("anime4k");
        eprintln!("mean brightness: linear {linear:.4}, anime4k {anime4k:.4}");
        // The guided build includes the uniform-alignment fix, so a black picture is a regression.
        // (Without the fix NVIDIA renders black: VUID-VkWriteDescriptorSet-descriptorType-00327.)
        let fixed = read_manifest(&AppPaths::under(&root)).is_some_and(|m| m.has_ubo_alignment_fix());
        if fixed {
            assert!(anime4k > 0.01, "-F anime4k rendered a black image even with the alignment fix (mean {anime4k})");
            assert!(
                (anime4k - linear).abs() < 0.05 * linear,
                "anime4k brightness {anime4k} is far from linear {linear}"
            );
        } else if anime4k <= 0.001 {
            eprintln!("WARNING: this fork build renders black here (no alignment fix recorded in its manifest)");
        }
    }

    #[test]
    fn move_dir_copies_trees_and_symlinks() {
        let tmp = tempfile::tempdir().unwrap();
        let from = tmp.path().join("a");
        std::fs::create_dir_all(from.join("bin")).unwrap();
        std::fs::write(from.join("bin/x"), "1").unwrap();
        std::os::unix::fs::symlink("x", from.join("bin/link")).unwrap();
        copy_dir(&from, &tmp.path().join("b")).unwrap();
        assert_eq!(std::fs::read_to_string(tmp.path().join("b/bin/x")).unwrap(), "1");
        assert_eq!(std::fs::read_link(tmp.path().join("b/bin/link")).unwrap(), PathBuf::from("x"));
    }
}
