//! Screen capture layer.
//!
//! Sub-modules:
//! - [`permissions`]     – TCC permission check and onboarding
//! - [`content_filter`]  – `SCContentFilter` builder and display/window enumeration
//! - [`engine`]          – `SCStream` lifecycle and frame ingestion
//! - [`audio`]           – Microphone capture configuration

// Implemented in Phase 2 (M1)–Phase 4 (M3).

pub mod audio;
pub mod engine;
pub mod permissions;
