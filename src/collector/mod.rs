//! Who else is on this network, per platform.
//!
//! Each platform answers the same question from its own table of neighbours —
//! the addresses this machine has actually spoken to — and returns the same
//! [`Node`](crate::model::Node). Nothing is sent to find them: a neighbour
//! table is what the kernel already knows.

// The two text formats, compiled everywhere so both are tested everywhere —
// only the command that produces each belongs to a platform. Which means that
// on any one build, one of them is called by nobody; that is the point of it,
// not an oversight.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod bsd;
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
mod windows_lines;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::run_deep_scan;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::run_deep_scan;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::run_deep_scan;

/// Anywhere else: nothing, rather than a guess.
#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
pub fn run_deep_scan() -> Vec<crate::model::Node> {
    Vec::new()
}
