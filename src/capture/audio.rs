//! Audio capture configuration helpers.
//!
//! [`audio_capture_params`] derives the audio stream parameters from
//! [`RecordingSettings`] and returns them as a plain tuple so that
//! [`super::engine`] can apply them to the `SCStreamConfiguration` builder.
//!
//! # macOS 15+ note
//!
//! The `captureMicrophone` property of `SCStreamConfiguration` was added in
//! macOS 15.0 and is **not yet exposed** by `screencapturekit` 1.5.1.  Any
//! migration to a future crate version that adds the method should guard the
//! call with a runtime `os_version >= 15.0` check via `NSProcessInfo`, or a
//! compile-time `#[cfg(…)]` gate once the SDK feature is stabilised.

use crate::config::settings::RecordingSettings;

// ---------------------------------------------------------------------------
// Public API (T032)
// ---------------------------------------------------------------------------

/// Returns `(captures_audio, sample_rate_hz, channel_count)` for use with
/// `SCStreamConfiguration`.
///
/// * `captures_audio`  – mirrors `settings.capture_mic`.
/// * `sample_rate_hz`  – always `48_000` Hz (required by the AAC encoder).
/// * `channel_count`   – always `2` (stereo).
///
/// Both numeric values are `i32` to match `SCStreamConfiguration::with_sample_rate`
/// and `::with_channel_count` which accept `impl Into<i32>`.
///
/// The `captureMicrophone` field (macOS 15+) is intentionally omitted; it is
/// not yet accessible through `screencapturekit` 1.5.1.
#[must_use]
pub const fn audio_capture_params(settings: &RecordingSettings) -> (bool, i32, i32) {
    (settings.capture_mic, 48_000, 2)
}

// ---------------------------------------------------------------------------
// Unit tests (T030)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::settings::RecordingSettings;

    fn settings_with_mic(capture_mic: bool) -> RecordingSettings {
        RecordingSettings {
            capture_mic,
            ..Default::default()
        }
    }

    /// T030: `captures_audio` is `true` when `capture_mic` is enabled.
    #[test]
    fn captures_audio_true_when_mic_enabled() {
        let (captures_audio, sample_rate, channel_count) =
            audio_capture_params(&settings_with_mic(true));
        assert!(
            captures_audio,
            "captures_audio should be true when capture_mic = true"
        );
        assert_eq!(sample_rate, 48_000, "sample rate must be 48 000 Hz");
        assert_eq!(channel_count, 2, "channel count must be 2 (stereo)");
    }

    /// T030: `captures_audio` is `false` when `capture_mic` is disabled.
    #[test]
    fn captures_audio_false_when_mic_disabled() {
        let (captures_audio, sample_rate, channel_count) =
            audio_capture_params(&settings_with_mic(false));
        assert!(
            !captures_audio,
            "captures_audio should be false when capture_mic = false"
        );
        assert_eq!(sample_rate, 48_000, "sample rate must be 48 000 Hz");
        assert_eq!(channel_count, 2, "channel count must be 2 (stereo)");
    }

    /// T030: `captureMicrophone` (macOS 15+) is intentionally absent from the
    /// returned tuple — `screencapturekit` 1.5.1 does not expose this field.
    /// This test documents the omission and will need revisiting if the crate
    /// ever gains the `with_captures_microphone()` builder method.
    #[test]
    fn no_capture_microphone_field_in_params() {
        // The function returns exactly a 3-tuple.
        // There is deliberately no fourth element for captureMicrophone.
        let (captures_audio, _sample_rate, _channel_count) =
            audio_capture_params(&settings_with_mic(true));
        // Just verify the first field is correct; the absence of a fourth
        // element is enforced by the type system.
        assert!(captures_audio);
    }
}
