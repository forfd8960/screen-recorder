// Crate-level lint policy (see AGENTS.md and specs/0003-impl-plan.md §Constitution Check).
// Unsafe code is forbidden at the crate root.  Individual modules that must
// call into Objective-C FFI (capture::engine, encode::pipeline) carry a
// module-level `#[allow(unsafe_code)]` attribute with per-block `// SAFETY:`
// comments.
#![deny(unsafe_code)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![warn(rust_2018_idioms)]

mod capture;
mod config;
mod encode;
mod output;
mod ui;

fn main() {
    // Phase 1 scaffold: modules compile, lint gates pass.
    // Full application entry-point wired in Phase 2 (T012 / T013).
    println!("screen-recorder: scaffold OK");
}
