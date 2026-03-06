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
    AVFileTypeMPEG4, AVMediaTypeAudio, AVMediaTypeVideo, AVVideoAverageBitRateKey,
    AVVideoCodecKey, AVVideoCodecTypeH264, AVVideoCompressionPropertiesKey, AVVideoHeightKey,
    AVVideoWidthKey,
};
use objc2_core_media::CMSampleBuffer as ObjcCMSampleBuffer;
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
        let capture_audio = settings.capture_mic;
        // `width` and `height` are provided by the caller (from CaptureEngine::start)
        // and are guaranteed to be the actual SCStream dimensions — never zero.
        let (result_tx, result_rx) = oneshot::channel::<Result<PathBuf, AppError>>();

        let task = tokio::task::spawn_blocking(move || {
            // `temp` is moved into this closure — it will be kept (via keep())
            // on success or deleted on drop on failure.
            let outcome = run_encoding_thread(
                &output_path,
                video_rx,
                audio_rx,
                width,
                height,
                bitrate,
                capture_audio,
            );

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
    capture_audio: bool,
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
        // 4.  Audio input (AAC)                                            //
        // ---------------------------------------------------------------- //
        let audio_input = if capture_audio {
            let audio_settings = build_audio_settings();
            let media_audio = AVMediaTypeAudio.expect("AVMediaTypeAudio must exist");
            let input = AVAssetWriterInput::assetWriterInputWithMediaType_outputSettings(
                media_audio,
                Some(&*audio_settings),
            );
            input.setExpectsMediaDataInRealTime(true);
            writer.addInput(&input);
            Some(input)
        } else {
            None
        };

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

        let mut audio_frames = 0u64;
        let mut video_closed = false;
        let mut audio_closed = !capture_audio;

        tokio::runtime::Handle::current().block_on(async {
            while !(video_closed && audio_closed) {
                tokio::select! {
                    maybe_vb = video_rx.recv(), if !video_closed => {
                        let Some(vb) = maybe_vb else {
                            video_closed = true;
                            continue;
                        };

                        let pts = sc_time_to_cm(vb.presentation_timestamp());

                        if !session_started {
                            writer.startSessionAtSourceTime(pts);
                            session_started = true;
                        }

                        let Some(sc_pixel_buf) = vb.image_buffer() else {
                            continue;
                        };

                        let Some(retained_pix) = sc_pixel_buf_to_objc(&sc_pixel_buf) else {
                            continue;
                        };

                        if video_input.isReadyForMoreMediaData() {
                            frame_count += 1;
                            if !adaptor.appendPixelBuffer_withPresentationTime(&retained_pix, pts) {
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
                    }

                    maybe_ab = audio_rx.recv(), if !audio_closed => {
                        let Some(ab) = maybe_ab else {
                            audio_closed = true;
                            continue;
                        };

                        let Some(input) = audio_input.as_ref() else {
                            continue;
                        };

                        let pts = sc_time_to_cm(ab.presentation_timestamp());
                        if !session_started {
                            writer.startSessionAtSourceTime(pts);
                            session_started = true;
                        }

                        if input.isReadyForMoreMediaData() {
                            audio_frames += 1;
                            let Some(retained_sample) = sc_sample_buf_to_objc(&ab) else {
                                continue;
                            };

                            if !input.appendSampleBuffer(&retained_sample) {
                                let writer_err = writer
                                    .error()
                                    .map(|e| format!("{e:?}"))
                                    .unwrap_or_else(|| format!("status={}", writer.status().0));
                                warn!(audio_frames, writer_err, "appendSampleBuffer(audio) returned false");
                                append_failed = true;
                                break;
                            }
                        }
                    }
                }
            }
        });

        if video_closed {
            info!(frame_count, audio_frames, "channels disconnected — finalizing output");
        }

        if append_failed {
            // continue to common finalize path below (will cancel writer)
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
        if let Some(input) = audio_input.as_ref() {
            input.markAsFinished();
        }

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

unsafe fn sc_sample_buf_to_objc(
    sample: &screencapturekit::cm::CMSampleBuffer,
) -> Option<Retained<ObjcCMSampleBuffer>> {
    let raw = sample.as_ptr().cast::<ObjcCMSampleBuffer>();
    Retained::retain(raw)
}

fn sc_time_to_cm(sc_time: screencapturekit::cm::CMTime) -> objc2_core_media::CMTime {
    objc2_core_media::CMTime {
        value: sc_time.value,
        timescale: sc_time.timescale,
        flags: objc2_core_media::CMTimeFlags(1),
        epoch: 0,
    }
}

unsafe fn build_audio_settings() -> Retained<NSMutableDictionary<NSString, AnyObject>> {
    let dict = NSMutableDictionary::<NSString, AnyObject>::new();

    let format_id_key = NSString::from_str("AVFormatIDKey");
    let sample_rate_key = NSString::from_str("AVSampleRateKey");
    let channels_key = NSString::from_str("AVNumberOfChannelsKey");
    let bitrate_key = NSString::from_str("AVEncoderBitRateKey");

    let format_id = NSNumber::new_u32(0x6161_6320); // kAudioFormatMPEG4AAC
    let sample_rate = NSNumber::new_u32(48_000);
    let channels = NSNumber::new_u32(2);
    let bitrate = NSNumber::new_u32(128_000);

    dict.insert(&*format_id_key, &*format_id);
    dict.insert(&*sample_rate_key, &*sample_rate);
    dict.insert(&*channels_key, &*channels);
    dict.insert(&*bitrate_key, &*bitrate);

    dict
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
