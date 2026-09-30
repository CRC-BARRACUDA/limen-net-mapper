//! macOS: the neighbour table, read through `arp`.
//!
//! There is no `/proc` here and no PowerShell, so this asks the two tools every
//! macOS has: `arp -an` for who is on the link, `netstat -rn` for the way off
//! it. Both are called by absolute path — a scan is not the moment to let
//! `PATH` decide which `arp` runs.
//!
//! What they print is parsed in [`super::bsd`], which is compiled everywhere so
//! the format is tested on any machine; only these two calls need a Mac.

use std::process::Command;

use crate::model::Node;

pub fn run_deep_scan() -> Vec<Node> {
    let arp = run("/usr/sbin/arp", &["-an"]);
    let routes = run("/usr/sbin/netstat", &["-rn", "-f", "inet"]);
    super::bsd::parse(&arp, &super::bsd::parse_gateways(&routes))
}

fn run(program: &str, args: &[&str]) -> String {
    Command::new(program)
        .args(args)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}
