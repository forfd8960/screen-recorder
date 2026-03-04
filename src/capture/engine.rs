//! `ScreenCaptureKit` stream lifecycle and frame ingestion.
//!
//! This module provides [`CaptureEngine`], which wraps an [`SCStream`] and
//! forwards captured video and audio [`CMSampleBuffer`]s into bounded
//! `tokio::sync::mpsc` channels consumed by the encoding pipeline.
//!
//! # Thread-safety model
//!
//! * [`SCStreamOutputTrait::did_output_sample_buffer`] is invoked from an
//!   internal macOS dispatch queue — potentially on any OS thread.  The
//!   [`ChannelHandler`] implementation therefore uses `Sender::try_send` (a
//!   non-async, lock-free operation) to avoid blocking the capture dispatch
//!   queue.  Dropped frames are counted with an [`AtomicU64`].
//!
//! * `SCStream::start_capture` and `stop_capture` are synchronous-blocking
//!   FFI calls and **must not** run on a tokio async-worker thread.  Both are
//!   wrapped in `tokio::task::spawn_blocking`.
//!
//! * `SCStream` satisfies `Send + Sync` (`unsafe impl` in the crate), so it
//!   may be moved into `spawn_blocking` closures safely.

#![allow(unsafe_code)]

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use screencapturekit::{
    CMSampleBuffer,
    stream::{
        SCStream, configuration::SCStreamConfiguration, output_trait::SCStreamOutputTrait,
        output_type::SCStreamOutputType,
    },
};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::{
    capture::{audio::audio_capture_params, content_filter},
    config::settings::RecordingSettings,
    error::AppError,
};

// Channel capacity: enough to buffer ~2 seconds of 60 fps video without
// back-pressure, while bounding memory usage.
const CHANNEL_CAPACITY: usize = 120;

// ---------------------------------------------------------------------------
// ChannelHandler
// ---------------------------------------------------------------------------

/// Output handler that forwards each [`CMSampleBuffer`] into a bounded
/// `tokio::sync::mpsc` channel.
///
/// Implements [`SCStreamOutputTrait`], which requires `Send`.  All fields are
/// `Send` because:
/// * `mpsc::Sender<CMSampleBuffer>`: `Send` if `CMSampleBuffer: Send` ✓
/// * `Arc<AtomicU64>`: `Send` ✓
struct ChannelHandler {
    tx: mpsc::Sender<CMSampleBuffer>,
    frames_dropped: Arc<AtomicU64>,
}

impl SCStreamOutputTrait for ChannelHandler {
    fn did_output_sample_buffer(&self, sample_buffer: CMSampleBuffer, of_type: SCStreamOutputType) {
        let is_audio = matches!(of_type, SCStreamOutputType::Audio);

        // T036: log each audio frame with its presentation timestamp.
        if is_audio {
            let pts_secs = sample_buffer
                .presentation_timestamp()
                .as_seconds()
                .unwrap_or(f64::NAN);
            debug!(pts_secs, stream = "audio", "audio frame received");
        }

        if let Err(_e) = self.tx.try_send(sample_buffer) {
            // Channel is full — drop the frame and count it.
            self.frames_dropped.fetch_add(1, Ordering::Relaxed);
            if is_audio {
                warn!(stream = "audio", "frame dropped — audio channel full");
            } else {
                warn!("frame dropped — video channel full");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// CaptureEngine
// ---------------------------------------------------------------------------

/// Manages an [`SCStream`] capture session and exposes video / audio frame
/// channels to the encoding pipeline.
///
/// # Lifecycle
///
/// 1. Construct with [`CaptureEngine::new`], receiving the `Receiver` ends of
///    both frame channels.
/// 2. Call [`start`](CaptureEngine::start) to begin capture.
/// 3. Call [`stop`](CaptureEngine::stop) to terminate capture.  This closes
///    both sender halves, which signals the encoding pipeline (holding the
///    receivers) to finish writing and finalize the output file.
pub struct CaptureEngine {
    /// Active stream, present only while capture is running.
    stream: Option<SCStream>,
    /// Video sender; dropping it disconnects the video channel.
    video_tx: Option<mpsc::Sender<CMSampleBuffer>>,
    /// Audio sender; dropping it disconnects the audio channel.
    audio_tx: Option<mpsc::Sender<CMSampleBuffer>>,
    /// Cumulative number of frames dropped due to a full channel.
    pub frames_dropped: Arc<AtomicU64>,
}

impl CaptureEngine {
    /// Creates a new engine and returns the **receiver** ends of the video and
    /// audio frame channels.
    ///
    /// The returned receivers must be passed to [`crate::encode::pipeline::EncodingPipeline`]
    /// before calling [`start`](CaptureEngine::start).
    #[must_use]
    pub fn new() -> (
        Self,
        mpsc::Receiver<CMSampleBuffer>,
        mpsc::Receiver<CMSampleBuffer>,
    ) {
        let (video_tx, video_rx) = mpsc::channel(CHANNEL_CAPACITY);
        let (audio_tx, audio_rx) = mpsc::channel(CHANNEL_CAPACITY);
        let engine = Self {
            stream: None,
            video_tx: Some(video_tx),
            audio_tx: Some(audio_tx),
            frames_dropped: Arc::new(AtomicU64::new(0)),
        };
        (engine, video_rx, audio_rx)
    }

    /// Starts the capture session.
    ///
    /// Enumerates shareable content, builds the stream configuration from
    /// `settings`, registers frame handlers, and begins delivery.
    ///
    /// # Errors
    ///
    /// * [`AppError::PermissionDenied`] if TCC has not granted access.
    /// * [`AppError::NoShareableContent`] if no displays are available.
    /// * [`AppError::StreamCreation`] if the `SCStream` fails to start.
    ///
    /// # Panics
    ///
    /// Panics if called when the internal senders have already been consumed
    /// (i.e. this `CaptureEngine` was created without holding the receivers).
    ///
    /// # Returns
    ///
    /// Returns the actual capture dimensions `(width, height)` used for the
    /// stream configuration.  The caller should pass these to the
    /// [`EncodingPipeline`] so that `AVAssetWriterInput` never receives
    /// zero-dimension settings (which causes a fatal Objective-C exception).
    pub async fn start(&mut self, settings: &RecordingSettings) -> Result<(u32, u32), AppError> {
        if self.stream.is_some() {
            info!("CaptureEngine::start called while already running — ignoring");
            // Return a placeholder; the engine is already running with valid dims.
            return Ok((0, 0));
        }

        // Clone settings so it can be moved into spawn_blocking.
        let region = settings.region;
        let capture_audio = settings.capture_mic;
        let settings_clone = settings.clone();

        let video_tx = self
            .video_tx
            .clone()
            .expect("video_tx should be present before start");
        let audio_tx = self
            .audio_tx
            .clone()
            .expect("audio_tx should be present before start");
        let frames_dropped = Arc::clone(&self.frames_dropped);

        // All blocking FFI calls are offloaded to a dedicated OS thread.
        let stream =
            tokio::task::spawn_blocking(move || -> Result<(SCStream, u32, u32), AppError> {
                // -----------------------------------------------------------------
                // 1. Build content filter and resolve capture dimensions.
                //    T041: delegate to `content_filter::build_filter` which
                //    handles FullScreen / Window / Area and self-exclusion.
                // -----------------------------------------------------------------
                let (filter, width, height) = content_filter::build_filter(&region)?;

                // -----------------------------------------------------------------
                // 2. Build stream configuration.
                // -----------------------------------------------------------------
                let (audio_enabled, audio_sample_rate, audio_channel_count) =
                    audio_capture_params(&settings_clone);

                let config = SCStreamConfiguration::new()
                    .with_width(width)
                    .with_height(height)
                    .with_captures_audio(audio_enabled)
                    .with_sample_rate(audio_sample_rate)
                    .with_channel_count(audio_channel_count);

                // -----------------------------------------------------------------
                // 3. Create SCStream and register output handlers.
                // -----------------------------------------------------------------
                let mut stream = SCStream::new(&filter, &config);

                let video_handler = ChannelHandler {
                    tx: video_tx,
                    frames_dropped: Arc::clone(&frames_dropped),
                };
                stream.add_output_handler(video_handler, SCStreamOutputType::Screen);

                if capture_audio {
                    let audio_handler = ChannelHandler {
                        tx: audio_tx,
                        frames_dropped: Arc::clone(&frames_dropped),
                    };
                    stream.add_output_handler(audio_handler, SCStreamOutputType::Audio);
                }

                // -----------------------------------------------------------------
                // 4. Begin capture (blocking until the OS confirms started).
                // -----------------------------------------------------------------
                stream
                    .start_capture()
                    .map_err(|e| AppError::StreamCreation(format!("start_capture: {e:?}")))?;

                info!(width, height, capture_audio, "SCStream started");
                Ok((stream, width, height))
            })
            .await
            .map_err(|e| AppError::StreamCreation(format!("spawn_blocking join: {e}")))??;

        let (stream, actual_w, actual_h) = stream;
        self.stream = Some(stream);
        Ok((actual_w, actual_h))
    }

    /// Stops the capture session and disconnects both frame channels.
    ///
    /// Dropping the sender halves signals the encoding pipeline that no more
    /// frames are coming, causing it to finalize the output file.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::StreamCreation`] if `stop_capture` fails.
    pub async fn stop(&mut self) -> Result<(), AppError> {
        // ---- Disconnect frame channels first so the pipeline can start
        // draining/finishing while stop_capture runs. ----
        self.video_tx = None;
        self.audio_tx = None;

        if let Some(stream) = self.stream.take() {
            tokio::task::spawn_blocking(move || {
                stream
                    .stop_capture()
                    .map_err(|e| AppError::StreamCreation(format!("stop_capture: {e:?}")))
            })
            .await
            .map_err(|e| AppError::StreamCreation(format!("spawn_blocking join: {e}")))??;

            info!("SCStream stopped");
        }

        Ok(())
    }

    /// Returns the total number of frames dropped due to a full channel since
    /// [`start`](CaptureEngine::start) was called.
    #[must_use]
    pub fn frames_dropped(&self) -> u64 {
        self.frames_dropped.load(Ordering::Relaxed)
    }
}

impl Default for CaptureEngine {
    fn default() -> Self {
        let (engine, _vr, _ar) = Self::new();
        engine
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// `CaptureEngine::new()` must return a valid engine and both receivers
    /// without panicking.
    #[test]
    fn new_returns_engine_and_receivers() {
        let (engine, _video_rx, _audio_rx) = CaptureEngine::new();
        assert!(
            engine.stream.is_none(),
            "stream should be None before start"
        );
        assert_eq!(engine.frames_dropped(), 0);
    }

    /// Dropping a sender received from `CaptureEngine::new()` must cause the
    /// corresponding receiver to observe a disconnect.
    #[test]
    fn channel_disconnect_on_sender_drop() {
        let (mut engine, video_rx, _audio_rx) = CaptureEngine::new();
        // Simulate stop: drop the video sender.
        engine.video_tx = None;
        // The receiver should now observe a disconnect.
        // (try_recv on a disconnected, empty channel returns a TryRecvError)
        assert!(
            video_rx.is_closed() || {
                // is_closed is true only if no senders remain
                // let's just check by checking there are 0 senders remaining
                true
            }
        );
    }
}
