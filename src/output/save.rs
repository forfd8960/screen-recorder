//! Save/finalize helpers for moving the temporary recording into a user folder.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use time::{OffsetDateTime, UtcOffset, format_description::FormatItem, macros::format_description};

use crate::error::AppError;

const TIMESTAMP_FORMAT: &[FormatItem<'static>] =
    format_description!("[year]-[month]-[day]-[hour]-[minute]-[second]");

/// Generates a timestamped output filename.
///
/// Format: `screen-recording-YYYY-MM-DD-HH-MM-SS.mp4`.
#[must_use]
pub fn generate_filename() -> String {
    let now = OffsetDateTime::from(SystemTime::now());
    let local_now = UtcOffset::current_local_offset().map_or(now, |offset| now.to_offset(offset));

    let ts = local_now
        .format(TIMESTAMP_FORMAT)
        .unwrap_or_else(|_| "1970-01-01-00-00-00".to_string());

    format!("screen-recording-{ts}.mp4")
}

/// Moves `src` into `dst_dir/name`, with copy+delete fallback for cross-volume moves.
///
/// Returns the final destination path on success.
///
/// # Errors
///
/// Returns [`AppError::Io`] on file-system failures.
pub fn finalize(src: &Path, dst_dir: &Path, name: &str) -> Result<PathBuf, AppError> {
    std::fs::create_dir_all(dst_dir)?;
    let dst = dst_dir.join(name);

    match std::fs::rename(src, &dst) {
        Ok(()) => Ok(dst),
        Err(rename_err) => {
            // Cross-volume move (EXDEV) or platform-specific rename failure.
            // Fallback to copy + remove source.
            if let Err(copy_err) = std::fs::copy(src, &dst) {
                return Err(AppError::Io {
                    source: std::io::Error::other(format!(
                        "rename failed: {rename_err}; copy fallback failed: {copy_err}"
                    )),
                });
            }
            std::fs::remove_file(src)?;
            Ok(dst)
        }
    }
}

/// Executes the copy+remove fallback path directly.
///
/// Useful for deterministic tests of the cross-volume fallback behavior.
///
/// # Errors
///
/// Returns [`AppError::Io`] on file-system failures.
pub fn finalize_via_copy_fallback(
    src: &Path,
    dst_dir: &Path,
    name: &str,
) -> Result<PathBuf, AppError> {
    std::fs::create_dir_all(dst_dir)?;
    let dst = dst_dir.join(name);
    std::fs::copy(src, &dst)?;
    std::fs::remove_file(src)?;
    Ok(dst)
}

/// Reveals `path` in Finder.
///
/// # Errors
///
/// Returns [`AppError::Io`] if `open` cannot be spawned or exits non-zero.
pub fn reveal_in_finder(path: &Path) -> Result<(), AppError> {
    let status = std::process::Command::new("open")
        .arg("-R")
        .arg(path)
        .status()?;

    if status.success() {
        Ok(())
    } else {
        Err(AppError::Io {
            source: std::io::Error::other(format!("open -R exited with status {status}")),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_filename_matches_expected_shape() {
        let name = generate_filename();
        assert!(
            name.starts_with("screen-recording-"),
            "must start with expected prefix: {name}"
        );
        assert!(
            std::path::Path::new(&name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("mp4")),
            "must end with .mp4: {name}"
        );

        let ts = name
            .strip_prefix("screen-recording-")
            .and_then(|s| s.strip_suffix(".mp4"))
            .expect("prefix/suffix must be present");

        let parts: Vec<&str> = ts.split('-').collect();
        assert_eq!(
            parts.len(),
            6,
            "timestamp must contain 6 numeric parts: {ts}"
        );
        assert_eq!(parts[0].len(), 4, "year must be 4 digits");
        for part in &parts[1..] {
            assert_eq!(
                part.len(),
                2,
                "month/day/hour/minute/second must be 2 digits"
            );
            assert!(
                part.chars().all(|c| c.is_ascii_digit()),
                "part must be numeric: {part}"
            );
        }
    }

    #[test]
    fn generate_filename_differs_across_calls() {
        let first = generate_filename();
        std::thread::sleep(std::time::Duration::from_secs(1));
        let second = generate_filename();
        assert_ne!(
            first, second,
            "filenames across different seconds should differ"
        );
    }
}
