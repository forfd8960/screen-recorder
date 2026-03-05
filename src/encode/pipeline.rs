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
    AVAssetWriter, AVAssetWriterInput, AVAssetWriterInputPixelBufferAdaptor, AVAssetWriterStatus,
    AVFileTypeMPEG4, AVMediaTypeVideo, AVVideoAverageBitRateKey, AVVideoCodecKey,
    AVVideoCodecTypeH264, AVVideoCompressionPropertiesKey, AVVideoHeightKey, AVVideoWidthKey,
};
use objc2_core_video::CVPixelBuffer as ObjcCVPixelBuffer;
use objc2_foundation::{NSMutableDictionary, NSNumber, NSString, NSURL};
use tokio::sync::{mpsc, oneshot};
use tracing::{info, warn};

use crate::{
    config::settings::{RecordingSettings, VideoQuality},
    encode::temp_file::TempFile,
    error::AppError,
};

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

        let bitrate = bitrate_for_quality(settings.quality);
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
        writer.addInput(&video_input);

        // ---------------------------------------------------------------- //
        // 3b. Pixel buffer adaptor                                         //
        //                                                                   //
        //     SCKit CMSampleBuffers carry ScreenCapture-specific metadata   //
        //     extensions (dirty rects, display time, content scale, etc.).  //
        //     When those buffers are passed directly to                     //
        //     AVAssetWriterInput.appendSampleBuffer, the H.264 encoder      //
        //     chokes on the unknown extensions and returns AVErrorUnknown   //
        //     (-11800), putting the writer into AVAssetWriterStatusFailed.  //
        //                                                                   //
        //     The correct approach is to extract the raw CVPixelBuffer from //
        //     the CMSampleBuffer and feed it through                        //
        //     AVAssetWriterInputPixelBufferAdaptor, which bypasses the      //
        //     sample-level metadata entirely.                               //
        // ---------------------------------------------------------------- //
        let adaptor =
            AVAssetWriterInputPixelBufferAdaptor::
                assetWriterInputPixelBufferAdaptorWithAssetWriterInput_sourcePixelBufferAttributes(
                    &video_input,
                    None, // let AVFoundation decide the pool format
                );

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
        // Tracks whether the loop was broken by an appendSampleBuffer failure
        // rather than a normal channel-close.  If true we must cancel the
        // writer instead of calling finishWritingWithCompletionHandler, which
        // would itself fail with AVErrorUnknown (-11800) on a Failed writer.
        let mut append_failed = false;

        loop {
            // --- Drain (and discard) audio channel ---
            while audio_rx.try_recv().is_ok() {
                // Audio buffers discarded until Phase 8 LPCM→AAC converter.
            }

            // --- Wait for next video frame (blocking) ---
            if let Some(vb) = video_rx.blocking_recv() {
                // Extract the raw CVPixelBuffer and the presentation timestamp
                // from the screencapturekit CMSampleBuffer.  We bypass
                // appendSampleBuffer entirely to avoid the SCKit-specific
                // metadata extensions that cause AVErrorUnknown (-11800).
                let sc_pts = vb.presentation_timestamp();
                let pts_value = sc_pts.value;
                let pts_timescale = sc_pts.timescale;

                // Build a CMTime compatible with objc2_core_media.
                let pts = objc2_core_media::CMTime {
                    value: pts_value,
                    timescale: pts_timescale,
                    flags: objc2_core_media::CMTimeFlags(1), // kCMTimeFlags_Valid
                    epoch: 0,
                };

                // Anchor the writing session to the first frame's real PTS.
                // Must happen BEFORE the first appendPixelBuffer call.
                if !session_started {
                    writer.startSessionAtSourceTime(pts);
                    session_started = true;
                }

                // Get the CVPixelBuffer from the screencapturekit CMSampleBuffer.
                let Some(sc_pixel_buf) = vb.image_buffer() else {
                    // Frame has no pixel data (e.g. status-only frame) — skip.
                    continue;
                };

                // Toll-free bridge: screencapturekit's CVPixelBuffer is the
                // same CF object as objc2_core_video::CVPixelBuffer.  We
                // retain to keep the buffer alive for the duration of this
                // call, balancing screencapturekit's own retain on the buffer.
                let Some(retained_pix) = sc_pixel_buf_to_objc(&sc_pixel_buf) else {
                    continue;
                };

                if video_input.isReadyForMoreMediaData() {
                    frame_count += 1;
                    if !adaptor.appendPixelBuffer_withPresentationTime(&retained_pix, pts) {
                        // Writer entered a failed state.  Log the underlying
                        // AVFoundation error before breaking so we have full
                        // context in the log.
                        let writer_err = writer
                            .error()
                            .map(|e| format!("{e:?}"))
                            .unwrap_or_else(|| format!("status={}", writer.status().0));
                        warn!(
                            frame_count,
                            writer_err,
                            "appendPixelBuffer returned false — writer entered failed state"
                        );
                        append_failed = true;
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
            // If the writer was started (startWriting succeeded) but no session
            // was begun, cancel to release the file and any internal resources.
            writer.cancelWriting();
            return Err(AppError::EncodingError(
                "no video frames were written — recording session was never started".into(),
            ));
        }

        // If the write loop exited because appendSampleBuffer put the writer
        // into a Failed state, calling finishWritingWithCompletionHandler would
        // itself fail with AVErrorUnknown (-11800).  Instead we call
        // cancelWriting() for proper cleanup (it also deletes the partial
        // output file) and surface the writer's underlying error.
        if append_failed || writer.status() == AVAssetWriterStatus::Failed {
            let err_desc = writer
                .error()
                .map(|e| format!("{e:?}"))
                .unwrap_or_else(|| format!("status={}", writer.status().0));
            // cancelWriting is a no-op when already Failed, but still
            // triggers internal cleanup and deletes any partial output file.
            writer.cancelWriting();
            return Err(AppError::EncodingError(format!(
                "AVAssetWriter entered failed state during recording: {err_desc}"
            )));
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

/// Toll-free bridges a `screencapturekit` `CVPixelBuffer` (a
/// `*mut c_void` wrapping a `CVPixelBufferRef`) into an
/// `objc2_core_video::CVPixelBuffer` suitable for
/// `AVAssetWriterInputPixelBufferAdaptor::appendPixelBuffer:withPresentationTime:`.
///
/// We pass the raw pixel buffer *directly* without adding SCKit-specific
/// metadata extensions, which is what caused `AVErrorUnknown (-11800)` when
/// using `appendSampleBuffer` with the full `CMSampleBuffer`.
///
/// # Safety
///
/// * `CVPixelBufferRef` is a `CFTypeRef`-compatible opaque pointer.  Both
///   screencapturekit and objc2_core_video ultimately point to the same CF
///   object — the bridge is zero-cost.
/// * `CFRetained::retain` increments the CF retain count and balances the
///   `Drop` in screencapturekit's `CVPixelBuffer` wrapper.
/// * The pointer is non-null whenever screencapturekit delivers a valid image
///   buffer.
unsafe fn sc_pixel_buf_to_objc(
    sc_pix: &screencapturekit::cv::CVPixelBuffer,
) -> Option<Retained<ObjcCVPixelBuffer>> {
    // SAFETY: CVPixelBufferRef is a CFTypeRef-compatible opaque pointer.
    // Both wrappers point to the same CF object.
    let raw = sc_pix.as_ptr().cast::<ObjcCVPixelBuffer>();
    // SAFETY: raw is non-null when screencapturekit delivers a valid pixel buffer.
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

#[must_use]
const fn bitrate_for_quality(quality: VideoQuality) -> u32 {
    match quality {
        VideoQuality::High => 8_000_000,
        VideoQuality::Medium => 4_000_000,
        VideoQuality::Low => 2_000_000,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quality_bitrate_mapping_matches_spec() {
        assert_eq!(bitrate_for_quality(VideoQuality::High), 8_000_000);
        assert_eq!(bitrate_for_quality(VideoQuality::Medium), 4_000_000);
        assert_eq!(bitrate_for_quality(VideoQuality::Low), 2_000_000);
    }
}
