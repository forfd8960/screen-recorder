//! T031 — Audio pipeline integration test.
//!
//! This test is gated behind the `integration` Cargo feature so that it is
//! **ignored** in normal `cargo test` runs (which have no screen-recording
//! permission and no macOS audio runtime available in CI).
//!
//! To run the full suite:
//!
//! ```text
//! cargo test --features integration
//! ```

/// Smoke test: constructing an `EncodingPipeline` must not panic, even when
/// the audio receiver is immediately disconnected (empty channel).
///
/// Verifying that a mock `CMSampleBuffer` produces a non-zero audio track
/// duration in the finalized MP4 requires the full macOS AVFoundation runtime
/// and screen-recording permission.  That level of coverage is deferred to
/// manual / on-device testing; this test guards against regressions in the
/// pipeline constructor path.
#[cfg_attr(not(feature = "integration"), ignore)]
#[test]
fn encoding_pipeline_new_does_not_panic() {
    use screen_recorder::{
        config::settings::RecordingSettings, encode::pipeline::EncodingPipeline,
    };
    use tokio::sync::mpsc;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");

    rt.block_on(async {
        let settings = RecordingSettings::default();
        let (_video_tx, video_rx) = mpsc::channel(8);
        let (_audio_tx, audio_rx) = mpsc::channel(8);

        let pipeline = EncodingPipeline::new(&settings, video_rx, audio_rx, 1280, 720);
        assert!(
            pipeline.is_ok(),
            "EncodingPipeline::new must succeed: {:?}",
            pipeline.err()
        );
    });
}
