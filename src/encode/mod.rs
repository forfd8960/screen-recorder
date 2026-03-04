//! Encoding and muxing layer.
//!
//! Sub-modules:
//! - [`pipeline`]   – `AVAssetWriter` + `AVAssetWriterInput` lifecycle
//! - [`sync`]       – `PtsNormalizer`: A/V timestamp base-time normalization
//! - [`temp_file`]  – RAII wrapper for the temporary `$TMPDIR/screen-recorder/<uuid>.mp4` path

// Implemented in Phase 3 (M2 / M3).
