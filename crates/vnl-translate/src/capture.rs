//! Screen capture (`ScreenCapture`) for OCR.
//!
//! What works on Linux, verified on GNOME 51 / Wayland with XWayland:
//! - **X11 (`x11`)**: `ffmpeg -f x11grab` (fallback ImageMagick `import`).
//!   GNOME's XWayland is *rootless* and windows are redirected, so capturing
//!   the root window returns black even where X11 windows are — verified on
//!   the dev machine. What works is capturing a specific X11 window
//!   (`-window_id <id>` / `import -window <id>`): verified with a GTK X11
//!   window, then OCRed correctly by a local Ollama vision model. Wine sets
//!   `WM_CLASS` to the exe name (e.g. `game.exe`), so the game window is found
//!   via `xprop` on `_NET_CLIENT_LIST`. Root capture still works on real X11
//!   sessions; gamescope's nested Xwayland (e.g. `:2`) is untested.
//! - **xdg-desktop-portal (`portal`)**: `org.freedesktop.portal.Screenshot`.
//!   Captures Wayland too, but on GNOME it asks the user for permission
//!   (interactive dialog the first time / every time when `interactive`).
//!   Unsuitable for periodic OCR; fine for manual captures.
//! - **file (`file`)**: OCR an existing image (tests, manual use).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use vnl_core::{AppError, CaptureRegion, CommandSpec, ErrorCode};

use crate::types::BackendInfo;

#[derive(Debug, Clone)]
pub struct CapturedImage {
    /// PNG (or other format understood by the OCR backend) bytes.
    pub bytes: Vec<u8>,
    pub source: String,
}

#[async_trait]
pub trait ScreenCapture: Send + Sync {
    fn id(&self) -> &str;
    async fn capture(&self, region: Option<CaptureRegion>) -> Result<CapturedImage, AppError>;
}

pub fn validate_region(r: &CaptureRegion) -> Result<(), AppError> {
    if r.width == 0 || r.height == 0 || r.width > 16384 || r.height > 16384 || r.x < 0 || r.y < 0 {
        return Err(AppError::invalid(format!(
            "invalid capture region {}x{}+{}+{} (width/height must be 1–16384, x/y ≥ 0)",
            r.width, r.height, r.x, r.y
        )));
    }
    Ok(())
}

/// `ffmpeg` x11grab invocation writing one PNG to stdout.
/// `window`: X11 window id (`0x3000003`); region offsets are then relative to it.
pub fn ffmpeg_x11_command(
    ffmpeg: &Path,
    display: &str,
    window: Option<&str>,
    region: Option<CaptureRegion>,
) -> CommandSpec {
    let mut spec = CommandSpec::new(ffmpeg).args(["-hide_banner", "-loglevel", "error", "-nostdin", "-f", "x11grab"]);
    if let Some(w) = window {
        spec = spec.args(["-window_id", w]);
    }
    let input = match region {
        Some(r) => {
            spec = spec.args(["-video_size".to_string(), format!("{}x{}", r.width, r.height)]);
            format!("{display}+{},{}", r.x, r.y)
        }
        None => display.to_string(),
    };
    spec.args(["-draw_mouse", "0", "-i"]).arg(input).args(["-frames:v", "1", "-f", "image2pipe", "-vcodec", "png", "-"])
}

/// ImageMagick `import` invocation writing one PNG to stdout.
pub fn import_x11_command(
    import: &Path,
    display: &str,
    window: Option<&str>,
    region: Option<CaptureRegion>,
) -> CommandSpec {
    let mut spec = CommandSpec::new(import).args(["-display", display, "-window", window.unwrap_or("root"), "-silent"]);
    if let Some(r) = region {
        spec = spec.args([
            "-crop".to_string(),
            format!("{}x{}+{}+{}", r.width, r.height, r.x, r.y),
            "+repage".to_string(),
        ]);
    }
    spec.arg("png:-")
}

/// Runs a command (no shell), returning stdout; fails with stderr on error.
pub async fn run_capture_command(spec: &CommandSpec, timeout: Duration) -> Result<Vec<u8>, AppError> {
    let mut cmd = tokio::process::Command::from(spec.to_std_command());
    cmd.stdin(std::process::Stdio::null()).kill_on_drop(true);
    let out = tokio::time::timeout(timeout, cmd.output())
        .await
        .map_err(|_| AppError::new(ErrorCode::Process, format!("capture timed out: {}", spec.preview())))?
        .map_err(|e| AppError::new(ErrorCode::Process, format!("cannot run {}: {e}", spec.program.display())))?;
    if !out.status.success() || out.stdout.is_empty() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(AppError::new(ErrorCode::Process, format!("screen capture failed ({})", out.status))
            .with_details(format!("{}\n{stderr}", spec.preview())));
    }
    Ok(out.stdout)
}

pub fn is_png(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x89PNG\r\n\x1a\n")
}

/// Width/height from a PNG IHDR chunk.
pub fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if !is_png(bytes) || bytes.len() < 24 {
        return None;
    }
    let w = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let h = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    Some((w, h))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum X11Tool {
    Ffmpeg,
    Import,
}

/// Which X11 drawable to capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum X11Target {
    Root,
    /// Explicit window id (`0x3000003`).
    WindowId(String),
    /// First client window whose WM_CLASS (or title) matches, e.g. `game.exe`.
    WindowClass(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct X11Window {
    pub id: String,
    pub class: Vec<String>,
    pub name: Option<String>,
    pub pid: Option<u32>,
}

/// `_NET_CLIENT_LIST(WINDOW): window id # 0x3000003, 0x400003` → ids.
pub fn parse_client_list(out: &str) -> Vec<String> {
    out.split_once('#')
        .map(|(_, ids)| ids.split(',').map(|i| i.trim().to_string()).filter(|i| i.starts_with("0x")).collect())
        .unwrap_or_default()
}

fn quoted_values(v: &str) -> Vec<String> {
    v.split('"').skip(1).step_by(2).map(str::to_string).collect()
}

/// Parses `xprop -id <id> WM_CLASS _NET_WM_PID WM_NAME _NET_WM_NAME` output.
pub fn parse_window_props(id: &str, out: &str) -> X11Window {
    let mut w = X11Window { id: id.to_string(), class: Vec::new(), name: None, pid: None };
    for line in out.lines() {
        let Some((key, val)) = line.split_once('=') else { continue };
        let key = key.split('(').next().unwrap_or("").trim();
        match key {
            "WM_CLASS" => w.class = quoted_values(val),
            "_NET_WM_PID" => w.pid = val.trim().parse().ok(),
            "_NET_WM_NAME" => w.name = quoted_values(val).into_iter().next().or(w.name.take()),
            "WM_NAME" if w.name.is_none() => w.name = quoted_values(val).into_iter().next(),
            _ => {}
        }
    }
    w
}

/// Wine sets WM_CLASS to `("game.exe", "Wine")`; also match the exe stem in the title.
pub fn match_window<'a>(windows: &'a [X11Window], exe_name: &str) -> Option<&'a X11Window> {
    let want = exe_name.to_lowercase();
    let stem = want.trim_end_matches(".exe");
    windows
        .iter()
        .find(|w| w.class.iter().any(|c| c.to_lowercase() == want))
        .or_else(|| windows.iter().find(|w| w.class.iter().any(|c| c.to_lowercase() == stem)))
        .or_else(|| {
            windows
                .iter()
                .find(|w| w.name.as_deref().is_some_and(|n| !stem.is_empty() && n.to_lowercase().contains(stem)))
        })
}

async fn xprop(display: &str, args: &[&str]) -> Result<String, AppError> {
    let xprop = which::which("xprop").map_err(|_| {
        AppError::new(ErrorCode::MissingDependency, "xprop is needed to find the game window")
            .with_details("Install with: pacman -S xorg-xprop (apt install x11-utils)")
    })?;
    let out = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::process::Command::new(xprop).arg("-display").arg(display).args(args).kill_on_drop(true).output(),
    )
    .await
    .map_err(|_| AppError::new(ErrorCode::Process, "xprop timed out"))??;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Lists managed client windows on an X display (`_NET_CLIENT_LIST`).
pub async fn list_x11_windows(display: &str) -> Result<Vec<X11Window>, AppError> {
    let ids = parse_client_list(&xprop(display, &["-root", "_NET_CLIENT_LIST"]).await?);
    let mut out = Vec::new();
    for id in ids {
        let props = xprop(display, &["-id", &id, "WM_CLASS", "_NET_WM_PID", "WM_NAME", "_NET_WM_NAME"]).await?;
        out.push(parse_window_props(&id, &props));
    }
    Ok(out)
}

pub struct X11Capture {
    pub display: String,
    pub tool: X11Tool,
    pub program: PathBuf,
    pub target: X11Target,
    resolved: std::sync::Mutex<Option<String>>,
}

impl X11Capture {
    /// Picks ffmpeg, then ImageMagick `import`. `display` defaults to `$DISPLAY`.
    pub fn detect(display: Option<&str>) -> Result<Self, AppError> {
        let display = display
            .map(str::to_string)
            .or_else(|| std::env::var("DISPLAY").ok())
            .filter(|d| !d.is_empty())
            .ok_or_else(|| {
            AppError::new(ErrorCode::Unsupported, "No X11 display: $DISPLAY is not set (XWayland not running?)")
        })?;
        if !display.contains(':') {
            return Err(AppError::invalid(format!("'{display}' is not an X11 display name like ':1'")));
        }
        let (tool, program) = if let Ok(p) = which::which("ffmpeg") {
            (X11Tool::Ffmpeg, p)
        } else if let Ok(p) = which::which("import") {
            (X11Tool::Import, p)
        } else {
            return Err(AppError::new(ErrorCode::MissingDependency, "X11 capture needs ffmpeg or ImageMagick")
                .with_details("Install with: pacman -S ffmpeg  (or: apt install ffmpeg / dnf install ffmpeg)"));
        };
        Ok(Self { display, tool, program, target: X11Target::Root, resolved: std::sync::Mutex::new(None) })
    }

    pub fn with_target(mut self, target: X11Target) -> Self {
        self.target = target;
        self
    }

    pub fn command(&self, window: Option<&str>, region: Option<CaptureRegion>) -> CommandSpec {
        match self.tool {
            X11Tool::Ffmpeg => ffmpeg_x11_command(&self.program, &self.display, window, region),
            X11Tool::Import => import_x11_command(&self.program, &self.display, window, region),
        }
    }

    async fn window_id(&self) -> Result<Option<String>, AppError> {
        match &self.target {
            X11Target::Root => Ok(None),
            X11Target::WindowId(id) => Ok(Some(id.clone())),
            X11Target::WindowClass(class) => {
                if let Some(id) = self.resolved.lock().unwrap().clone() {
                    return Ok(Some(id));
                }
                let windows = list_x11_windows(&self.display).await?;
                let w = match_window(&windows, class).ok_or_else(|| {
                    AppError::not_found(format!(
                        "Game window ({class}) not found on X display {} — is the game running under X11/XWayland?",
                        self.display
                    ))
                })?;
                *self.resolved.lock().unwrap() = Some(w.id.clone());
                Ok(Some(w.id.clone()))
            }
        }
    }
}

#[async_trait]
impl ScreenCapture for X11Capture {
    fn id(&self) -> &str {
        "x11"
    }

    async fn capture(&self, region: Option<CaptureRegion>) -> Result<CapturedImage, AppError> {
        if let Some(r) = &region {
            validate_region(r)?;
        }
        let window = self.window_id().await?;
        let result = run_capture_command(&self.command(window.as_deref(), region), Duration::from_secs(10)).await;
        let bytes = match result {
            Ok(b) => b,
            Err(e) => {
                // The window may have been re-created; resolve again next time.
                self.resolved.lock().unwrap().take();
                return Err(e);
            }
        };
        if !is_png(&bytes) || png_size(&bytes) == Some((1, 1)) {
            self.resolved.lock().unwrap().take();
            return Err(AppError::new(
                ErrorCode::Process,
                "X11 capture returned no image (window gone or not capturable)",
            ));
        }
        Ok(CapturedImage { bytes, source: format!("x11 {} {}", self.display, window.as_deref().unwrap_or("root")) })
    }
}

/// Reads an existing image file.
pub struct FileCapture {
    pub path: PathBuf,
}

#[async_trait]
impl ScreenCapture for FileCapture {
    fn id(&self) -> &str {
        "file"
    }

    async fn capture(&self, region: Option<CaptureRegion>) -> Result<CapturedImage, AppError> {
        let bytes = tokio::fs::read(&self.path)
            .await
            .map_err(|e| AppError::new(ErrorCode::Io, format!("cannot read image {}: {e}", self.path.display())))?;
        let bytes = match region {
            Some(r) => crop_with_magick(&bytes, r).await.unwrap_or(bytes),
            None => bytes,
        };
        Ok(CapturedImage { bytes, source: format!("file {}", self.path.display()) })
    }
}

/// Crops with ImageMagick (`magick - -crop … png:-`) when available.
pub async fn crop_with_magick(bytes: &[u8], r: CaptureRegion) -> Result<Vec<u8>, AppError> {
    use tokio::io::AsyncWriteExt;
    validate_region(&r)?;
    let magick = which::which("magick")
        .map_err(|_| AppError::new(ErrorCode::MissingDependency, "cropping needs ImageMagick (`magick`)"))?;
    let mut child = tokio::process::Command::new(magick)
        .args(["-", "-crop", &format!("{}x{}+{}+{}", r.width, r.height, r.x, r.y), "+repage", "png:-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| AppError::new(ErrorCode::Process, format!("cannot run magick: {e}")))?;
    let mut stdin = child.stdin.take().expect("piped stdin");
    let data = bytes.to_vec();
    let writer = tokio::spawn(async move {
        let _ = stdin.write_all(&data).await;
    });
    let out = tokio::time::timeout(Duration::from_secs(20), child.wait_with_output())
        .await
        .map_err(|_| AppError::new(ErrorCode::Process, "magick crop timed out"))??;
    let _ = writer.await;
    if !out.status.success() || !is_png(&out.stdout) {
        return Err(AppError::new(ErrorCode::Process, "magick crop failed")
            .with_details(String::from_utf8_lossy(&out.stderr).to_string()));
    }
    Ok(out.stdout)
}

/// xdg-desktop-portal Screenshot. `interactive = true` lets the user pick the
/// area in the compositor's UI; GNOME may show a permission dialog either way.
pub struct PortalCapture {
    pub interactive: bool,
    pub timeout: Duration,
}

impl PortalCapture {
    pub async fn screenshot_uri(&self) -> Result<String, AppError> {
        use futures_util::StreamExt;
        use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

        let perr = |e: zbus::Error| AppError::new(ErrorCode::Unsupported, format!("screenshot portal: {e}"));
        let conn = zbus::Connection::session().await.map_err(perr)?;
        let unique = conn.unique_name().ok_or_else(|| AppError::internal("no D-Bus unique name"))?.to_string();
        let token = format!("vnl{}", uuid::Uuid::new_v4().simple());
        let sender = unique.trim_start_matches(':').replace('.', "_");
        let request_path = format!("/org/freedesktop/portal/desktop/request/{sender}/{token}");
        let request = zbus::Proxy::new(
            &conn,
            "org.freedesktop.portal.Desktop",
            request_path.as_str(),
            "org.freedesktop.portal.Request",
        )
        .await
        .map_err(perr)?;
        // Subscribe before calling to avoid missing a fast response.
        let mut responses = request.receive_signal("Response").await.map_err(perr)?;
        let portal = zbus::Proxy::new(
            &conn,
            "org.freedesktop.portal.Desktop",
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.Screenshot",
        )
        .await
        .map_err(perr)?;
        let mut options: HashMap<&str, Value<'_>> = HashMap::new();
        options.insert("handle_token", Value::from(token.as_str()));
        options.insert("interactive", Value::from(self.interactive));
        let _handle: OwnedObjectPath = portal.call("Screenshot", &("", options)).await.map_err(perr)?;
        let msg = tokio::time::timeout(self.timeout, responses.next())
            .await
            .map_err(|_| {
                AppError::new(
                    ErrorCode::Unsupported,
                    "screenshot portal did not answer (permission dialog not confirmed?)",
                )
            })?
            .ok_or_else(|| AppError::internal("screenshot portal closed the request"))?;
        let (code, results): (u32, HashMap<String, OwnedValue>) =
            msg.body().deserialize().map_err(|e| AppError::internal(format!("bad portal response: {e}")))?;
        match code {
            0 => {}
            1 => return Err(AppError::invalid("Screenshot cancelled or denied by the user")),
            _ => return Err(AppError::new(ErrorCode::Unsupported, "Screenshot portal request failed")),
        }
        let uri = results
            .get("uri")
            .and_then(|v| <&str>::try_from(v).ok().map(str::to_string))
            .ok_or_else(|| AppError::internal("screenshot portal returned no uri"))?;
        Ok(uri)
    }
}

pub fn file_uri_to_path(uri: &str) -> Result<PathBuf, AppError> {
    let url = url::Url::parse(uri).map_err(|e| AppError::internal(format!("bad file uri {uri}: {e}")))?;
    url.to_file_path().map_err(|_| AppError::internal(format!("not a local file uri: {uri}")))
}

#[async_trait]
impl ScreenCapture for PortalCapture {
    fn id(&self) -> &str {
        "portal"
    }

    async fn capture(&self, region: Option<CaptureRegion>) -> Result<CapturedImage, AppError> {
        let uri = self.screenshot_uri().await?;
        let path = file_uri_to_path(&uri)?;
        let bytes = tokio::fs::read(&path).await?;
        // The portal saves into ~/Pictures; remove our temporary screenshot.
        let _ = tokio::fs::remove_file(&path).await;
        let bytes = match region {
            Some(r) => crop_with_magick(&bytes, r).await?,
            None => bytes,
        };
        Ok(CapturedImage { bytes, source: "xdg-desktop-portal".into() })
    }
}

/// Diagnostics for the capture methods (shown alongside OCR backends).
pub fn capture_backends() -> Vec<BackendInfo> {
    let mut out = Vec::new();
    let display = std::env::var("DISPLAY").ok().filter(|d| !d.is_empty());
    let wayland = std::env::var("WAYLAND_DISPLAY").ok().filter(|d| !d.is_empty());
    let mut diag = Vec::new();
    let x11 = X11Capture::detect(None);
    match &x11 {
        Ok(c) => {
            diag.push(format!("Using {:?} on display {}", c.tool, c.display));
            if wayland.is_some() {
                diag.push(
                    "Wayland session (rootless XWayland): the game window is captured directly (found by its WM_CLASS, \
                     which Wine sets to the exe name); full-screen/root capture is black here. Wayland-native windows \
                     cannot be captured this way."
                        .into(),
                );
            }
            if which::which("xprop").is_err() {
                diag.push("xprop not found: cannot locate the game window (pacman -S xorg-xprop).".into());
            }
        }
        Err(e) => diag.push(e.message.clone()),
    }
    out.push(BackendInfo {
        id: "capture:x11".into(),
        name: "X11 / XWayland capture".into(),
        available: x11.is_ok(),
        diagnostics: diag,
    });
    let portal_ok = std::env::var("DBUS_SESSION_BUS_ADDRESS").is_ok() || display.is_some() || wayland.is_some();
    out.push(BackendInfo {
        id: "capture:portal".into(),
        name: "xdg-desktop-portal screenshot".into(),
        available: portal_ok,
        diagnostics: vec![
            "Works on Wayland; GNOME asks for permission (dialog) so it is meant for manual captures, not periodic OCR.".into(),
        ],
    });
    out.push(BackendInfo {
        id: "capture:file".into(),
        name: "Image file".into(),
        available: true,
        diagnostics: vec!["OCR an existing screenshot (imagePath).".into()],
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const REGION: CaptureRegion = CaptureRegion { x: 10, y: 20, width: 640, height: 120 };

    #[test]
    fn ffmpeg_command_shape() {
        let spec = ffmpeg_x11_command(Path::new("/usr/bin/ffmpeg"), ":1", None, Some(REGION));
        let args = spec.args.join(" ");
        assert!(args.contains("-f x11grab"));
        assert!(args.contains("-video_size 640x120"));
        assert!(args.contains("-i :1+10,20"));
        assert!(args.ends_with("-frames:v 1 -f image2pipe -vcodec png -"));
        let full = ffmpeg_x11_command(Path::new("ffmpeg"), ":2", None, None);
        assert!(full.args.iter().any(|a| a == ":2"));
        assert!(!full.args.iter().any(|a| a == "-video_size"));
        let win = ffmpeg_x11_command(Path::new("ffmpeg"), ":1", Some("0x3000003"), Some(REGION)).args.join(" ");
        assert!(win.contains("-f x11grab -window_id 0x3000003 -video_size 640x120"), "{win}");
    }

    #[test]
    fn import_command_shape() {
        let spec = import_x11_command(Path::new("import"), ":1", None, Some(REGION));
        assert_eq!(
            spec.args,
            vec!["-display", ":1", "-window", "root", "-silent", "-crop", "640x120+10+20", "+repage", "png:-"]
        );
        let spec = import_x11_command(Path::new("import"), ":1", Some("0x42"), None);
        assert_eq!(spec.args, vec!["-display", ":1", "-window", "0x42", "-silent", "png:-"]);
    }

    #[test]
    fn xprop_parsing_and_window_matching() {
        // Captured with xprop on the dev machine (GTK X11 window) + a Wine-style entry.
        assert_eq!(
            parse_client_list("_NET_CLIENT_LIST(WINDOW): window id # 0x3000003, 0x400003"),
            vec!["0x3000003", "0x400003"]
        );
        assert!(parse_client_list("_NET_CLIENT_LIST:  not found.").is_empty());
        let gtk = parse_window_props(
            "0x3000003",
            "WM_CLASS(STRING) = \"xwin.py\", \"Xwin.py\"\n_NET_WM_PID(CARDINAL) = 969296\nWM_NAME(STRING) = \"vnl-ocr-test\"\n",
        );
        assert_eq!(gtk.class, vec!["xwin.py", "Xwin.py"]);
        assert_eq!(gtk.pid, Some(969296));
        assert_eq!(gtk.name.as_deref(), Some("vnl-ocr-test"));
        let wine = parse_window_props(
            "0x5",
            "WM_CLASS(STRING) = \"ゲーム.exe\", \"Wine\"\n_NET_WM_NAME(UTF8_STRING) = \"タイトル\"\n",
        );
        let windows = vec![gtk, wine];
        assert_eq!(match_window(&windows, "ゲーム.EXE").unwrap().id, "0x5");
        assert_eq!(match_window(&windows, "vnl-ocr-test.exe").unwrap().id, "0x3000003");
        assert!(match_window(&windows, "other.exe").is_none());
    }

    #[test]
    fn region_validation() {
        assert!(validate_region(&REGION).is_ok());
        assert!(validate_region(&CaptureRegion { x: 0, y: 0, width: 0, height: 10 }).is_err());
        assert!(validate_region(&CaptureRegion { x: -1, y: 0, width: 5, height: 10 }).is_err());
    }

    #[test]
    fn png_header_parsing() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend_from_slice(&800u32.to_be_bytes());
        png.extend_from_slice(&600u32.to_be_bytes());
        assert_eq!(png_size(&png), Some((800, 600)));
        assert_eq!(png_size(b"GIF89a"), None);
    }

    #[test]
    fn file_uri() {
        assert_eq!(file_uri_to_path("file:///tmp/a%20b.png").unwrap(), PathBuf::from("/tmp/a b.png"));
        assert!(file_uri_to_path("https://x/y.png").is_err());
    }

    /// Real xdg-desktop-portal screenshot (non-interactive). GNOME may show a
    /// permission dialog. Run with `-- --ignored real_portal --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn real_portal_screenshot() {
        let cap = PortalCapture { interactive: false, timeout: Duration::from_secs(20) };
        match cap.capture(None).await {
            Ok(img) => eprintln!("portal ok: {} bytes, size {:?}", img.bytes.len(), png_size(&img.bytes)),
            Err(e) => panic!("portal failed: {} {:?}", e.message, e.details),
        }
    }

    /// Real capture on the current X display (XWayland). Run with `-- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn real_x11_capture() {
        let mut cap = X11Capture::detect(None).expect("X11 capture tool");
        if let Ok(id) = std::env::var("VNL_X11_WINDOW") {
            cap = cap.with_target(X11Target::WindowId(id));
        }
        let img = cap.capture(Some(CaptureRegion { x: 0, y: 0, width: 320, height: 200 })).await.expect("capture");
        eprintln!("captured {} bytes via {:?}: {:?}", img.bytes.len(), cap.tool, png_size(&img.bytes));
        assert_eq!(png_size(&img.bytes), Some((320, 200)));
    }
}
