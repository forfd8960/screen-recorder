//! Domain error type for the screen recorder application.
//!
//! All fallible operations in `capture`, `encode`, `output`, and `config`
//! return `Result<T, AppError>`.  The UI layer pattern-matches on the variant
//! to decide between modal dialogs, non-blocking banners, and onboarding
//! screens (see `src/app.rs` and `src/ui/main_window.rs`).

/// Application-wide error type.
///
/// Variants are ordered from most-specific (permission and stream errors) to
/// most-general (`Io`).  Add new variants here rather than using `anyhow`
/// inside library modules.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// The macOS TCC subsystem has not granted screen-recording access.
    ///
    /// The UI should present an onboarding screen directing the user to
    /// **System Settings → Privacy & Security → Screen Recording**.
    #[error(
        "Screen capture permission denied — enable Screen Recording in \
         System Settings → Privacy & Security"
    )]
    PermissionDenied,

    /// `SCShareableContent` returned empty display and window arrays.
    ///
    /// Usually indicates either a permission denial or a system-level issue
    /// enumerating displays.
    #[error("No shareable content available — check Screen Recording permission")]
    NoShareableContent,

    /// The `SCStream` could not be created or started.
    #[error("Failed to create capture stream: {0}")]
    StreamCreation(String),

    /// An error occurred inside the `AVAssetWriter` encoding pipeline.
    #[error("Encoding pipeline error: {0}")]
    EncodingError(String),

    /// A file-system operation failed.
    ///
    /// Wraps [`std::io::Error`] via the `#[from]` derive so that the `?`
    /// operator converts `io::Error` to `AppError::Io` automatically.
    #[error("File I/O error: {source}")]
    Io {
        /// The underlying I/O error.
        #[from]
        source: std::io::Error,
    },

    /// Microphone access was denied or the device is unavailable.
    ///
    /// The application continues with video-only recording and displays a
    /// non-blocking banner in the UI.
    #[error("Microphone unavailable — recording will continue with video only")]
    MicrophoneUnavailable,
}
