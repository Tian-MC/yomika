//! `OverlayController`: shows/hides the translation overlay window. The Tauri
//! implementation lives in `src-tauri/src/commands/translation.rs` (this crate
//! never depends on Tauri). The overlay page listens to `vnl://translation`.
//!
//! Wayland note: positioning and always-on-top are compositor-controlled.
//! On GNOME/Mutter an always-on-top request is honoured for normal windows
//! but a fullscreen game (or gamescope's fullscreen window) is usually placed
//! above it; run the game windowed/borderless, or use gamescope windowed.
//! Under X11/XWayland window managers the hint generally works.

use vnl_core::{AppError, OverlayConfig};

pub trait OverlayController: Send + Sync {
    fn show(&self, config: &OverlayConfig) -> Result<(), AppError>;
    fn hide(&self) -> Result<(), AppError>;
    fn is_visible(&self) -> bool;
}

/// No-op controller (headless use and tests).
#[derive(Default)]
pub struct NullOverlay {
    visible: std::sync::atomic::AtomicBool,
}

impl OverlayController for NullOverlay {
    fn show(&self, _config: &OverlayConfig) -> Result<(), AppError> {
        self.visible.store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
    fn hide(&self) -> Result<(), AppError> {
        self.visible.store(false, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
    fn is_visible(&self) -> bool {
        self.visible.load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// Clamps user-provided overlay settings to sane values.
pub fn sanitize(config: &OverlayConfig) -> OverlayConfig {
    OverlayConfig {
        enabled: config.enabled,
        font_size: config.font_size.clamp(8, 96),
        opacity: if config.opacity.is_finite() { config.opacity.clamp(0.1, 1.0) } else { 0.85 },
        show_original: config.show_original,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes() {
        let c = sanitize(&OverlayConfig { enabled: true, font_size: 400, opacity: f32::NAN, show_original: false });
        assert_eq!(c.font_size, 96);
        assert_eq!(c.opacity, 0.85);
        let o = NullOverlay::default();
        o.show(&c).unwrap();
        assert!(o.is_visible());
        o.hide().unwrap();
        assert!(!o.is_visible());
    }
}
