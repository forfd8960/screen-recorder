//! RAII wrapper for the temporary output file used during recording.
//!
//! [`TempFile`] creates a uniquely-named `*.mp4` file under
//! `$TMPDIR/screen-recorder/` when constructed and **deletes it on drop
//! unless [`TempFile::keep`] has been called**.
//!
//! # Example
//!
//! ```no_run
//! use screen_recorder::encode::temp_file::TempFile;
//!
//! let mut tmp = TempFile::new().unwrap();
//! // … write the encoded bytes …
//! tmp.keep(); // prevent deletion on drop
//! let final_path = tmp.path().to_path_buf();
//! ```

use std::path::{Path, PathBuf};

use uuid::Uuid;

use crate::error::AppError;

// ---------------------------------------------------------------------------
// TempFile
// ---------------------------------------------------------------------------

/// A temporary MP4 file that is automatically removed when dropped unless
/// [`keep`](TempFile::keep) is called.
pub struct TempFile {
    /// Absolute path to the temporary file.
    path: PathBuf,
    /// When `true` the file is **not** removed on drop.
    keep: bool,
}

impl TempFile {
    /// Creates the `$TMPDIR/screen-recorder/` directory (if absent) and
    /// reserves a new `<uuid>.mp4` path inside it.
    ///
    /// The file itself is **not** created on disk at this point; the encoding
    /// pipeline will create it when `AVAssetWriter` begins writing.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Io`] if the directory cannot be created.
    pub fn new() -> Result<Self, AppError> {
        let dir = std::env::temp_dir().join("screen-recorder");
        std::fs::create_dir_all(&dir)?;

        let file_name = format!("{}.mp4", Uuid::new_v4());
        Ok(Self {
            path: dir.join(file_name),
            keep: false,
        })
    }

    /// Returns the reserved path for this temporary file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Marks the file to be **kept** after this `TempFile` is dropped.
    ///
    /// Call this once the encoding pipeline has finished writing and you want
    /// to hand ownership of the file to the caller.
    pub const fn keep(&mut self) {
        self.keep = true;
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        if !self.keep
            && self.path.exists()
            && let Err(e) = std::fs::remove_file(&self.path)
        {
            // Best-effort: log but do not panic on cleanup failure.
            tracing::warn!(
                path = %self.path.display(),
                error = %e,
                "failed to delete temporary recording file"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_returns_valid_path() {
        let tmp = TempFile::new().expect("TempFile::new should succeed");
        // Path is under the system temp dir.
        assert!(tmp.path().starts_with(std::env::temp_dir()));
        // Extension is .mp4
        assert_eq!(tmp.path().extension().and_then(|e| e.to_str()), Some("mp4"));
    }

    #[test]
    fn file_is_deleted_on_drop_when_it_exists() {
        let path = {
            let tmp = TempFile::new().expect("TempFile::new");
            // Create the file so there is something to delete.
            std::fs::File::create(tmp.path()).expect("create test file");
            assert!(tmp.path().exists());
            tmp.path().to_path_buf()
            // `tmp` drops here — should delete the file
        };
        assert!(!path.exists(), "file should have been deleted on drop");
    }

    #[test]
    fn keep_prevents_deletion() {
        let path = {
            let mut tmp = TempFile::new().expect("TempFile::new");
            std::fs::File::create(tmp.path()).expect("create test file");
            tmp.keep();
            tmp.path().to_path_buf()
            // `tmp` drops here — file should NOT be deleted
        };
        assert!(path.exists(), "file should be kept after keep()");
        // Clean up manually.
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn drop_on_nonexistent_file_is_noop() {
        // If the encoder never wrote anything, the file won't exist yet.
        // Drop must not panic.
        let _tmp = TempFile::new().expect("TempFile::new");
        // _tmp drops without the file ever being created — must be silent.
    }
}
