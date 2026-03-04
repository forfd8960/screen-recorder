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

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_av_foundation::{
    AVAssetWriter, AVAssetWriterInput, AVFileTypeMPEG4, AVMediaTypeAudio, AVMediaTypeVideo,
    AVVideoAverageBitRateKey, AVVideoCodecKey, AVVideoCodecTypeH264,
    AVVideoCompressionPropertiesKey, AVVideoHeightKey, AVVideoWidthKey,
};
use objc2_core_media::{CMSampleBuffer as ObjcCMSampleBuffer, CMTime, CMTimeFlags};
use objc2_foundation::{NSMutableDictionary, NSNumber, NSString, NSURL, ns_string};
use tokio::sync::{mpsc, oneshot};
use tracing::{info, warn};

use crate::{
    config::settings::RecordingSettings,
    encode::{sync::PtsNormalizer, temp_file::TempFile},
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
    // SAFETY: All AVFoundation objects are created and used exclusively on
    // this thread. `spawn_blocking` guarantees a dedicated OS thread.
    // `CMSampleBuffer` type coercions use the toll-free bridge between
    // CMSampleBufferRef (screencapturekit) and the ObjC CMSampleBuffer class
    // (objc2-core-media). Both wrappers point to the same underlying CF object.
    unsafe {
        // ---------------------------------------------------------------- //
        // 1.  Output URL                                                    //
        // ---------------------------------------------------------------- //
        let path_str = output_path
            .to_str()
            .ok_or_else(|| AppError::EncodingError("output path contains invalid UTF-8".into()))?;
        let ns_path = NSString::from_str(path_str);
        // SAFETY: NSURL::fileURLWithPath is a class method returning a valid URL for UTF-8 paths.
        let url = NSURL::fileURLWithPath(&ns_path);

        // ---------------------------------------------------------------- //
        // 2.  AVAssetWriter                                                 //
        // ---------------------------------------------------------------- //
        // SAFETY: AVFileTypeMPEG4 is a non-null static NSString guaranteed by the SDK.
        let file_type = AVFileTypeMPEG4.expect("AVFileTypeMPEG4 must exist");
        let writer = AVAssetWriter::assetWriterWithURL_fileType_error(&url, file_type)
            .map_err(|e| AppError::EncodingError(format!("AVAssetWriter init failed: {e:?}")))?;

        // ---------------------------------------------------------------- //
        // 3.  Video input (H.264)                                          //
        // ---------------------------------------------------------------- //
        let video_settings = build_video_settings(width, height, bitrate);
        // SAFETY: AVMediaTypeVideo is a non-null static NSString.
        let media_video = AVMediaTypeVideo.expect("AVMediaTypeVideo must exist");
        let video_input = AVAssetWriterInput::assetWriterInputWithMediaType_outputSettings(
            media_video,
            Some(&*video_settings),
        );
        video_input.setExpectsMediaDataInRealTime(true);

        // ---------------------------------------------------------------- //
        // 4.  Audio input (AAC 48 kHz stereo 128 kbps)                    //
        // ---------------------------------------------------------------- //
        let audio_settings = build_audio_settings();
        // SAFETY: AVMediaTypeAudio is a non-null static NSString.
        let media_audio = AVMediaTypeAudio.expect("AVMediaTypeAudio must exist");
        let audio_input = AVAssetWriterInput::assetWriterInputWithMediaType_outputSettings(
            media_audio,
            Some(&*audio_settings),
        );
        audio_input.setExpectsMediaDataInRealTime(true);

        writer.addInput(&video_input);
        writer.addInput(&audio_input);

        // ---------------------------------------------------------------- //
        // 5.  Start writing session at time zero                          //
        // ---------------------------------------------------------------- //
        if !writer.startWriting() {
            return Err(AppError::EncodingError(
                "AVAssetWriter.startWriting() failed".into(),
            ));
        }

        // kCMTimeZero: value=0, timescale=1, flags=kCMTimeFlags_Valid(1), epoch=0
        let time_zero = CMTime {
            value: 0,
            timescale: 1,
            flags: CMTimeFlags(1),
            epoch: 0,
        };
        writer.startSessionAtSourceTime(time_zero);

        // ---------------------------------------------------------------- //
        // 6.  Write loop                                                    //
        // ---------------------------------------------------------------- //
        let mut video_norm = PtsNormalizer::new();
        let mut audio_norm = PtsNormalizer::new();

        loop {
            // --- Drain audio channel (non-blocking) ---
            while let Ok(ab) = audio_rx.try_recv() {
                if audio_input.isReadyForMoreMediaData()
                    && let Some(retained) = sc_buf_to_retained(&ab)
                {
                    let _pts_secs = ab
                        .presentation_timestamp()
                        .as_seconds()
                        .map(|s| audio_norm.normalize_secs(s));
                    audio_input.appendSampleBuffer(&retained);
                }
            }

            // --- Wait for next video frame (blocking) ---
            if let Some(vb) = video_rx.blocking_recv() {
                if video_input.isReadyForMoreMediaData()
                    && let Some(retained) = sc_buf_to_retained(&vb)
                {
                    let _pts_secs = vb
                        .presentation_timestamp()
                        .as_seconds()
                        .map(|s| video_norm.normalize_secs(s));
                    video_input.appendSampleBuffer(&retained);
                }
            } else {
                // All video senders disconnected → stop recording.
                info!("video channel disconnected — finalizing output");
                break;
            }
        }

        // Drain any remaining audio after the video loop exits.
        while let Ok(ab) = audio_rx.try_recv() {
            if audio_input.isReadyForMoreMediaData()
                && let Some(retained) = sc_buf_to_retained(&ab)
            {
                audio_input.appendSampleBuffer(&retained);
            }
        }

        // ---------------------------------------------------------------- //
        // 7.  Finalize                                                      //
        // ---------------------------------------------------------------- //
        video_input.markAsFinished();
        audio_input.markAsFinished();

        #[allow(deprecated)]
        let finished = writer.finishWriting();
        if !finished {
            return Err(AppError::EncodingError(
                "AVAssetWriter.finishWriting() returned false".into(),
            ));
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

/// Builds the AAC audio settings dictionary:
/// `{ AVFormatIDKey: kAudioFormatMPEG4AAC, AVSampleRateKey: 48000,
///    AVNumberOfChannelsKey: 2, AVEncoderBitRateKey: 128000 }`
///
/// Audio settings keys are absent from objc2-av-foundation 0.3.2 (the
/// `AVAudioSettings.rs` generated file is empty).  We construct them via
/// `ns_string!` literals.
///
/// # Safety
///
/// `ns_string!` returns `&'static NSString` — always valid, no allocation.
unsafe fn build_audio_settings() -> Retained<NSMutableDictionary<NSString, AnyObject>> {
    // kAudioFormatMPEG4AAC = FourCC 'mp4a' = 0x6D70_3461
    const K_AUDIO_FORMAT_MPEG4_AAC: u32 = 0x6D70_3461;

    let dict = NSMutableDictionary::<NSString, AnyObject>::new();

    let fmt_num = NSNumber::new_u32(K_AUDIO_FORMAT_MPEG4_AAC);
    dict.insert(ns_string!("AVFormatIDKey"), &*fmt_num);

    let sr_num = NSNumber::new_f64(48_000.0);
    dict.insert(ns_string!("AVSampleRateKey"), &*sr_num);

    let ch_num = NSNumber::new_u32(2);
    dict.insert(ns_string!("AVNumberOfChannelsKey"), &*ch_num);

    let br_num = NSNumber::new_u32(128_000);
    dict.insert(ns_string!("AVEncoderBitRateKey"), &*br_num);

    dict
}
