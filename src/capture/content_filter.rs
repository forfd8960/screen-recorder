//! `SCContentFilter` builder and display/window enumeration.
//!
//! This module translates the application-level [`CaptureRegion`] into an
//! `SCContentFilter` suitable for passing to `SCStream`.  All public
//! functions that call into `ScreenCaptureKit` are **synchronous blocking** and
//! must be invoked from `tokio::task::spawn_blocking`.
//!
//! # Self-exclusion
//!
//! By default, [`build_filter`] excludes the current process from the capture.
//! This prevents the recorder's own windows from appearing in the recording.
//! The exclusion is implemented by locating the [`SCRunningApplication`] whose
//! [`process_id`](screencapturekit::shareable_content::SCRunningApplication::process_id)
//! matches `std::process::id()` and passing it to
//! `SCContentFilterBuilder::with_excluding_applications`.
//!
//! # Area Capture
//!
//! Rectangular sub-region capture requires macOS 14.2+ via
//! `SCContentFilter::set_content_rect`.  If the `screencapturekit` crate is
//! compiled without the `macos_14_2` feature the Area variant falls back to
//! full-display capture while still applying rect validation.

use screencapturekit::{
    shareable_content::{SCDisplay, SCRunningApplication, SCShareableContent, SCWindow},
    stream::content_filter::SCContentFilter,
};

use crate::{
    config::settings::{CaptureRegion, Rect},
    error::AppError,
};

// ---------------------------------------------------------------------------
// Public lightweight info types (used by AppState / UI)
// ---------------------------------------------------------------------------

/// Lightweight snapshot of a display's identity and geometry.
///
/// Stored in [`AppState`](crate::app::AppState) so the UI can enumerate
/// available displays without holding live `SCDisplay` references.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayInfo {
    /// Core Graphics display identifier.
    pub display_id: u32,
    /// Native pixel width.
    pub width: u32,
    /// Native pixel height.
    pub height: u32,
}

impl std::fmt::Display for DisplayInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Display {} ({}×{})",
            self.display_id, self.width, self.height
        )
    }
}

/// Lightweight snapshot of a window's identity and name.
///
/// Stored in [`AppState`](crate::app::AppState) so the UI can enumerate
/// available windows without holding live `SCWindow` references.
#[derive(Debug, Clone)]
pub struct WindowInfo {
    /// `ScreenCaptureKit` window identifier.
    pub window_id: u32,
    /// Window title, or `"<untitled>"` if unavailable.
    pub title: String,
    /// Name of the owning application.
    pub app_name: String,
}

impl std::fmt::Display for WindowInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} — {}", self.app_name, self.title)
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Returns lightweight metadata for all currently available displays.
///
/// **Must be called from `spawn_blocking`** — `SCShareableContent::get()` is
/// a blocking FFI call that must not run on a Tokio worker thread.
///
/// # Errors
///
/// Returns [`AppError::StreamCreation`] if `SCShareableContent::get()` fails
/// or [`AppError::NoShareableContent`] if the display list is empty.
pub fn list_displays() -> Result<Vec<DisplayInfo>, AppError> {
    let content = get_content()?;
    let displays: Vec<DisplayInfo> = content
        .displays()
        .iter()
        .map(|d| DisplayInfo {
            display_id: d.display_id(),
            width: d.width(),
            height: d.height(),
        })
        .collect();
    Ok(displays)
}

/// Returns lightweight metadata for all currently on-screen windows.
///
/// **Must be called from `spawn_blocking`**.
///
/// # Errors
///
/// Returns [`AppError::StreamCreation`] if `SCShareableContent::get()` fails.
pub fn list_windows() -> Result<Vec<WindowInfo>, AppError> {
    let content = get_content()?;
    let windows: Vec<WindowInfo> = content
        .windows()
        .iter()
        .map(|w| WindowInfo {
            window_id: w.window_id(),
            title: w.title().unwrap_or_else(|| "<untitled>".to_string()),
            app_name: w
                .owning_application()
                .map_or_else(|| "<unknown>".to_string(), |a| a.application_name()),
        })
        .collect();
    Ok(windows)
}

/// Builds an `SCContentFilter` for the given [`CaptureRegion`].
///
/// Also returns the pixel dimensions `(width, height)` that should be used
/// when configuring `SCStreamConfiguration` — these are the effective capture
/// dimensions for the chosen region.
///
/// Self-exclusion: the current process is always excluded from display/area
/// captures so the recorder's own windows do not appear in the recording.
///
/// **Must be called from `spawn_blocking`**.
///
/// # Errors
///
/// * [`AppError::InvalidRegion`] — `Area` rect has non-positive width or height.
/// * [`AppError::NoShareableContent`] — no displays or the requested window/display is missing.
/// * [`AppError::StreamCreation`] — `SCShareableContent::get()` failed.
pub fn build_filter(region: &CaptureRegion) -> Result<(SCContentFilter, u32, u32), AppError> {
    match region {
        CaptureRegion::FullScreen { display_id } => {
            let content = get_content()?;
            let display = find_display(&content, *display_id)?;
            let (w, h) = (display.width(), display.height());
            let filter = build_display_filter_with_self_exclusion(&display, &content);
            Ok((filter, w, h))
        }

        CaptureRegion::Window { window_id } => {
            let content = get_content()?;
            let window = find_window(&content, *window_id)?;
            let frame = window.frame();
            #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
            let (w, h) = (frame.width.max(0.0) as u32, frame.height.max(0.0) as u32);
            let filter = SCContentFilter::create().with_window(&window).build();
            Ok((filter, w.max(1), h.max(1)))
        }

        CaptureRegion::Area { rect } => {
            validate_rect(rect)?;
            let content = get_content()?;
            // Use the primary display for the area capture; content_rect
            // (macOS 14.2+) is set below when the feature is compiled in.
            let display = find_primary_display(&content)?;
            let (dw, dh) = (display.width(), display.height());
            let filter = build_display_filter_with_self_exclusion(&display, &content);

            // macOS 14.2+: set the capture rect on the filter.
            #[cfg(feature = "macos_14_2")]
            let filter = {
                use screencapturekit::cg::CGRect;
                let cg_rect = CGRect::new(rect.x, rect.y, rect.width, rect.height);
                filter.set_content_rect(cg_rect)
            };

            // Effective dimensions: use rect dimensions if set; fall back to display.
            #[cfg(feature = "macos_14_2")]
            let (ew, eh) = {
                #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
                (rect.width as u32, rect.height as u32)
            };
            #[cfg(not(feature = "macos_14_2"))]
            let (ew, eh) = (dw, dh);
            let _ = (dw, dh); // suppress unused warning in macos_14_2 build

            Ok((filter, ew, eh))
        }
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Fetches the current shareable content (blocking).
fn get_content() -> Result<SCShareableContent, AppError> {
    SCShareableContent::get()
        .map_err(|e| AppError::StreamCreation(format!("SCShareableContent::get failed: {e:?}")))
}

/// Finds a display by its `display_id`.  If `display_id == 0` returns the
/// primary (first) display.
fn find_display(content: &SCShareableContent, display_id: u32) -> Result<SCDisplay, AppError> {
    let displays = content.displays();
    if displays.is_empty() {
        return Err(AppError::NoShareableContent);
    }
    if display_id == 0 {
        return Ok(displays[0].clone());
    }
    displays
        .iter()
        .find(|d| d.display_id() == display_id)
        .cloned()
        .ok_or_else(|| AppError::StreamCreation(format!("Display with id {display_id} not found")))
}

/// Returns the primary (first) display, or [`AppError::NoShareableContent`].
fn find_primary_display(content: &SCShareableContent) -> Result<SCDisplay, AppError> {
    content
        .displays()
        .first()
        .cloned()
        .ok_or(AppError::NoShareableContent)
}

/// Finds a window by its `window_id`.
fn find_window(content: &SCShareableContent, window_id: u32) -> Result<SCWindow, AppError> {
    content
        .windows()
        .iter()
        .find(|w| w.window_id() == window_id)
        .cloned()
        .ok_or_else(|| AppError::StreamCreation(format!("Window with id {window_id} not found")))
}

/// Builds a display-capturing filter that excludes the current process.
///
/// Self-exclusion prevents the recorder's own UI from appearing in recordings.
fn build_display_filter_with_self_exclusion(
    display: &SCDisplay,
    content: &SCShareableContent,
) -> SCContentFilter {
    let current_pid = std::process::id().cast_signed();
    let apps: Vec<SCRunningApplication> = content
        .applications()
        .iter()
        .filter(|a| a.process_id() == current_pid)
        .map(Clone::clone)
        .collect();

    if apps.is_empty() {
        // Self not found in application list (e.g. in unit tests / CI);
        // fall back to a plain display filter with no exclusions.
        SCContentFilter::create()
            .with_display(display)
            .with_excluding_windows(&[])
            .build()
    } else {
        let app_refs: Vec<&SCRunningApplication> = apps.iter().collect();
        SCContentFilter::create()
            .with_display(display)
            .with_excluding_applications(&app_refs, &[])
            .build()
    }
}

/// Validates a [`Rect`] for area capture.
///
/// Returns [`AppError::InvalidRegion`] if either dimension is ≤ 0.
fn validate_rect(rect: &Rect) -> Result<(), AppError> {
    if rect.width <= 0.0 {
        return Err(AppError::InvalidRegion(format!(
            "rect.width must be > 0, got {}",
            rect.width
        )));
    }
    if rect.height <= 0.0 {
        return Err(AppError::InvalidRegion(format!(
            "rect.height must be > 0, got {}",
            rect.height
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Unit tests  (T037 + T038)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::settings::{CaptureRegion, Rect};

    // T038 — pure Rust, no SCKit required.

    /// Zero-width rect must produce `AppError::InvalidRegion`.
    #[test]
    fn area_zero_width_returns_invalid_region() {
        let region = CaptureRegion::Area {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 720.0,
            },
        };
        // We only need to exercise `validate_rect` — no live SCKit session.
        let rect = match &region {
            CaptureRegion::Area { rect } => rect,
            _ => panic!("expected Area"),
        };
        let result = validate_rect(rect);
        assert!(
            matches!(result, Err(AppError::InvalidRegion(_))),
            "expected InvalidRegion, got {result:?}"
        );
    }

    /// Zero-height rect must produce `AppError::InvalidRegion`.
    #[test]
    fn area_zero_height_returns_invalid_region() {
        let rect = Rect {
            x: 100.0,
            y: 100.0,
            width: 1280.0,
            height: 0.0,
        };
        let result = validate_rect(&rect);
        assert!(
            matches!(result, Err(AppError::InvalidRegion(_))),
            "expected InvalidRegion for height=0, got {result:?}"
        );
    }

    /// Negative dimensions must also be rejected.
    #[test]
    fn area_negative_dimensions_returns_invalid_region() {
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: -100.0,
            height: 200.0,
        };
        assert!(matches!(
            validate_rect(&rect),
            Err(AppError::InvalidRegion(_))
        ));
    }

    /// A positive rect must pass validation.
    #[test]
    fn area_valid_rect_passes_validation() {
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 1280.0,
            height: 720.0,
        };
        assert!(validate_rect(&rect).is_ok());
    }

    // T037 — integration tests (require screen-recording permission + real display).

    /// `build_filter(FullScreen { display_id: 0 })` must succeed on a real Mac
    /// with screen-recording permission and at least one display.
    #[cfg_attr(not(feature = "integration"), ignore)]
    #[test]
    fn build_filter_full_screen_primary_returns_ok() {
        let region = CaptureRegion::FullScreen { display_id: 0 };
        let result = build_filter(&region);
        assert!(
            result.is_ok(),
            "expected Ok for FullScreen primary display, got {result:?}"
        );
        let (_filter, w, h) = result.unwrap();
        assert!(w > 0, "width should be > 0");
        assert!(h > 0, "height should be > 0");
    }

    /// `build_filter(Window { window_id })` must produce a valid filter when
    /// a window with the given ID exists.
    #[cfg_attr(not(feature = "integration"), ignore)]
    #[test]
    fn build_filter_first_window_returns_ok() {
        let content = SCShareableContent::get().expect("need screen-recording permission");
        let windows = content.windows();
        if windows.is_empty() {
            // No windows to capture — skip gracefully.
            return;
        }
        let window_id = windows[0].window_id();
        let region = CaptureRegion::Window { window_id };
        let result = build_filter(&region);
        assert!(
            result.is_ok(),
            "expected Ok for Window filter, got {result:?}"
        );
    }

    /// `build_filter(FullScreen)` self-exclusion: the returned filter excludes
    /// the current process.  We verify indirectly by checking `build_filter`
    /// returns `Ok` — the actual exclusion is enforced by
    /// `build_display_filter_with_self_exclusion`.
    #[cfg_attr(not(feature = "integration"), ignore)]
    #[test]
    fn build_filter_full_screen_self_exclusion_does_not_panic() {
        let region = CaptureRegion::FullScreen { display_id: 0 };
        // Simply asserting no panic is sufficient — the self-exclusion logic
        // cannot be introspected without SCKit internals.
        let result = build_filter(&region);
        assert!(
            result.is_ok(),
            "self-exclusion build must not error: {result:?}"
        );
    }
}
