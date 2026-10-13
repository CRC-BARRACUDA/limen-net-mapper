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
pub use linux::{interfaces, now, run_deep_scan, this_machine, Iface};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::run_deep_scan;

/// Anywhere else: nothing, rather than a guess.
#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
pub fn run_deep_scan() -> Vec<crate::model::Node> {
    Vec::new()
}

/// One of the networks this machine is on.
#[cfg(not(target_os = "linux"))]
pub struct Iface {
    pub name: String,
    pub mac: String,
    pub up: bool,
    pub addrs: Vec<String>,
    pub out: bool,
    pub physical: bool,
    pub nets: Vec<(u32, u32)>,
}

/// Every network this machine is on.
#[cfg(not(target_os = "linux"))]
pub fn interfaces() -> Vec<Iface> {
    Vec::new()
}

/// Seconds since the epoch.
#[cfg(not(target_os = "linux"))]
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// This machine itself, which no neighbour table contains.
///
/// Read from the routing table on Linux. Not yet written for Windows or macOS:
/// each keeps the same facts somewhere else, and a parser written against
/// output nobody here can run is a parser nobody has tested. Those platforms
/// show the map as they always did — their neighbours, without themselves —
/// rather than a guess at where this machine is.
#[cfg(not(target_os = "linux"))]
pub fn this_machine() -> Option<crate::model::Node> {
    None
}
