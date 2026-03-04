//! Presentation-timestamp normalisation for the encoding pipeline.
//!
//! Both video and audio streams carry wall-clock timestamps that must be
//! converted to recording-relative times (starting at 0.0 s) before being
//! passed to `AVAssetWriter`.

// ---------------------------------------------------------------------------
// PtsNormalizer (T021)
// ---------------------------------------------------------------------------

/// Normalises sample presentation timestamps to recording-relative seconds.
///
/// The first call to [`normalize_secs`](Self::normalize_secs) establishes the
/// *base time*; every subsequent call returns `(pts_secs − base_secs).max(0.0)`.
///
/// The `max(0.0)` clamp handles the edge case where an audio sample arrives
/// slightly before the first video frame (common at stream start).
#[derive(Debug, Default)]
pub struct PtsNormalizer {
    base_secs: Option<f64>,
}

impl PtsNormalizer {
    /// Creates a new normaliser with no base time set.
    #[must_use]
    pub const fn new() -> Self {
        Self { base_secs: None }
    }

    /// Returns the recording-relative presentation time for `pts_secs`.
    ///
    /// The first invocation anchors the base time at `pts_secs`; all
    /// subsequent calls return values relative to that anchor.  The result
    /// is clamped to `0.0` to handle timestamps that arrive slightly before
    /// the anchor (e.g. audio-before-video start race).
    pub fn normalize_secs(&mut self, pts_secs: f64) -> f64 {
        let base = *self.base_secs.get_or_insert(pts_secs);
        (pts_secs - base).max(0.0)
    }

    /// Resets the base time so the next call establishes a new anchor.
    pub const fn reset(&mut self) {
        self.base_secs = None;
    }
}

// ---------------------------------------------------------------------------
// Unit tests (T018)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::PtsNormalizer;

    #[test]
    fn first_call_returns_zero() {
        let mut n = PtsNormalizer::new();
        assert!(n.normalize_secs(100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn subsequent_calls_are_monotonically_increasing() {
        let mut n = PtsNormalizer::new();
        let t0 = n.normalize_secs(100.0);
        let t1 = n.normalize_secs(100.5);
        let t2 = n.normalize_secs(101.0);
        assert!(t0 < t1, "t0={t0} should be < t1={t1}");
        assert!(t1 < t2, "t1={t1} should be < t2={t2}");
    }

    #[test]
    fn handles_static_screen_frame_gap() {
        let mut n = PtsNormalizer::new();
        let _ = n.normalize_secs(10.0); // base = 10.0
        let _ = n.normalize_secs(11.0); // 1.0
        // 2-second gap simulating a static screen (no new frames generated)
        let after_gap = n.normalize_secs(13.0);
        assert!(
            (after_gap - 3.0).abs() < 1e-9,
            "gap should preserve absolute time delta: got {after_gap}"
        );
    }

    #[test]
    fn audio_arrives_before_video_is_clamped_to_zero() {
        // Simulate audio PTS slightly earlier than video PTS.
        // When both share the same base-time anchor, the "earlier" one
        // result in a slightly negative diff that gets clamped to 0.
        let mut audio_n = PtsNormalizer::new();
        // Audio sets base = 10.0
        let a0 = audio_n.normalize_secs(10.0);
        // Video arrives at 10.033 → relative = 0.033
        let mut video_n = PtsNormalizer::new();
        let v0 = video_n.normalize_secs(10.0);
        assert!(a0.abs() < f64::EPSILON);
        assert!(v0.abs() < f64::EPSILON);
        // Verify clamping: a timestamp slightly below base gives 0, not negative
        let below_base = audio_n.normalize_secs(9.999_999);
        assert!(
            below_base.abs() < f64::EPSILON,
            "below-base should be clamped to 0.0"
        );
    }
}
