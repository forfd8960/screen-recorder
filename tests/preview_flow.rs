//! Integration tests for the preview → Discard / Accept flow.
//!
//! Tests exercise `TempFile` keep-semantics and the `handle_discard` helper
//! to verify the full Previewing → Idle / Saving state transition path.
//!
//! Gated by the `integration` feature so they are skipped in normal CI runs
//! (`cargo test`); enable with `cargo test --features integration`.
//!
//! These tests write real files to `std::env::temp_dir()` and rely on the
//! tokio synchronous channel primitives — no async runtime required.

use screen_recorder::{
    app::RecorderCommand, encode::temp_file::TempFile, ui::preview_panel::handle_discard,
};

// ---------------------------------------------------------------------------
// TempFile keep semantics
// ---------------------------------------------------------------------------

/// After `TempFile::keep()`, dropping the wrapper must NOT delete the file.
#[cfg_attr(
    not(feature = "integration"),
    ignore = "requires --features integration"
)]
#[test]
fn temp_file_survives_drop_after_keep() {
    let mut tmp = TempFile::new().expect("TempFile::new failed");
    std::fs::write(tmp.path(), b"fake mp4 data").expect("write failed");
    let path = tmp.path().to_path_buf();

    // Mark the file to be kept.
    tmp.keep();
    drop(tmp);

    assert!(
        path.exists(),
        "TempFile with keep() must not be deleted on drop"
    );
    std::fs::remove_file(&path).ok();
}

/// Without `keep()`, dropping `TempFile` deletes the underlying file.
#[cfg_attr(
    not(feature = "integration"),
    ignore = "requires --features integration"
)]
#[test]
fn temp_file_deleted_on_drop_without_keep() {
    let tmp = TempFile::new().expect("TempFile::new failed");
    std::fs::write(tmp.path(), b"fake mp4 data").expect("write failed");
    let path = tmp.path().to_path_buf();

    drop(tmp); // keep not called — must delete.

    assert!(
        !path.exists(),
        "TempFile without keep() must be deleted on drop"
    );
}

// ---------------------------------------------------------------------------
// Discard flow
// ---------------------------------------------------------------------------

/// Full preview → Discard path:
/// `TempFile` → write data → `keep()` → `handle_discard` → file gone +
/// `RecorderCommand::Discard` received.
#[cfg_attr(
    not(feature = "integration"),
    ignore = "requires --features integration"
)]
#[test]
fn discard_deletes_kept_temp_file_and_sends_command() {
    // 1. Simulate encoding pipeline finalizing a temp file.
    let mut tmp = TempFile::new().expect("TempFile::new failed");
    std::fs::write(tmp.path(), b"fake mp4 data").expect("write failed");
    let path = tmp.path().to_path_buf();
    tmp.keep(); // Pipeline calls keep() when Previewing state is entered.
    drop(tmp);

    assert!(
        path.exists(),
        "pre-condition: file must exist before discard"
    );

    // 2. Simulate user clicking Discard in the preview panel.
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<RecorderCommand>();
    handle_discard(Some(&path), &tx);

    // 3. File must be gone.
    assert!(!path.exists(), "handle_discard must delete the temp file");

    // 4. Command must propagate.
    let cmd = rx.try_recv().expect("Discard command not received");
    assert!(
        matches!(cmd, RecorderCommand::Discard),
        "expected RecorderCommand::Discard, got {cmd:?}"
    );
}
