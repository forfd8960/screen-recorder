//! Integration tests: `RecordingSettings` JSON round-trip (T014).
//!
//! These tests exercise the full serialise → deserialise cycle of every
//! settings type using `serde_json`.  No disk I/O is required for the
//! serialisation tests; `write_settings_to` / `read_settings_from` are
//! tested separately with a `tempfile`-backed path.

use std::path::PathBuf;

use screen_recorder::config::settings::{
    CaptureRegion, RecordingSettings, Rect, Resolution, VideoQuality, read_settings_from,
    write_settings_to,
};

// ---------------------------------------------------------------------------
// Serialisation correctness
// ---------------------------------------------------------------------------

#[test]
fn default_settings_json_round_trip() {
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

#[test]
fn all_resolution_variants_round_trip() {
    for r in [Resolution::Native, Resolution::P1080, Resolution::P720] {
        let json = serde_json::to_string(&r).expect("serialize");
        let back: Resolution = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(r, back, "Resolution::{r:?} round-trip failed");
    }
}

#[test]
fn all_quality_variants_round_trip() {
    for q in [VideoQuality::High, VideoQuality::Medium, VideoQuality::Low] {
        let json = serde_json::to_string(&q).expect("serialize");
        let back: VideoQuality = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(q, back, "VideoQuality::{q:?} round-trip failed");
    }
}

#[test]
fn recording_settings_round_trip_all_resolution_and_quality_combinations() {
    for resolution in [Resolution::Native, Resolution::P1080, Resolution::P720] {
        for quality in [VideoQuality::Low, VideoQuality::Medium, VideoQuality::High] {
            let original = RecordingSettings {
                resolution,
                frame_rate: 30,
                region: CaptureRegion::FullScreen { display_id: 1 },
                capture_mic: true,
                output_dir: PathBuf::from("/tmp/screen-recorder-tests"),
                quality,
            };

            let json = serde_json::to_string(&original).expect("serialize");
            let restored: RecordingSettings = serde_json::from_str(&json).expect("deserialize");

            assert_eq!(restored.resolution, resolution);
            assert_eq!(restored.quality, quality);
            assert_eq!(restored.frame_rate, 30);
        }
    }
}

#[test]
fn capture_region_all_variants_round_trip() {
    let variants: Vec<CaptureRegion> = vec![
        CaptureRegion::FullScreen { display_id: 0 },
        CaptureRegion::FullScreen { display_id: 2 },
        CaptureRegion::Window { window_id: 123 },
        CaptureRegion::Area {
            rect: Rect {
                x: 0.0,
                y: 100.0,
                width: 1440.0,
                height: 900.0,
            },
        },
    ];

    for variant in &variants {
        let json = serde_json::to_string(variant).expect("serialize");
        let back: CaptureRegion = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(variant, &back, "CaptureRegion variant round-trip failed");
    }
}

#[test]
fn video_quality_bitrate_mapping() {
    assert_eq!(VideoQuality::High.bitrate_bps(), 8_000_000);
    assert_eq!(VideoQuality::Medium.bitrate_bps(), 4_000_000);
    assert_eq!(VideoQuality::Low.bitrate_bps(), 2_000_000);
}

// ---------------------------------------------------------------------------
// File persistence
// ---------------------------------------------------------------------------

#[test]
fn settings_persist_and_reload_from_temp_file() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join("settings.json");

    let original = RecordingSettings {
        resolution: Resolution::P720,
        frame_rate: 60,
        region: CaptureRegion::Window { window_id: 99 },
        capture_mic: false,
        output_dir: PathBuf::from("/tmp/recordings"),
        quality: VideoQuality::Low,
    };

    write_settings_to(&path, &original).expect("write settings");

    let restored = read_settings_from(&path).expect("read settings");
    assert_eq!(original.resolution, restored.resolution);
    assert_eq!(original.frame_rate, restored.frame_rate);
    assert_eq!(original.region, restored.region);
    assert_eq!(original.capture_mic, restored.capture_mic);
    assert_eq!(original.output_dir, restored.output_dir);
    assert_eq!(original.quality, restored.quality);
}

#[test]
fn read_settings_from_nonexistent_path_returns_none() {
    let result = read_settings_from(&PathBuf::from("/no/such/file.json"));
    assert!(result.is_none());
}
