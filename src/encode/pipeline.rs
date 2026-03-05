//! `AVAssetWriter`-based encoding and muxing pipeline.
//!
//! [`EncodingPipeline`] spawns a dedicated blocking thread (via
//! `tokio::task::spawn_blocking`) that owns the `AVAssetWriter` and both
//! `AVAssetWriterInput` objects for the entire duration of a recording.
//! Because `AVAssetWriter` is `!Send + !Sync`, it must never leave the thread
//! on which it was created.
//!
//! # Lifecycle
//!
//! 1. [`EncodingPipeline::new`] creates a `TempFile`, launches the blocking
//!    thread, and returns immediately.
//! 2. The blocking thread drives a write loop that:
//!    - drains the audio channel (`try_recv`) on each iteration, and
//!    - blocks on the video channel (`blocking_recv`) waiting for the next frame.
//! 3. When the [`CaptureEngine`] calls `stop()`, it drops the video sender.
//!    `blocking_recv` returns `None` → the write loop exits.
//! 4. The blocking thread finishes writing, marks inputs as done, completes
//!    the asset writer, and sends the output path through an oneshot channel.
//! 5. The caller awaits [`EncodingPipeline::finish`] to retrieve the path.

#![allow(unsafe_code)]
#![allow(clippy::expect_used)] // static Option<&'static> keys always exist at runtime
#![allow(unsafe_op_in_unsafe_fn)] // extern static accesses are safe inside documented unsafe fns

use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_av_foundation::{
    AVAssetWriter, AVAssetWriterInput, AVAssetWriterStatus, AVFileTypeMPEG4, AVMediaTypeVideo,
    AVVideoAverageBitRateKey, AVVideoCodecKey, AVVideoCodecTypeH264,
    AVVideoCompressionPropertiesKey, AVVideoHeightKey, AVVideoWidthKey,
};
use objc2_core_media::CMSampleBuffer as ObjcCMSampleBuffer;
use objc2_foundation::{NSMutableDictionary, NSNumber, NSString, NSURL};
use tokio::sync::{mpsc, oneshot};
use tracing::{info, warn};

use crate::{config::settings::RecordingSettings, encode::temp_file::TempFile, error::AppError};

// ---------------------------------------------------------------------------
// EncodingPipeline
// ---------------------------------------------------------------------------

/// Manages the `AVAssetWriter` encoding pipeline on a dedicated blocking thread.
///
/// Use [`finish`](EncodingPipeline::finish) to await the final output path once
/// recording is complete.
pub struct EncodingPipeline {
    /// Receives the final `PathBuf` (or error) from the blocking thread.
    result_rx: oneshot::Receiver<Result<PathBuf, AppError>>,
    /// Handle to the underlying blocking task (used to detect panics on join).
    _task: tokio::task::JoinHandle<()>,
}

impl EncodingPipeline {
    /// Creates the pipeline and starts the encoding thread.
    ///
    /// # Parameters
    ///
    /// * `settings` – recording configuration (resolution, quality, audio).
    /// * `video_rx` – receives video [`screencapturekit::cm::CMSampleBuffer`]s
    ///   from `CaptureEngine`.
    /// * `audio_rx` – receives audio sample buffers from `CaptureEngine`.
    /// * `width` / `height` – actual capture dimensions returned by
    ///   [`CaptureEngine::start`].  Must be non-zero; passing zero would cause
    ///   `AVAssetWriterInput` to throw a fatal Objective-C exception.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Io`] if the temporary output file cannot be reserved.
    pub fn new(
        settings: &RecordingSettings,
        video_rx: mpsc::Receiver<screencapturekit::CMSampleBuffer>,
        audio_rx: mpsc::Receiver<screencapturekit::CMSampleBuffer>,
        width: u32,
        height: u32,
    ) -> Result<Self, AppError> {
        let mut temp = TempFile::new()?;
        let output_path = temp.path().to_path_buf();

        let bitrate = settings.quality.bitrate_bps();
        // `width` and `height` are provided by the caller (from CaptureEngine::start)
        // and are guaranteed to be the actual SCStream dimensions — never zero.
        let (result_tx, result_rx) = oneshot::channel::<Result<PathBuf, AppError>>();

        let task = tokio::task::spawn_blocking(move || {
            // `temp` is moved into this closure — it will be kept (via keep())
            // on success or deleted on drop on failure.
            let outcome =
                run_encoding_thread(&output_path, video_rx, audio_rx, width, height, bitrate);

            if outcome.is_ok() {
                // Prevent TempFile from deleting the output file.
                temp.keep();
            }

            if result_tx.send(outcome).is_err() {
                warn!("EncodingPipeline: result receiver dropped before encoding finished");
            }
        });

        Ok(Self {
            result_rx,
            _task: task,
        })
    }

    /// Awaits the encoding thread and returns the path to the finalized MP4.
    ///
    /// Must be called after the `CaptureEngine` has been stopped (which closes
    /// the frame channels and causes the encoding thread to exit).
    ///
    /// # Errors
    ///
    /// Returns [`AppError::EncodingError`] if encoding failed or the task panicked.
    pub async fn finish(self) -> Result<PathBuf, AppError> {
        self.result_rx.await.map_err(|_| {
            AppError::EncodingError("encoding task dropped result sender".to_string())
        })?
    }
}

// ---------------------------------------------------------------------------
// Encoding thread body
// ---------------------------------------------------------------------------

/// Runs the full `AVAssetWriter` lifecycle on the calling (blocking) thread.
///
/// The function returns only after the write loop exits and the writer is
/// finalized.
///
/// # Safety
///
/// `AVAssetWriter` and `AVAssetWriterInput` are `!Send + !Sync`.  This
/// function must only be called from within `tokio::task::spawn_blocking` so
/// that all operations occur on a consistent OS thread.
fn run_encoding_thread(
    output_path: &std::path::Path,
    mut video_rx: mpsc::Receiver<screencapturekit::CMSampleBuffer>,
    mut audio_rx: mpsc::Receiver<screencapturekit::CMSampleBuffer>,
    width: u32,
    height: u32,
    bitrate: u32,
) -> Result<PathBuf, AppError> {
    unsafe {
        // ---------------------------------------------------------------- //
        // 1.  Output URL                                                    //
        // ---------------------------------------------------------------- //
        let path_str = output_path
            .to_str()
            .ok_or_else(|| AppError::EncodingError("output path contains invalid UTF-8".into()))?;
        let ns_path = NSString::from_str(path_str);
        let url = NSURL::fileURLWithPath(&ns_path);

        // ---------------------------------------------------------------- //
        // 2.  AVAssetWriter                                                 //
        // ---------------------------------------------------------------- //
        let file_type = AVFileTypeMPEG4.expect("AVFileTypeMPEG4 must exist");
        let writer = AVAssetWriter::assetWriterWithURL_fileType_error(&url, file_type)
            .map_err(|e| AppError::EncodingError(format!("AVAssetWriter init failed: {e:?}")))?;

        // ---------------------------------------------------------------- //
        // 3.  Video input (H.264)                                          //
        // ---------------------------------------------------------------- //
        let video_settings = build_video_settings(width, height, bitrate);
        let media_video = AVMediaTypeVideo.expect("AVMediaTypeVideo must exist");
        let video_input = AVAssetWriterInput::assetWriterInputWithMediaType_outputSettings(
            media_video,
            Some(&*video_settings),
        );
        video_input.setExpectsMediaDataInRealTime(true);

        // ---------------------------------------------------------------- //
        // 4.  Audio NOTE — not added to writer in this phase.
        //
        //     ScreenCaptureKit delivers audio as LPCM Float32 Non-Interleaved.
        //     AVAssetWriterInput for AAC compression requires interleaved input
        //     or a specific format description that matches the encoder's
        //     expectations.  Passing the SCKit CMSampleBuffer directly to
        //     appendSampleBuffer throws NSInvalidArgumentException.
        //
        //     Using outputSettings=nil (passthrough) also fails because the MP4
        //     container does not support raw LPCM audio tracks.
        //
        //     Proper LPCM→AAC conversion via AudioConverter will be implemented
        //     in Phase 8.  Until then, the encoder is video-only and audio
        //     buffers received from the capture engine are drained and discarded.
        // ---------------------------------------------------------------- //
        writer.addInput(&video_input);
        // NOTE: audio input not added — see Phase 8 comment above.

        // ---------------------------------------------------------------- //
        // 5.  Start writing                                                 //
        // ---------------------------------------------------------------- //
        if !writer.startWriting() {
            return Err(AppError::EncodingError(
                "AVAssetWriter.startWriting() failed".into(),
            ));
        }
        // startSessionAtSourceTime is called lazily on the first video frame
        // using that frame's actual CMTime.  Using a hardcoded time_zero (0 s)
        // mismatches SCKit's wall-clock timestamps and causes AVFoundation to
        // reject every buffer with AVErrorUnknown (-11800).

        // ---------------------------------------------------------------- //
        // 6.  Write loop                                                    //
        // ---------------------------------------------------------------- //
        let mut frame_count = 0u64;
        let mut session_started = false;

        loop {
            // --- Drain (and discard) audio channel ---
            while audio_rx.try_recv().is_ok() {
                // Audio buffers discarded until Phase 8 LPCM→AAC converter.
            }

            // --- Wait for next video frame (blocking) ---
            if let Some(vb) = video_rx.blocking_recv() {
                // sc_buf_to_retained may return None if the underlying
                // CMSampleBufferRef is null or already invalid.
                let Some(retained) = sc_buf_to_retained(&vb) else {
                    continue;
                };

                // Anchor the writing session to the first frame's real
                // presentation timestamp.  This must happen BEFORE the first
                // appendSampleBuffer call and independently of
                // isReadyForMoreMediaData: gating it on readiness means the
                // session is never started when the encoder is still warming
                // up, causing finishWritingWithCompletionHandler to fail with
                // AVErrorUnknown (-11800).
                if !session_started {
                    let first_pts = retained.presentation_time_stamp();
                    writer.startSessionAtSourceTime(first_pts);
                    session_started = true;
                }

                if video_input.isReadyForMoreMediaData() {
                    frame_count += 1;
                    if !video_input.appendSampleBuffer(&retained) {
                        // Writer entered a failed state; surface error below.
                        warn!(
                            frame_count,
                            "appendSampleBuffer returned false — breaking write loop"
                        );
                        break;
                    }
                }
            } else {
                // All video senders disconnected → stop recording.
                info!(
                    frame_count,
                    "video channel disconnected — finalizing output"
                );
                break;
            }
        }

        // Drain any remaining audio after the video loop exits.
        while audio_rx.try_recv().is_ok() {
            // discard
        }

        // ---------------------------------------------------------------- //
        // 7.  Finalize                                                      //
        // ---------------------------------------------------------------- //

        // Guard: if no valid frames were ever received (e.g. the channel was
        // closed before any CMSampleBuffer could be retained) the session was
        // never started.  finishWritingWithCompletionHandler on an unstarted
        // writer produces AVErrorUnknown (-11800), so fail fast instead.
        if !session_started {
            return Err(AppError::EncodingError(
                "no video frames were written — recording session was never started".into(),
            ));
        }
        video_input.markAsFinished();

        // Use the non-deprecated async completion-handler API so that
        // finishWriting works correctly on all supported macOS versions.
        // We block the current spawn_blocking thread using a Condvar.
        let signal = Arc::new((Mutex::new(false), Condvar::new()));
        let signal2 = Arc::clone(&signal);
        let completion = block2::RcBlock::new(move || {
            *signal2.0.lock().unwrap() = true;
            signal2.1.notify_one();
        });
        writer.finishWritingWithCompletionHandler(&*completion);

        // Wait for the completion handler to fire.
        let mut done = signal.0.lock().unwrap();
        while !*done {
            done = signal.1.wait(done).unwrap();
        }
        drop(done);

        // Check final status; surface the NSError if the writer failed.
        if writer.status() != AVAssetWriterStatus::Completed {
            let err_desc = writer
                .error()
                .map(|e| format!("{e:?}"))
                .unwrap_or_else(|| format!("status={}", writer.status().0));
            return Err(AppError::EncodingError(format!(
                "AVAssetWriter finishWriting failed: {err_desc}"
            )));
        }

        info!(path = %output_path.display(), "encoding pipeline finished");
        Ok(output_path.to_path_buf())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Converts a `screencapturekit` [`CMSampleBuffer`] (wrapping a raw
/// `CMSampleBufferRef`) into a `Retained<objc2_core_media::CMSampleBuffer>`
/// suitable for `AVAssetWriterInput::appendSampleBuffer`.
///
/// # Safety
///
/// * The `CMSampleBufferRef` underlying both types is the same CF object.
/// * `Retained::retain` increments the reference count, balancing the `Drop`
///   impl of `screencapturekit::CMSampleBuffer`.
/// * The pointer is non-null whenever the screencapturekit callback delivers
///   a valid buffer.
unsafe fn sc_buf_to_retained(
    sc_buf: &screencapturekit::CMSampleBuffer,
) -> Option<Retained<ObjcCMSampleBuffer>> {
    // SAFETY: toll-free bridge — CMSampleBufferRef is the same type underlying
    // both wrappers. We retain to prevent premature deallocation.
    let raw = sc_buf.as_ptr().cast::<ObjcCMSampleBuffer>();
    // SAFETY: raw is non-null when screencapturekit delivers a valid buffer.
    Retained::retain(raw)
}

/// Builds the H.264 / `VideoToolbox` settings dictionary:
/// `{ AVVideoCodecKey: H264, AVVideoWidthKey: w, AVVideoHeightKey: h,
///    AVVideoCompressionPropertiesKey: { AVVideoAverageBitRateKey: bitrate } }`
///
/// # Safety
///
/// All `Option<&'static NSString>` statics are non-null at runtime.
/// `NSMutableDictionary::insert` handles the `NSCopying` key requirement.
unsafe fn build_video_settings(
    width: u32,
    height: u32,
    bitrate: u32,
) -> Retained<NSMutableDictionary<NSString, AnyObject>> {
    let dict = NSMutableDictionary::<NSString, AnyObject>::new();

    // Codec = H.264.  AVVideoCodecTypeH264 is &'static AVVideoCodecType = &'static NSString.
    // SAFETY: all static keys are non-null by SDK guarantee.
    let codec_key = AVVideoCodecKey.expect("AVVideoCodecKey");
    let codec_val: &objc2_av_foundation::AVVideoCodecType =
        AVVideoCodecTypeH264.expect("AVVideoCodecTypeH264");
    dict.insert(codec_key, codec_val);

    // Width and height
    let width_num = NSNumber::new_u32(width);
    dict.insert(AVVideoWidthKey.expect("AVVideoWidthKey"), &*width_num);

    let height_num = NSNumber::new_u32(height);
    dict.insert(AVVideoHeightKey.expect("AVVideoHeightKey"), &*height_num);

    // Compression properties sub-dictionary (bit-rate)
    let comp = NSMutableDictionary::<NSString, AnyObject>::new();
    let br_num = NSNumber::new_u32(bitrate);
    comp.insert(
        AVVideoAverageBitRateKey.expect("AVVideoAverageBitRateKey"),
        &*br_num,
    );
    dict.insert(
        AVVideoCompressionPropertiesKey.expect("AVVideoCompressionPropertiesKey"),
        &**comp,
    );

    dict
}
