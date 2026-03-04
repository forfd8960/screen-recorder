//! User-configurable recording settings.
//!
//! [`RecordingSettings`] is the single source of truth for all parameters
//! passed to the capture and encoding pipelines.  It is serialized to JSON
//! at `~/Library/Application Support/screen-recorder/settings.json` and
//! loaded on startup.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::AppError;

// ---------------------------------------------------------------------------
// Enumerations
// ---------------------------------------------------------------------------

/// Output video resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Resolution {
    /// Use the display's native pixel dimensions.
    #[default]
    Native,
    /// 1920 × 1080 (aspect ratio preserved).
    P1080,
    /// 1280 × 720 (aspect ratio preserved).
    P720,
}

/// Video encoding quality preset.
///
/// Maps to an average bit-rate target passed to the `VideoToolbox` hardware
/// encoder via `AVVideoAverageBitRateKey`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum VideoQuality {
    /// 8 Mbps — highest quality, largest file.
    #[default]
    High,
    /// 4 Mbps — balanced quality/size trade-off.
    Medium,
    /// 2 Mbps — smallest file, suitable for screen demos.
    Low,
}

impl VideoQuality {
    /// Returns the target average bit rate in bits per second.
    #[must_use]
    pub const fn bitrate_bps(self) -> u32 {
        match self {
            Self::High => 8_000_000,
            Self::Medium => 4_000_000,
            Self::Low => 2_000_000,
        }
    }
}

// ---------------------------------------------------------------------------
// Rect (platform-independent capture region geometry)
// ---------------------------------------------------------------------------

/// A platform-independent rectangle used for the [`CaptureRegion::Area`] variant.
///
/// We avoid `CGRect` here to keep `serde` support and separation between the
/// settings layer and the Apple framework layer.  [`crate::capture::content_filter`]
/// converts this into a `CGRect` when building the `SCContentFilter`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    /// Horizontal origin in display points (top-left = 0, 0).
    pub x: f64,
    /// Vertical origin in display points.
    pub y: f64,
    /// Width in display points; must be > 0 to be valid.
    pub width: f64,
    /// Height in display points; must be > 0 to be valid.
    pub height: f64,
}

// ---------------------------------------------------------------------------
// CaptureRegion
// ---------------------------------------------------------------------------

/// Specifies which part of the screen to capture.
///
/// Maps 1-to-1 to `SCContentFilter` variants (see
/// `src/capture/content_filter.rs`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CaptureRegion {
    /// Capture an entire display, identified by its Core Graphics display ID.
    FullScreen {
        /// The `CGDirectDisplayID` of the target display (`0` = primary display).
        display_id: u32,
    },
    /// Capture a single application window, regardless of occlusion.
    Window {
        /// The `SCWindow.windowID` value returned by `SCShareableContent`.
        window_id: u32,
    },
    /// Capture a rectangular area within a display.
    Area {
        /// The capture rectangle in display points.
        rect: Rect,
    },
}

impl Default for CaptureRegion {
    fn default() -> Self {
        Self::FullScreen { display_id: 0 }
    }
}

// ---------------------------------------------------------------------------
// RecordingSettings
// ---------------------------------------------------------------------------

/// All user-configurable parameters for a recording session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingSettings {
    /// Output video resolution preset.
    #[serde(default)]
    pub resolution: Resolution,

    /// Target capture frame rate in frames per second.
    ///
    /// Must be one of `24`, `30`, or `60`.
    pub frame_rate: u32,

    /// Which part of the screen to record.
    #[serde(default)]
    pub region: CaptureRegion,

    /// Whether to capture microphone audio alongside system audio.
    pub capture_mic: bool,

    /// Folder where finalized MP4 files are saved.
    pub output_dir: PathBuf,

    /// Video encoding quality preset.
    #[serde(default)]
    pub quality: VideoQuality,
}

impl Default for RecordingSettings {
    fn default() -> Self {
        Self {
            resolution: Resolution::default(),
            frame_rate: 30,
            region: CaptureRegion::default(),
            capture_mic: true,
            output_dir: default_output_dir(),
            quality: VideoQuality::default(),
        }
    }
}

// ---------------------------------------------------------------------------
// Persistence helpers
// ---------------------------------------------------------------------------

fn default_output_dir() -> PathBuf {
    dirs::desktop_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

/// Returns the canonical path for the settings JSON file.
///
/// On macOS this resolves to
/// `~/Library/Application Support/screen-recorder/settings.json`.
#[must_use]
pub fn settings_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join("Library/Application Support")
        })
        .join("screen-recorder")
        .join("settings.json")
}

/// Serializes `settings` to JSON and writes to `path`, creating parent
/// directories as needed.
///
/// # Errors
///
/// Returns [`AppError::Io`] if serialization or any file-system operation
/// fails.
pub fn write_settings_to(path: &Path, settings: &RecordingSettings) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(settings)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, json)?;
    Ok(())
}

/// Reads and deserializes settings from `path`.
///
/// Returns `None` if the file does not exist or cannot be deserialized.
#[must_use]
pub fn read_settings_from(path: &Path) -> Option<RecordingSettings> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
}

/// Loads settings from the canonical settings path, returning
/// [`RecordingSettings::default`] on any error.
#[must_use]
pub fn load_settings() -> RecordingSettings {
    read_settings_from(&settings_path()).unwrap_or_default()
}

/// Persists `settings` to the canonical settings path.
///
/// # Errors
///
/// Returns [`AppError::Io`] if the file cannot be written.
pub fn save_settings(settings: &RecordingSettings) -> Result<(), AppError> {
    write_settings_to(&settings_path(), settings)
}

// ---------------------------------------------------------------------------
// Tests (T015)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_frame_rate_is_valid() {
        let s = RecordingSettings::default();
        assert!(
            [24_u32, 30, 60].contains(&s.frame_rate),
            "default frame_rate {} must be one of 24/30/60",
            s.frame_rate,
        );
    }

    #[test]
    fn all_resolutions_serialize_round_trip() {
        for r in [Resolution::Native, Resolution::P1080, Resolution::P720] {
            let json = serde_json::to_string(&r).expect("serialize");
            let back: Resolution = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(r, back);
        }
    }

    #[test]
    fn all_qualities_serialize_round_trip() {
        for q in [VideoQuality::High, VideoQuality::Medium, VideoQuality::Low] {
            let json = serde_json::to_string(&q).expect("serialize");
            let back: VideoQuality = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(q, back);
        }
    }

    #[test]
    fn bitrate_mapping_is_correct() {
        assert_eq!(VideoQuality::High.bitrate_bps(), 8_000_000);
        assert_eq!(VideoQuality::Medium.bitrate_bps(), 4_000_000);
        assert_eq!(VideoQuality::Low.bitrate_bps(), 2_000_000);
    }

    #[test]
    fn capture_region_fullscreen_round_trip() {
        let r = CaptureRegion::FullScreen { display_id: 1 };
        let json = serde_json::to_string(&r).expect("serialize");
        let back: CaptureRegion = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(r, back);
    }

    #[test]
    fn capture_region_window_round_trip() {
        let r = CaptureRegion::Window { window_id: 42 };
        let json = serde_json::to_string(&r).expect("serialize");
        let back: CaptureRegion = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(r, back);
    }

    #[test]
    fn capture_region_area_round_trip() {
        let r = CaptureRegion::Area {
            rect: Rect {
                x: 10.0,
                y: 20.0,
                width: 800.0,
                height: 600.0,
            },
        };
        let json = serde_json::to_string(&r).expect("serialize");
        let back: CaptureRegion = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(r, back);
    }

    #[test]
    fn recording_settings_default_round_trip() {
        let original = RecordingSettings::default();
        let json = serde_json::to_string_pretty(&original).expect("serialize");
        let restored: RecordingSettings = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(original.resolution, restored.resolution);
        assert_eq!(original.frame_rate, restored.frame_rate);
        assert_eq!(original.region, restored.region);
        assert_eq!(original.capture_mic, restored.capture_mic);
        assert_eq!(original.output_dir, restored.output_dir);
        assert_eq!(original.quality, restored.quality);
    }
}
