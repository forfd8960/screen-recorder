# Tutorial 07: Encoding Pipeline and Audio/Video Sync

## Goal

Build an `AVAssetWriter` pipeline that writes H.264 video and optional AAC audio to temp MP4 files.

## Topics

- Why encoding runs on a dedicated blocking thread
- `TempFile` lifecycle and cleanup guarantees
- Writer setup:
  - `AVAssetWriter` output URL and MP4 file type
  - video input settings (codec, dimensions, bitrate)
  - audio input settings and optional mic track
- Pixel-buffer path:
  - why to append pixel buffers via adaptor
  - avoiding writer failures from sample metadata mismatch
- Session start and timestamp handling:
  - lazy session start at first frame PTS
  - audio/video channel draining strategy
- Finalization and failure branches:
  - mark inputs finished
  - `finishWritingWithCompletionHandler`
  - cancel on failed writer state

## Deliverable

- Recording produces valid MP4 output from channel-fed frames.
