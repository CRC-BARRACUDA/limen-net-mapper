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

/// This machine's own cards, as `card|`/`addr|`/`out|` lines for
/// [`super::windows_self`]: every adapter, every address against the adapter
/// it sits on, and the one route that says which card is the way out.
///
/// Flat lines rather than one joined object, for the same reason the
/// neighbour script prints them: what joins a card to its addresses here is
/// an interface index, and an index is a number in every culture PowerShell
/// runs in.
const SELF_SCRIPT: &str = r#"
$ErrorActionPreference = 'SilentlyContinue'
$ProgressPreference = 'SilentlyContinue'

# Every adapter with a connection name, which is every card somebody could
# point at: the miniports and WAN stubs beneath them have none.
#
# The Win32 classes rather than Get-NetAdapter because the NetAdapter and
# NetTCPIP modules cost about two seconds to load on first use, and this runs
# whenever the screen is drawn. Same facts, named the same way in every
# culture, in a quarter of the time.
foreach ($a in Get-CimInstance -ClassName Win32_NetworkAdapter -Filter "NetConnectionID IS NOT NULL") {
    $mac = ''
    if ($a.MACAddress) { $mac = $a.MACAddress }
    $up = 'False'
    if ($a.NetEnabled) { $up = 'True' }
    # Hardware behind it, rather than a bridge this machine made for itself.
    $phys = 'False'
    if ($a.PhysicalAdapter) { $phys = 'True' }
    # The separator can never appear inside a field: an adapter can be renamed
    # to anything, including a pipe.
    $name = ([string]$a.NetConnectionID).Replace('|', '/')
    Write-Output ('card|' + $a.InterfaceIndex + '|' + $name + '|' + $mac + '|' + $up + '|' + $phys)
}

# Every address this machine answers to, against the card it answers on — and
# the way out, named by the same object: a card with a default gateway offers
# one, and what it costs to go that way decides between them.
#
# The mask is passed on as it comes, dotted. Which bits that is belongs with
# the parser, where it can be tested without a machine to read.
foreach ($c in Get-CimInstance -ClassName Win32_NetworkAdapterConfiguration -Filter "IPEnabled=TRUE") {
    for ($i = 0; $i -lt @($c.IPAddress).Count; $i++) {
        $mask = ''
        if ($c.IPSubnet -and $i -lt @($c.IPSubnet).Count) { $mask = @($c.IPSubnet)[$i] }
        Write-Output ('addr|' + $c.InterfaceIndex + '|' + @($c.IPAddress)[$i] + '|' + $mask)
    }
    if ($c.DefaultIPGateway) {
        $metric = $c.IPConnectionMetric
        if ($null -eq $metric) { $metric = '' }
        Write-Output ('out|' + $c.InterfaceIndex + '|' + $metric)
    }
}
"#;

/// Every network this machine is on.
pub fn interfaces() -> Vec<super::Iface> {
    super::windows_self::parse(&run(&[
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        SELF_SCRIPT,
    ]))
}

/// This machine itself, which no neighbour table contains.
///
/// One node, however many cards: a machine is a machine. Which address it
/// answers to depends on who is asking, so its addresses are not kept here —
/// they belong to the links, and the map writes each one on the line it
/// applies to.
pub fn this_machine() -> Option<Node> {
    let cards = interfaces();
    // No address anywhere is a machine that is on no network: there is nothing
    // to put it on the map beside.
    if cards.iter().all(|c| c.addrs.is_empty()) {
        return None;
    }
    Some(Node {
        // The machine, not one of its cards. Its own hardware addresses are
        // listed beside it.
        mac: crate::SELF.to_string(),
        ips: Vec::new(),
        hostname: hostname(),
        device_type: crate::model::DeviceType::This,
        parent_mac: None,
        macs: cards.iter().map(|c| c.mac.clone()).collect(),
        iface: None,
        local_ips: Vec::new(),
        local_mac: None,
        last_seen: Some(super::now()),
    })
}

/// What this machine calls itself, from the environment rather than a command:
/// it is already here, and a scan should not spend a process on it.
fn hostname() -> Option<String> {
    std::env::var("COMPUTERNAME")
        .ok()
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
}

#[cfg(test)]
mod me {
    /// What this machine looks like on its own map, for a human to check
    /// against what they know their address to be.
    #[test]
    #[ignore = "prints this machine; run with --ignored"]
    fn show_this_machine() {
        for card in super::interfaces() {
            println!(
                "  {:<28} {}  {:<6} {:<24}{}{}",
                card.name,
                card.mac,
                if card.up { "up" } else { "down" },
                card.addrs.join(", "),
                if card.physical { "" } else { "  (virtual)" },
                if card.out { "   <- the way out" } else { "" }
            );
        }
        match super::this_machine() {
            Some(me) => println!(
                "  this machine: {} · {:?} · cards {:?}",
                me.mac,
                me.hostname.as_deref().unwrap_or(""),
                me.macs
            ),
            None => println!("  this machine: not on any network"),
        }
    }
}
