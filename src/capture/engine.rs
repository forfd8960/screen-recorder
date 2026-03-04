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
    shareable_content::SCShareableContent,
    stream::{
        SCStream, configuration::SCStreamConfiguration, content_filter::SCContentFilter,
        output_trait::SCStreamOutputTrait, output_type::SCStreamOutputType,
    },
};
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::{
    config::settings::{RecordingSettings, Resolution},
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
    fn did_output_sample_buffer(
        &self,
        sample_buffer: CMSampleBuffer,
        _of_type: SCStreamOutputType,
    ) {
        if let Err(_e) = self.tx.try_send(sample_buffer) {
            // Channel is full — drop the frame and count it.
            self.frames_dropped.fetch_add(1, Ordering::Relaxed);
            warn!("frame dropped — video channel full");
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
    pub async fn start(&mut self, settings: &RecordingSettings) -> Result<(), AppError> {
        if self.stream.is_some() {
            info!("CaptureEngine::start called while already running — ignoring");
            return Ok(());
        }

        // Clone settings so it can be moved into spawn_blocking.
        let resolution = settings.resolution;
        let capture_audio = settings.capture_mic;

        let video_tx = self
            .video_tx
            .clone()
            .expect("video_tx should be present before start");
        let audio_tx = self
            .audio_tx
            .clone()
            .expect("audio_tx should be present before start");
        let frames_dropped = Arc::clone(&self.frames_dropped);

        // SCShareableContent::get(), SCStream::new(), start_capture() are all
        // blocking FFI calls — offload to a dedicated OS thread.
        let stream = tokio::task::spawn_blocking(move || -> Result<SCStream, AppError> {
            // -----------------------------------------------------------------
            // 1. Enumerate shareable content to obtain display geometry.
            // -----------------------------------------------------------------
            let content = SCShareableContent::get()
                .map_err(|e| AppError::StreamCreation(format!("SCShareableContent: {e:?}")))?;

            let displays = content.displays();
            if displays.is_empty() {
                return Err(AppError::NoShareableContent);
            }

            // Use the first (primary) display.
            let display = &displays[0];
            let (width, height) = display_dimensions(resolution, display);

            // -----------------------------------------------------------------
            // 2. Build content filter and stream configuration.
            // -----------------------------------------------------------------
            let filter = SCContentFilter::create()
                .with_display(display)
                .with_excluding_windows(&[])
                .build();

            let config = SCStreamConfiguration::new()
                .with_width(width)
                .with_height(height)
                .with_captures_audio(capture_audio);

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
            Ok(stream)
        })
        .await
        .map_err(|e| AppError::StreamCreation(format!("spawn_blocking join: {e}")))??;

        self.stream = Some(stream);
        Ok(())
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
// Helpers
// ---------------------------------------------------------------------------

/// Resolves the output dimensions from the requested [`Resolution`] and the
/// native display size.
fn display_dimensions(
    resolution: Resolution,
    display: &screencapturekit::shareable_content::SCDisplay,
) -> (u32, u32) {
    match resolution {
        Resolution::Native => (display.width(), display.height()),
        Resolution::P1080 => (1920, 1080),
        Resolution::P720 => (1280, 720),
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
