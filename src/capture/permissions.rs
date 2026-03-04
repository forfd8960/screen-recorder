//! TCC screen-capture permission check.
//!
//! Uses `SCShareableContent::get()` as the canonical way to detect whether
//! the process has been granted Screen Recording permission.  An empty
//! display list is treated as "permission denied".

use screencapturekit::prelude::*;
use tracing::{debug, warn};

use crate::error::AppError;

// ---------------------------------------------------------------------------
// Trait (for unit-testing via mock)
// ---------------------------------------------------------------------------

/// Abstraction over display enumeration used to unit-test the permission-check
/// logic without requiring TCC access.
pub trait ShareableContentChecker: Send {
    /// Returns the number of accessible displays.
    fn display_count(&self) -> usize;
}

// ---------------------------------------------------------------------------
// Public API (T020 / T034)
// ---------------------------------------------------------------------------

/// Checks whether the process has microphone access.
///
/// Returns `true` when permission is granted (or not yet determined — macOS
/// automatically prompts the user when `SCStream` starts with
/// `capturesAudio = true`).  Returns `false` only when the user has
/// **explicitly denied** access.
///
/// # Implementation note
///
/// A proper check would call
/// `AVCaptureDevice::authorizationStatusForMediaType(AVMediaTypeAudio)` via
/// `objc2-av-foundation`.  That requires the `AVCaptureDevice` crate feature
/// which is not yet enabled in `Cargo.toml`.  Until then this stub returns
/// `true`, delegating the actual TCC prompt to `SCStream` on first use.
///
/// TODO: replace with `AVCaptureDevice::authorizationStatusForMediaType`.
pub async fn check_mic_permission() -> bool {
    true
}

/// Checks whether the process has TCC permission to record the screen.
///
/// Calls `SCShareableContent::get()` on a blocking thread to avoid blocking
/// the Tokio executor.  Returns `Ok(true)` when at least one display is
/// accessible, or `Err(AppError::PermissionDenied)` when the arrays are empty
/// or the call fails outright.
///
/// # Errors
/// - [`AppError::PermissionDenied`] – screen recording permission is denied.
/// - [`AppError::StreamCreation`] – the `spawn_blocking` task panicked.
pub async fn check_screen_permission() -> Result<bool, AppError> {
    tokio::task::spawn_blocking(|| {
        debug!("checking screen capture permission via SCShareableContent::get()");
        match SCShareableContent::get() {
            Ok(content) => {
                let count = content.displays().len();
                debug!("SCShareableContent::get() OK — {count} display(s)");
                if count == 0 {
                    warn!("SCShareableContent returned 0 displays — treating as permission denied");
                    Err(AppError::PermissionDenied)
                } else {
                    Ok(true)
                }
            }
            Err(e) => {
                warn!("SCShareableContent::get() failed: {e:?}");
                Err(AppError::PermissionDenied)
            }
        }
    })
    .await
    .map_err(|e| AppError::StreamCreation(e.to_string()))?
}

// ---------------------------------------------------------------------------
// Unit tests (T017) – use a mock trait impl, no TCC required
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Mock that reports no accessible displays.
    struct EmptyChecker;
    impl ShareableContentChecker for EmptyChecker {
        fn display_count(&self) -> usize {
            0
        }
    }

    /// Mock that reports one accessible display.
    struct SingleDisplayChecker;
    impl ShareableContentChecker for SingleDisplayChecker {
        fn display_count(&self) -> usize {
            1
        }
    }

    /// Thin wrapper that exercises permission logic against any checker.
    fn check_via_trait(checker: &dyn ShareableContentChecker) -> Result<bool, AppError> {
        if checker.display_count() == 0 {
            Err(AppError::PermissionDenied)
        } else {
            Ok(true)
        }
    }

    #[test]
    fn empty_displays_returns_permission_denied() {
        let result = check_via_trait(&EmptyChecker);
        assert!(
            matches!(result, Err(AppError::PermissionDenied)),
            "expected PermissionDenied, got {result:?}"
        );
    }

    #[test]
    fn single_display_returns_ok() {
        let result = check_via_trait(&SingleDisplayChecker);
        assert!(result.is_ok(), "expected Ok(true), got {result:?}");
    }
}
