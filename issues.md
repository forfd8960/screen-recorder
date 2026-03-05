# Bug Fix History

## Issue 1 — Audio buffers routed to video channel causing `NSInvalidArgumentException`

**Commit:** `9366c90`
**File:** `src/capture/engine.rs`

### Symptom

```
*** Terminating app due to uncaught exception 'NSInvalidArgumentException',
reason: '*** -[AVAssetWriterInput appendSampleBuffer:] Media type of sample
buffer must match receiver's media type ("vide")'
```

### Root Cause

`SCKit` routes **all** output callbacks (Screen + Audio) to **every** registered
handler, regardless of the `SCStreamOutputType` passed to `add_output_handler`.
Because `ChannelHandler` had no type guard, audio `CMSampleBuffer`s were
forwarded into `video_tx` and subsequently passed to the video-only
`AVAssetWriterInput`, which rejected the type mismatch with a fatal
Objective-C exception.

### Fix

Added `expected_type: SCStreamOutputType` to `ChannelHandler`. The
`did_output_sample_buffer` callback now discards any buffer whose `of_type`
does not match `expected_type` before forwarding to the channel:

```rust
if is_audio != want_audio {
    return; // discard — wrong type for this handler
}
```

The video handler is created with `expected_type: SCStreamOutputType::Screen`;
the audio handler with `expected_type: SCStreamOutputType::Audio`.

---

## Issue 2 — `AVAssetWriter.finishWriting()` returns `false` on Stop

**Commit:** `465368e`
**Files:** `Cargo.toml`, `src/encode/pipeline.rs`

### Symptom

```
ERROR screen_recorder::app: failed to stop recording:
Encoding pipeline error: AVAssetWriter.finishWriting() returned false
```

### Root Cause

Two compounding problems:

1. The return value of `appendSampleBuffer` was never checked. If
   AVFoundation rejected a buffer for any reason, the writer silently
   entered `AVAssetWriterStatusFailed`; the subsequent `finishWriting()`
   call then also returned `false`.
2. The deprecated synchronous `finishWriting()` is unreliable on modern
   macOS — Apple's own documentation recommends `finishWritingWithCompletionHandler:`
   for all current code.

### Fix

- Added `block2 = "0.6"` as a direct dependency and `"NSError"` to
  `objc2-foundation` features.
- `appendSampleBuffer` return value is now checked; `false` triggers a
  `warn!` and breaks out of the write loop immediately.
- Replaced the deprecated `finishWriting()` with
  `finishWritingWithCompletionHandler:` using an
  `Arc<(Mutex<bool>, Condvar)>` to block the `spawn_blocking` thread
  until the completion handler fires.
- After the handler fires, `writer.status()` is compared against
  `AVAssetWriterStatus::Completed`; on failure `writer.error()` surfaces
  the full `NSError` description.

---

## Issue 3 — `AVErrorUnknown` (-11800) on `finishWriting` after Stop

**Commit:** `91aa77a`
**File:** `src/encode/pipeline.rs`

### Symptom

```
ERROR screen_recorder::app: failed to stop recording:
Encoding pipeline error: AVAssetWriter finishWriting failed:
NSError { code: -11800, localizedDescription: "The operation could not
be completed", domain: "AVFoundationErrorDomain", ... }
```

Log also showed heavy frame drops on both video and audio channels before
Stop was clicked.

### Root Cause

`AVAssetWriter.startSessionAtSourceTime` was called with a hardcoded
`CMTime { value: 0, timescale: 1 }` (i.e. 0 seconds). `SCKit` delivers
`CMSampleBuffer`s with wall-clock presentation timestamps (≈ system uptime,
often tens of thousands of seconds). AVFoundation treated every incoming
buffer as arriving ≈ 86 400 s *after* the session start, rejected each
`appendSampleBuffer` call silently, and marked the writer as
`AVAssetWriterStatusFailed` — causing `finishWritingWithCompletionHandler:`
to complete with the `-11800` error.

`PtsNormalizer` was previously computing the correct recording-relative
offset, but the result was stored in `_pts_secs` (prefixed with `_`,
effectively discarded) and never applied to the session anchor.

### Fix

- Removed the pre-loop `startSessionAtSourceTime(time_zero)` call.
- On the **first valid video frame**, the session is now anchored to
  that frame's actual `CMTime`:

  ```rust
  if !session_started {
      let first_pts = retained.presentation_time_stamp();
      writer.startSessionAtSourceTime(first_pts);
      session_started = true;
  }
  ```

- Removed dead code: `_pts_secs` computation, `PtsNormalizer` usage in
  the pipeline, unused `CMTimeFlags` and `CMTime` imports.

---

## Issue 4 — Audio pipeline crashes on `appendSampleBuffer` (`kAudioFormatMPEG4AAC` + format mismatch)

**Commits:** `7818d90` (partial fix), then audio pipeline removed
**File:** `src/encode/pipeline.rs`

### Symptom

```
fatal runtime error: Rust cannot catch foreign exceptions, aborting
```

Crash occurred immediately after the write loop entered and the first
audio buffer was appended.

### Root Cause (two layers)

1. **Wrong FourCC constant** — `K_AUDIO_FORMAT_MPEG4_AAC` was set to
   `0x6D70_3461` (`'mp4a'`), which is the MP4 *container* codec tag.
   The correct CoreAudio `AudioFormat.h` identifier is `0x6161_6320`
   (`'aac '`). The wrong constant caused AVFoundation to throw
   `NSInvalidArgumentException` on `AVAssetWriterInput` creation.

2. **SCKit audio format incompatibility** — even with the corrected
   constant, SCKit delivers audio as **LPCM Float32 Non-Interleaved**
   (48 kHz, 2 ch). `AVAssetWriterInput` with AAC `outputSettings`
   requires a compatible interleaved LPCM input format; passing SCKit's
   `CMSampleBuffer` directly raises `NSInvalidArgumentException` on
   `appendSampleBuffer`. Using `outputSettings = nil` (passthrough) also
   fails because the MP4 container rejects raw LPCM audio tracks.

### Fix

Removed the audio `AVAssetWriterInput` from the writer entirely.
Audio buffers received from the capture engine are drained and discarded
in the write loop. Recording is **video-only** until Phase 8, which will
implement proper LPCM → AAC conversion via `AudioConverter` before
passing buffers to `AVAssetWriterInput`.

```rust
// Phase 8: convert LPCM Float32 NI → interleaved Int16 via AudioConverter,
// then appendSampleBuffer to an audio AVAssetWriterInput.
while audio_rx.try_recv().is_ok() { /* discard */ }
```


---

## Issue 5 -- TCC Screen Recording Permission Lost After Every Recompile

**Phase:** Phase 4 (T030-T036)
**Files:** `Makefile`

### Symptom

After granting Screen Recording permission in System Settings -> Privacy & Security,
the app still displayed the "Screen Recording permission denied" banner on the next
launch. Re-enabling the toggle in System Settings had no lasting effect -- each new
`cargo build` resulted in another permission denial.

### Root Cause

macOS TCC (Transparency, Consent, and Control) tracks **unsigned binaries** by
their SHA-256 content hash rather than by a stable bundle identifier. Every
`cargo build` produces a new binary with a different hash, which TCC treats as
a completely different, untrusted application. As a result the previously granted
permission no longer applied to the newly compiled binary.

Confirmed with:

    $ codesign -dv target/release/screen-recorder
    screen-recorder: code object is not signed at all

### Fix

Ad-hoc code-sign the binary with a stable bundle identifier after every build
so that TCC tracks the app by identifier instead of hash:

    codesign --sign - --identifier "com.forfd8960.screen-recorder" --force target/release/screen-recorder

Updated `Makefile` to run `codesign` automatically after `cargo build` and
`cargo build --release`. New targets added: `sign-debug`, `sign-release`, `reset-tcc`.

**Recovery steps (one-time after applying the fix):**
1. Run `tccutil reset ScreenCapture` to clear all stale TCC entries.
2. Run `make build-release` -- binary is now signed on every build.
3. Launch the app and grant Screen Recording once in System Settings.
4. Permission now persists across all subsequent recompiles.


---

## Issue 6 — `AVErrorUnknown` (-11800) caused by `startSessionAtSourceTime` gated behind `isReadyForMoreMediaData`

**Commit:** `8669041`
**Files:** `src/capture/engine.rs`, `src/encode/pipeline.rs`

### Symptom

```
WARN  frame dropped — video channel full
WARN  frame dropped — audio channel full stream="audio"
ERROR failed to stop recording: Encoding pipeline error: AVAssetWriter
      finishWriting failed: NSError { code: -11800,
      localizedDescription: "The operation could not be completed",
      domain: "AVFoundationErrorDomain", ... }
```

Heavy frame drops on both channels immediately before Stop, followed by
the same `-11800` error on `finishWritingWithCompletionHandler`.

### Root Cause

Two compounding problems:

1. **Session never started** — `startSessionAtSourceTime` was placed inside
   the `if video_input.isReadyForMoreMediaData() && let Some(retained) = ...`
   double-guard. `AVAssetWriterInput` is typically *not* ready for the first
   few frames while the encoder warms up. When those frames arrived before
   the input became ready, the entire branch was skipped — including the
   `session_started = true` assignment. The write loop eventually exited
   with `session_started == false`, and calling
   `finishWritingWithCompletionHandler` on a writer that never had an active
   session produces `AVErrorUnknown (-11800)`.

2. **Channel capacity too small** — `CHANNEL_CAPACITY` was 120 (~2 s at
   60 fps). SCKit starts delivering frames immediately after
   `start_capture()`, while the encoding thread is still constructing
   `AVAssetWriter` / `AVAssetWriterInput` / calling `startWriting()`. The
   120-frame ring fills during this initialisation window, producing the
   "frame dropped" flood.

### Fix

**`src/encode/pipeline.rs`** — decouple session start from readiness check:

```rust
// Always anchor the session on the first valid retained frame,
// independent of isReadyForMoreMediaData.
if !session_started {
    let first_pts = retained.presentation_time_stamp();
    writer.startSessionAtSourceTime(first_pts);
    session_started = true;
}

// Only the append is gated on readiness.
if video_input.isReadyForMoreMediaData() {
    frame_count += 1;
    if !video_input.appendSampleBuffer(&retained) { ... }
}
```

Also added a finalize guard: if `session_started == false` when the loop
exits (zero valid frames received), return a clear error immediately
instead of calling `finishWritingWithCompletionHandler` on an unstarted
writer.

**`src/capture/engine.rs`** — increased channel capacity:

```rust
// Before: const CHANNEL_CAPACITY: usize = 120;   // ~2 s @ 60 fps
const CHANNEL_CAPACITY: usize = 480;               // ~4 s @ 60 fps
```

---

## Issue 7 — `finishWritingWithCompletionHandler` called on already-Failed writer after Stop

**File:** `src/encode/pipeline.rs`

### Symptom

```
WARN  frame dropped — video channel full
WARN  frame dropped — audio channel full stream="audio"
INFO  SCStream stopped
ERROR failed to stop recording: Encoding pipeline error: AVAssetWriter
    finishWriting failed: NSError { code: -11800,
    localizedDescription: "The operation could not be completed",
    domain: "AVFoundationErrorDomain", ... }
```

The error was reported when Stop was clicked, but the writer had already
entered `Failed` state during recording.

### Root Cause

When `appendSampleBuffer` / `appendPixelBuffer` returned `false`, the loop
exited, but finalize still called `markAsFinished()` +
`finishWritingWithCompletionHandler:` unconditionally. Calling finish on a
writer already in `AVAssetWriterStatusFailed` produces `AVErrorUnknown`
(`-11800`).

### Fix

Added `append_failed: bool` and failure-aware finalize logic:

```rust
if append_failed || writer.status() == AVAssetWriterStatus::Failed {
  let err_desc = writer
    .error()
    .map(|e| format!("{e:?}"))
    .unwrap_or_else(|| format!("status={}", writer.status().0));
  writer.cancelWriting();
  return Err(AppError::EncodingError(format!(
    "AVAssetWriter entered failed state during recording: {err_desc}"
  )));
}
```

Also, the `!session_started` guard now calls `cancelWriting()` before
returning so partial files/resources are cleaned up in all early-fail paths.

---

## Issue 8 — `AVErrorUnknown` (`-11800`) caused by SCKit `CMSampleBuffer` metadata extensions

**Files:** `Cargo.toml`, `src/encode/pipeline.rs`

### Symptom

```
INFO  received recorder command cmd=Stop
INFO  SCStream stopped
ERROR failed to stop recording: Encoding pipeline error: AVAssetWriter
    entered failed state during recording: NSError { code: -11800,
    localizedDescription: "The operation could not be completed",
    domain: "AVFoundationErrorDomain", ... }
```

After Issue 7, finalize behavior was correct, but the writer still failed on
the first frame append.

### Root Cause

`ScreenCaptureKit` `CMSampleBuffer` carries proprietary frame attachments
(dirty rects, display time, content scale, content rect, etc.). Passing the
full sample buffer directly to `AVAssetWriterInput.appendSampleBuffer` can
cause VideoToolbox/H.264 to reject the frame and flip writer status to
`Failed` with `-11800`.

### Fix

Switched from sample-buffer append to pixel-buffer append using
`AVAssetWriterInputPixelBufferAdaptor`, which bypasses SCKit-specific sample
attachments:

1. Added `objc2-core-video` support in dependencies.
2. Created adaptor from `video_input`.
3. Extracted `CVPixelBuffer` + PTS from SCKit buffer and appended via:

```rust
adaptor.appendPixelBuffer_withPresentationTime(&retained_pix, pts)
```

4. Added a toll-free bridge helper from
`screencapturekit::cv::CVPixelBuffer` to `objc2_core_video::CVPixelBuffer`.

This keeps timing intact while avoiding metadata incompatibility that was
triggering `AVAssetWriterStatusFailed`.
