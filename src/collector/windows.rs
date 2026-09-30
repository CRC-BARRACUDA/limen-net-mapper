//! Windows: the neighbour table, read through PowerShell.
//!
//! `Get-NetNeighbor` is the table the kernel already has; nothing is sent to
//! build it. What the script prints is parsed in [`super::windows_lines`],
//! which is compiled everywhere so the format is tested on any machine — only
//! these two calls need Windows.

use std::os::windows::process::CommandExt;
use std::process::Command;

use crate::model::Node;

/// Keeps a console window from flashing up on every scan.
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// One line per neighbour, as `node|<ip>|<mac>|<hostname>`.
///
/// A flat line format rather than JSON: PowerShell's own object output changes
/// shape with the host's culture and version, and a truncated list of lines is
/// worth all but its last line.
const SCRIPT: &str = r#"
$ErrorActionPreference = 'SilentlyContinue'
$ProgressPreference = 'SilentlyContinue'

# Every neighbour the kernel has resolved. `Unreachable` and `Incomplete` are
# entries it gave up on, not machines that answered.
$arp = Get-NetNeighbor -AddressFamily IPv4 |
    Where-Object { $_.State -ne 'Unreachable' -and $_.State -ne 'Incomplete' }

foreach ($entry in $arp) {
    $ip = $entry.IPAddress
    # Multicast and broadcast are not neighbours.
    if ($ip -match '^(?:22[4-9]|23[0-9])\.' -or $ip -eq '255.255.255.255') { continue }
    if (-not $entry.LinkLayerAddress) { continue }
    $mac = $entry.LinkLayerAddress.Replace('-', ':').ToUpper()

    # A name only if this machine already knows one. Nothing is resolved over
    # the network: a lookup per neighbour is this tool announcing itself to
    # whatever is listening on the network it is mapping.
    $hostname = 'Unknown'
    $cached = Get-DnsClientCache -ErrorAction SilentlyContinue |
        Where-Object { $_.Data -eq $ip } | Select-Object -First 1
    if ($cached -and $cached.Entry) { $hostname = $cached.Entry.Split('.')[0] }

    Write-Output ('node|' + $ip + '|' + $mac + '|' + $hostname)
}
"#;

pub fn run_deep_scan() -> Vec<Node> {
    let neighbours = run(&["-NoProfile", "-NonInteractive", "-Command", SCRIPT]);
    let routes = route_print();
    super::windows_lines::parse(&neighbours, &super::windows_lines::parse_gateways(&routes))
}

fn run(args: &[&str]) -> String {
    Command::new("powershell")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

/// The routing table, for the one entry that says which neighbour is the way
/// out.
fn route_print() -> String {
    Command::new("route")
        .args(["print", "-4"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}
