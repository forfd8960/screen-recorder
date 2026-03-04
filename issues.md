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
