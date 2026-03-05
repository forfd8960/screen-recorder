use std::path::PathBuf;

use screen_recorder::output::save::{finalize, finalize_via_copy_fallback, generate_filename};

#[test]
fn finalize_moves_file_and_removes_source() {
    let src_dir = tempfile::tempdir().expect("create src temp dir");
    let dst_dir = tempfile::tempdir().expect("create dst temp dir");

    let src = src_dir.path().join("temp-recording.mp4");
    std::fs::write(&src, b"fake mp4 bytes").expect("write source file");

    let name = generate_filename();
    let out = finalize(&src, dst_dir.path(), &name).expect("finalize should succeed");

    assert_eq!(out, PathBuf::from(dst_dir.path()).join(name));
    assert!(out.exists(), "destination file should exist");
    assert!(
        !src.exists(),
        "source file should be removed after finalize"
    );
}

#[test]
fn finalize_copy_fallback_path_removes_source() {
    let src_dir = tempfile::tempdir().expect("create src temp dir");
    let dst_dir = tempfile::tempdir().expect("create dst temp dir");

    let src = src_dir.path().join("temp-recording.mp4");
    std::fs::write(&src, b"fake mp4 bytes for fallback").expect("write source file");

    let name = generate_filename();

    let out = finalize_via_copy_fallback(&src, dst_dir.path(), &name)
        .expect("finalize fallback should succeed");

    assert!(
        out.exists(),
        "destination file should exist on fallback path"
    );
    assert!(
        !src.exists(),
        "source file should be removed on fallback path"
    );
}
