//! Screen capture (`ScreenCapture`) for OCR.
//!
//! What works on Linux, verified on GNOME 51 / Wayland with XWayland:
//! - **X11 (`x11`)**: `ffmpeg -f x11grab` on `$DISPLAY` (or gamescope's
//!   nested display). GNOME's XWayland is *rootless*: the root window only
//!   contains X11 clients, i.e. Wine/Proton games (which use X11 unless the
//!   experimental winewayland driver is enabled) — exactly what we want.
//!   Wayland-native windows appear black. ImageMagick `import -window root`
//!   is used as a fallback when ffmpeg is missing (on the test machine it
//!   failed against rootless XWayland: "missing an image filename").
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
pub fn ffmpeg_x11_command(ffmpeg: &Path, display: &str, region: Option<CaptureRegion>) -> CommandSpec {
    let mut spec = CommandSpec::new(ffmpeg).args(["-hide_banner", "-loglevel", "error", "-nostdin", "-f", "x11grab"]);
    let input = match region {
        Some(r) => {
            spec = spec.args(["-video_size".to_string(), format!("{}x{}", r.width, r.height)]);
            format!("{display}+{},{}", r.x, r.y)
        }
        None => display.to_string(),
    };
    spec.args(["-draw_mouse", "0", "-i"])
        .arg(input)
        .args(["-frames:v", "1", "-f", "image2pipe", "-vcodec", "png", "-"])
}

/// ImageMagick `import` invocation writing one PNG to stdout.
pub fn import_x11_command(import: &Path, display: &str, region: Option<CaptureRegion>) -> CommandSpec {
    let mut spec = CommandSpec::new(import).args(["-display", display, "-window", "root", "-silent"]);
    if let Some(r) = region {
        spec = spec.args(["-crop".to_string(), format!("{}x{}+{}+{}", r.width, r.height, r.x, r.y), "+repage".to_string()]);
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

pub struct X11Capture {
    pub display: String,
    pub tool: X11Tool,
    pub program: PathBuf,
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
        if !display.starts_with(':') && !display.contains(':') {
            return Err(AppError::invalid(format!("'{display}' is not an X11 display name like ':1'")));
        }
        if let Ok(p) = which::which("ffmpeg") {
            return Ok(Self { display, tool: X11Tool::Ffmpeg, program: p });
        }
        if let Ok(p) = which::which("import") {
            return Ok(Self { display, tool: X11Tool::Import, program: p });
        }
        Err(AppError::new(ErrorCode::MissingDependency, "X11 capture needs ffmpeg or ImageMagick")
            .with_details("Install with: pacman -S ffmpeg  (or: apt install ffmpeg / dnf install ffmpeg)"))
    }

    pub fn command(&self, region: Option<CaptureRegion>) -> CommandSpec {
        match self.tool {
            X11Tool::Ffmpeg => ffmpeg_x11_command(&self.program, &self.display, region),
            X11Tool::Import => import_x11_command(&self.program, &self.display, region),
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
        let bytes = run_capture_command(&self.command(region), Duration::from_secs(10)).await?;
        if !is_png(&bytes) {
            return Err(AppError::new(ErrorCode::Process, "capture tool did not return a PNG image"));
        }
        Ok(CapturedImage { bytes, source: format!("x11 {}", self.display) })
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
                AppError::new(ErrorCode::Unsupported, "screenshot portal did not answer (permission dialog not confirmed?)")
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
                    "Wayland session: only X11/XWayland windows (Wine/Proton games) are visible to X11 capture; \
                     for gamescope pass its nested display (e.g. :2)."
                        .into(),
                );
            }
        }
        Err(e) => diag.push(e.message.clone()),
    }
    out.push(BackendInfo { id: "capture:x11".into(), name: "X11 / XWayland capture".into(), available: x11.is_ok(), diagnostics: diag });
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
        let spec = ffmpeg_x11_command(Path::new("/usr/bin/ffmpeg"), ":1", Some(REGION));
        let args = spec.args.join(" ");
        assert!(args.contains("-f x11grab"));
        assert!(args.contains("-video_size 640x120"));
        assert!(args.contains("-i :1+10,20"));
        assert!(args.ends_with("-frames:v 1 -f image2pipe -vcodec png -"));
        let full = ffmpeg_x11_command(Path::new("ffmpeg"), ":2", None);
        assert!(full.args.iter().any(|a| a == ":2"));
        assert!(!full.args.iter().any(|a| a == "-video_size"));
    }

    #[test]
    fn import_command_shape() {
        let spec = import_x11_command(Path::new("import"), ":1", Some(REGION));
        assert_eq!(
            spec.args,
            vec!["-display", ":1", "-window", "root", "-silent", "-crop", "640x120+10+20", "+repage", "png:-"]
        );
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

    /// Real capture on the current X display (XWayland). Run with `-- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn real_x11_capture() {
        let cap = X11Capture::detect(None).expect("X11 capture tool");
        let img = cap.capture(Some(CaptureRegion { x: 0, y: 0, width: 320, height: 200 })).await.expect("capture");
        eprintln!("captured {} bytes via {:?}: {:?}", img.bytes.len(), cap.tool, png_size(&img.bytes));
        assert_eq!(png_size(&img.bytes), Some((320, 200)));
    }
}
