// Crate-level lint policy (see AGENTS.md and specs/0003-impl-plan.md §Constitution Check).
#![deny(unsafe_code)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![warn(rust_2018_idioms)]

//! `screen-recorder` library crate.
//!
//! Exposes all domain modules so that integration tests (and future
//! sub-crates) can access public APIs without relying on the binary entry
//! point.

pub mod app;
pub mod capture;
pub mod config;
pub mod encode;
pub mod error;
pub mod output;
pub mod ui;
