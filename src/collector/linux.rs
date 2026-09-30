//! Linux: the neighbour table, read from `/proc`.
//!
//! `/proc/net/arp` is what the kernel has resolved on this link — the machines
//! this one has actually exchanged packets with. Reading it sends nothing: the
//! scan is a file read, and a machine that has been quiet is simply not in it
//! yet.
//!
//! No `ip neigh`, no `arp`, no `nmap`: each is a program that may not be
//! installed, may print in a different format than the one parsed here, and —
//! on a machine being looked at — may not be the binary it claims to be.

use std::collections::HashMap;

use crate::model::{DeviceType, Node};

pub fn run_deep_scan() -> Vec<Node> {
    let gateways = default_gateways();
    let hosts = etc_hosts();
    let mut nodes = Vec::new();

    let Ok(arp) = std::fs::read_to_string("/proc/net/arp") else {
        return nodes;
    };
    // IP address · HW type · Flags · HW address · Mask · Device
    for line in arp.lines().skip(1) {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 6 {
            continue;
        }
        let (ip, flags, mac) = (cols[0], cols[2], cols[3].to_uppercase());
        // `0x0` is an entry the kernel has given up on, the zero address is one
        // it never completed, and the all-ones one is the link's broadcast.
        // None of the three is a machine that answered.
        if flags == "0x0" || matches!(mac.as_str(), "00:00:00:00:00:00" | "FF:FF:FF:FF:FF:FF") {
            continue;
        }
        // Multicast and broadcast are not neighbours.
        if is_multicast(ip) || ip == "255.255.255.255" {
            continue;
        }
        let device_type = if gateways.contains(&ip.to_string()) {
            // The way off this network, named by the routing table rather than
            // guessed from an address ending in .1 — plenty of networks put
            // their gateway somewhere else.
            DeviceType::Router
        } else {
            DeviceType::PC
        };
        nodes.push(Node {
            mac,
            ip: ip.to_string(),
            hostname: hosts.get(ip).cloned(),
            device_type,
            parent_mac: None,
        });
    }
    nodes.sort_by_key(|n| sortable(&n.ip));
    nodes
}

/// Every gateway in the routing table — the machines that lead off this
/// network.
fn default_gateways() -> Vec<String> {
    let Ok(routes) = std::fs::read_to_string("/proc/net/route") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in routes.lines().skip(1) {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 3 {
            continue;
        }
        // Little-endian hex, as the kernel writes it.
        let Ok(raw) = u32::from_str_radix(cols[2], 16) else {
            continue;
        };
        if raw == 0 {
            continue; // an on-link route has no gateway
        }
        let b = raw.swap_bytes().to_be_bytes();
        let ip = format!("{}.{}.{}.{}", b[0], b[1], b[2], b[3]);
        if !out.contains(&ip) {
            out.push(ip);
        }
    }
    out
}

/// Names this machine already knows, from `/etc/hosts`.
///
/// No reverse DNS: a lookup per neighbour is a burst of traffic to the network
/// being mapped, and on a network being investigated that is the tool
/// announcing itself to whatever is listening. What is already on disk costs
/// nothing.
fn etc_hosts() -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Ok(text) = std::fs::read_to_string("/etc/hosts") else {
        return map;
    };
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("");
        let mut cols = line.split_whitespace();
        let (Some(ip), Some(name)) = (cols.next(), cols.next()) else {
            continue;
        };
        map.entry(ip.to_string()).or_insert_with(|| name.to_string());
    }
    map
}

fn is_multicast(ip: &str) -> bool {
    ip.split('.')
        .next()
        .and_then(|o| o.parse::<u8>().ok())
        .is_some_and(|first| (224..=239).contains(&first))
}

/// An address as four numbers, so `.9` sorts before `.10`.
fn sortable(ip: &str) -> Vec<u8> {
    ip.split('.').filter_map(|o| o.parse().ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The neighbours of the machine running the test — whatever they are.
    ///
    /// Asserted on shape rather than on values: a test that expected this
    /// network's addresses would pass here and fail everywhere else.
    #[test]
    fn the_neighbour_table_reads() {
        for node in run_deep_scan() {
            assert_eq!(node.mac.len(), 17, "a MAC: {}", node.mac);
            assert_eq!(node.mac, node.mac.to_uppercase());
            assert!(node.mac.chars().filter(|c| *c == ':').count() == 5, "{}", node.mac);
            assert_ne!(node.mac, "00:00:00:00:00:00", "an unresolved entry");
            let octets: Vec<u8> = node.ip.split('.').filter_map(|o| o.parse().ok()).collect();
            assert_eq!(octets.len(), 4, "an address: {}", node.ip);
            assert!(!(224..=239).contains(&octets[0]), "multicast: {}", node.ip);
        }
    }

    /// The way off this network comes from the routing table, not from an
    /// address that happens to end in .1 — plenty of networks put their
    /// gateway somewhere else, and plenty of ordinary machines sit on .1.
    #[test]
    fn a_router_is_one_the_routes_name() {
        let gateways = default_gateways();
        for node in run_deep_scan() {
            let is_router = matches!(node.device_type, DeviceType::Router);
            assert_eq!(
                is_router,
                gateways.contains(&node.ip),
                "{} is {:?} and the routes say {}",
                node.ip,
                node.device_type,
                gateways.contains(&node.ip)
            );
        }
        for gw in &gateways {
            let octets: Vec<u8> = gw.split('.').filter_map(|o| o.parse().ok()).collect();
            assert_eq!(octets.len(), 4, "a gateway address: {gw}");
            assert_ne!(gw, "0.0.0.0", "an on-link route has no gateway");
        }
    }

    /// Addresses come back in the order somebody reads them: .9 before .10.
    #[test]
    fn addresses_sort_as_addresses() {
        assert!(sortable("10.0.0.9") < sortable("10.0.0.10"));
        assert!(sortable("10.0.0.2") < sortable("10.0.1.1"));
        assert!(sortable("9.0.0.1") < sortable("10.0.0.1"));
    }

    #[test]
    fn multicast_is_not_a_neighbour() {
        assert!(is_multicast("224.0.0.251"));
        assert!(is_multicast("239.255.255.250"));
        assert!(!is_multicast("192.168.1.1"));
        assert!(!is_multicast("10.218.99.236"));
    }

    /// Names already on disk are used; nothing is asked of the network.
    #[test]
    fn hosts_are_read_not_resolved() {
        let hosts = etc_hosts();
        // Every machine has this one, and it is the shape that matters.
        if let Some(name) = hosts.get("127.0.0.1") {
            assert!(!name.is_empty());
            assert!(!name.starts_with('#'), "a comment was read as a name: {name}");
        }
    }
}

#[cfg(test)]
mod probe {
    /// What this machine's neighbour table looks like, for a human to check
    /// against what they know is on the network.
    #[test]
    #[ignore = "prints the network; run with --ignored"]
    fn show_neighbours() {
        for n in super::run_deep_scan() {
            println!(
                "  {:<16} {}  {:<8} {}",
                n.ip,
                n.mac,
                n.device_type.as_str(),
                n.hostname.as_deref().unwrap_or("")
            );
        }
    }
}
