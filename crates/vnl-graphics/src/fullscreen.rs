//! Fullscreen assist for games running inside gamescope.
//!
//! A windowed Wine game inside gamescope keeps the (undrawn) Windows frame
//! area inside its X window: e.g. a 1280x960 game gets a 1286x992 window with
//! the picture in the top-left corner. gamescope scales the whole window, so
//! the picture is slightly downscaled and an empty band appears below it.
//! The game's own fullscreen mode uses a frameless popup that gamescope
//! scales correctly.
//!
//! [`ensure_game_fullscreen`] connects to gamescope's nested Xwayland,
//! waits for the game window to settle and, only if it does not already
//! cover the screen, sends Alt+Enter once via XTest (the fullscreen toggle of
//! most Windows VN engines). Verified with BGI/Ethornell under GE-Proton 11.

use std::time::{Duration, Instant};

use vnl_core::{AppError, ErrorCode};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt as _, MapState, Window};
use x11rb::protocol::xtest::ConnectionExt as _;

const XK_ALT_L: u32 = 0xffe9;
const XK_RETURN: u32 = 0xff0d;
const KEY_PRESS: u8 = 2;
const KEY_RELEASE: u8 = 3;
/// Smaller windows are splash screens, tooltips or helper windows.
const MIN_GAME_WIDTH: u16 = 320;
const MIN_GAME_HEIGHT: u16 = 240;
const POLL: Duration = Duration::from_millis(500);
/// The game window must keep the same geometry this long before acting.
const SETTLE: Duration = Duration::from_millis(1500);
const VERIFY: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowGeometry {
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullscreenOutcome {
    /// The game window already covered the nested display; nothing sent.
    AlreadyFullscreen,
    /// Alt+Enter was sent and the window now covers the display.
    Switched,
    /// Alt+Enter was sent but the window did not change (engine without that shortcut).
    Unchanged(WindowGeometry),
    /// No game-sized window appeared before the timeout.
    NoWindow,
}

/// Extract the nested display from a gamescope log line such as
/// `[gamescope] [Info]  wlserver: [xwayland/server.c:108] Starting Xwayland on :3`.
pub fn parse_xwayland_display(line: &str) -> Option<String> {
    let rest = &line[line.find("Starting Xwayland on ")? + "Starting Xwayland on ".len()..];
    let display: String = rest.chars().take_while(|c| *c == ':' || c.is_ascii_digit()).collect();
    (display.len() > 1 && display.starts_with(':')).then_some(display)
}

/// Largest viewable, non-override-redirect window that is big enough to be a game.
pub fn pick_game_window(windows: &[(Window, WindowGeometry)]) -> Option<(Window, WindowGeometry)> {
    windows
        .iter()
        .filter(|(_, g)| g.width >= MIN_GAME_WIDTH && g.height >= MIN_GAME_HEIGHT)
        .max_by_key(|(_, g)| u32::from(g.width) * u32::from(g.height))
        .copied()
}

pub fn covers(window: WindowGeometry, root: WindowGeometry) -> bool {
    window.x <= 0 && window.y <= 0 && window.width >= root.width && window.height >= root.height
}

/// Keycode producing `keysym` in a `GetKeyboardMapping` reply.
pub fn keycode_for(keysym: u32, min_keycode: u8, keysyms_per_keycode: u8, keysyms: &[u32]) -> Option<u8> {
    let per = usize::from(keysyms_per_keycode.max(1));
    keysyms
        .chunks(per)
        .position(|syms| syms.contains(&keysym))
        .and_then(|i| u8::try_from(usize::from(min_keycode) + i).ok())
}

fn x11_error(e: impl std::fmt::Display) -> AppError {
    AppError::new(ErrorCode::Process, "cannot talk to gamescope's Xwayland").with_details(e.to_string())
}

struct Display {
    conn: x11rb::rust_connection::RustConnection,
    root: Window,
}

impl Display {
    fn connect(name: &str) -> Result<Self, AppError> {
        let (conn, screen) = x11rb::connect(Some(name)).map_err(x11_error)?;
        let root = conn.setup().roots[screen].root;
        Ok(Self { conn, root })
    }

    fn geometry(&self, window: Window) -> Result<WindowGeometry, AppError> {
        let g = self.conn.get_geometry(window).map_err(x11_error)?.reply().map_err(x11_error)?;
        Ok(WindowGeometry { x: g.x, y: g.y, width: g.width, height: g.height })
    }

    fn top_level_windows(&self) -> Result<Vec<(Window, WindowGeometry)>, AppError> {
        let tree = self.conn.query_tree(self.root).map_err(x11_error)?.reply().map_err(x11_error)?;
        let mut out = Vec::new();
        for window in tree.children {
            // Windows can disappear between the tree query and these requests.
            let Ok(Ok(attrs)) = self.conn.get_window_attributes(window).map(|c| c.reply()) else { continue };
            if attrs.map_state != MapState::VIEWABLE || attrs.override_redirect {
                continue;
            }
            if let Ok(g) = self.geometry(window) {
                out.push((window, g));
            }
        }
        Ok(out)
    }

    fn send_alt_enter(&self) -> Result<(), AppError> {
        let setup = self.conn.setup();
        let (min, max) = (setup.min_keycode, setup.max_keycode);
        let map = self.conn.get_keyboard_mapping(min, max - min + 1).map_err(x11_error)?.reply().map_err(x11_error)?;
        let code = |sym| {
            keycode_for(sym, min, map.keysyms_per_keycode, &map.keysyms)
                .ok_or_else(|| AppError::internal(format!("no keycode for keysym {sym:#x}")))
        };
        let (alt, ret) = (code(XK_ALT_L)?, code(XK_RETURN)?);
        for (kind, key) in [(KEY_PRESS, alt), (KEY_PRESS, ret), (KEY_RELEASE, ret), (KEY_RELEASE, alt)] {
            self.conn.xtest_fake_input(kind, key, x11rb::CURRENT_TIME, self.root, 0, 0, 0).map_err(x11_error)?;
            self.conn.flush().map_err(x11_error)?;
            std::thread::sleep(Duration::from_millis(40));
        }
        // Round trip so the events are processed before we start verifying.
        self.conn.get_input_focus().map_err(x11_error)?.reply().map_err(x11_error)?;
        Ok(())
    }
}

/// Blocking: wait up to `timeout` for the game window on `display`, then make
/// sure it is fullscreen (see module docs). Sends at most one Alt+Enter.
pub fn ensure_game_fullscreen(display: &str, timeout: Duration) -> Result<FullscreenOutcome, AppError> {
    let deadline = Instant::now() + timeout;
    // Xwayland starts slightly after gamescope logs its display.
    let x = loop {
        match Display::connect(display) {
            Ok(x) => break x,
            Err(e) if Instant::now() >= deadline => return Err(e),
            Err(_) => std::thread::sleep(POLL),
        }
    };
    let root = x.geometry(x.root)?;

    let mut stable: Option<(Window, WindowGeometry, Instant)> = None;
    let (window, geometry) = loop {
        if Instant::now() >= deadline {
            return Ok(FullscreenOutcome::NoWindow);
        }
        match pick_game_window(&x.top_level_windows()?) {
            Some((w, g)) => match stable {
                Some((sw, sg, since)) if sw == w && sg == g => {
                    if since.elapsed() >= SETTLE {
                        break (w, g);
                    }
                }
                _ => stable = Some((w, g, Instant::now())),
            },
            None => stable = None,
        }
        std::thread::sleep(POLL);
    };

    if covers(geometry, root) {
        return Ok(FullscreenOutcome::AlreadyFullscreen);
    }
    x.send_alt_enter()?;

    let verify_until = Instant::now() + VERIFY;
    let mut last = geometry;
    while Instant::now() < verify_until {
        std::thread::sleep(POLL);
        // The engine may recreate its window when switching modes.
        let current = pick_game_window(&x.top_level_windows()?).map(|(_, g)| g).or_else(|| x.geometry(window).ok());
        if let Some(g) = current {
            if covers(g, root) {
                return Ok(FullscreenOutcome::Switched);
            }
            last = g;
        }
    }
    Ok(FullscreenOutcome::Unchanged(last))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(x: i16, y: i16, width: u16, height: u16) -> WindowGeometry {
        WindowGeometry { x, y, width, height }
    }

    #[test]
    fn parses_display_from_gamescope_log() {
        let line = "[gamescope] [\u{1b}[0;34mInfo\u{1b}[0m]  wlserver: [xwayland/server.c:108] Starting Xwayland on :3";
        assert_eq!(parse_xwayland_display(line).as_deref(), Some(":3"));
        assert_eq!(parse_xwayland_display("Starting Xwayland on :12\n").as_deref(), Some(":12"));
        assert_eq!(parse_xwayland_display("Starting Xwayland on"), None);
        assert_eq!(parse_xwayland_display("Running compositor on wayland display 'gamescope-0'"), None);
    }

    #[test]
    fn picks_largest_game_sized_window() {
        let windows = [(1, g(0, 0, 1, 1)), (2, g(313, 35, 1286, 992)), (3, g(10, 10, 300, 200))];
        assert_eq!(pick_game_window(&windows), Some((2, g(313, 35, 1286, 992))));
        assert_eq!(pick_game_window(&[(1, g(0, 0, 1, 1))]), None);
    }

    #[test]
    fn framed_window_is_not_fullscreen() {
        let root = g(0, 0, 1920, 1080);
        assert!(!covers(g(313, 35, 1286, 992), root));
        assert!(covers(g(0, 0, 1920, 1080), root));
        assert!(!covers(g(0, 0, 1920, 1046), root));
    }

    #[test]
    fn finds_keycodes_in_mapping() {
        // min keycode 8, 2 keysyms per keycode.
        let keysyms = [0, 0, 0x61, 0x41, XK_RETURN, 0, XK_ALT_L, 0xffe7];
        assert_eq!(keycode_for(XK_RETURN, 8, 2, &keysyms), Some(10));
        assert_eq!(keycode_for(XK_ALT_L, 8, 2, &keysyms), Some(11));
        assert_eq!(keycode_for(0x1234, 8, 2, &keysyms), None);
    }
}
