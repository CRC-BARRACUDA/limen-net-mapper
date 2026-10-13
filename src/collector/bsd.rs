//! The BSD `arp`/`netstat` formats, read without a BSD.
//!
//! Compiled everywhere on purpose: the running of a command belongs to the
//! platform that has it, but the shape of what it prints is just text, and text
//! can be tested on any machine. Gated behind `cfg(target_os = "macos")` with
//! the rest, this parser would be checked by nobody — the only builds that
//! compile it are the ones nobody runs the test suite on.

use crate::model::{DeviceType, Node};

/// `arp -an` into neighbours.
///
/// ```text
/// ? (10.0.1.1) at 0:11:22:33:44:55 on en0 ifscope [ethernet]
/// ? (10.0.1.9) at (incomplete) on en0 [ethernet]
/// ```
///
/// BSD prints an octet's leading zero away — `0:11:22` — so each is padded
/// back, or the same machine has one address here and another everywhere else.
pub(crate) fn parse(arp: &str, gateways: &[String]) -> Vec<Node> {
    let mut nodes = Vec::new();
    for line in arp.lines() {
        let Some(open) = line.find('(') else { continue };
        let Some(close) = line[open..].find(')').map(|i| i + open) else {
            continue;
        };
        let ip = &line[open + 1..close];
        let Some(at) = line.find(" at ") else { continue };
        let mac_raw = line[at + 4..].split_whitespace().next().unwrap_or("");
        // An entry the kernel never completed is not a machine that answered.
        if mac_raw.starts_with('(') || mac_raw.is_empty() {
            continue;
        }
        let Some(mac) = normalise_mac(mac_raw) else { continue };
        if is_multicast(ip) || ip == "255.255.255.255" {
            continue;
        }
        nodes.push(Node {
            mac,
            ips: vec![ip.to_string()],
            hostname: name_in_line(line),
            device_type: if gateways.contains(&ip.to_string()) {
                DeviceType::Router
            } else {
                DeviceType::PC
            },
            parent_mac: None,
            macs: Vec::new(),
            iface: None,
            local_ips: Vec::new(),
            local_mac: None,
            last_seen: None,
        });
    }
    nodes.sort_by_key(|n| sortable(n.ip()));
    nodes.dedup_by(|a, b| a.mac == b.mac && a.ip() == b.ip());
    nodes
}

/// `arp` prints a name before the bracket when it has one, and `?` when it does
/// not. Nothing is resolved here: a lookup per neighbour is a burst of traffic
/// announcing this tool to the network it is mapping.
fn name_in_line(line: &str) -> Option<String> {
    let name = line.split_whitespace().next()?;
    (name != "?" && !name.is_empty()).then(|| name.to_string())
}

/// `0:11:22:33:44:55` → `00:11:22:33:44:55`, upper case.
///
/// `None` for the two addresses that are not a machine: the zero address, which
/// is an entry the kernel never completed, and the all-ones one, which is the
/// link's broadcast — it sits in the table under the subnet's last address and
/// answers to nobody.
fn normalise_mac(raw: &str) -> Option<String> {
    let parts: Vec<&str> = raw.split(':').collect();
    if parts.len() != 6 {
        return None;
    }
    let mac: Vec<String> = parts
        .iter()
        .map(|p| format!("{:0>2}", p.to_uppercase()))
        .collect();
    let mac = mac.join(":");
    (!matches!(mac.as_str(), "00:00:00:00:00:00" | "FF:FF:FF:FF:FF:FF")).then_some(mac)
}

/// The gateways in `netstat -rn`, which names them under `default`.
pub(crate) fn parse_gateways(routes: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in routes.lines() {
        let mut cols = line.split_whitespace();
        if cols.next() != Some("default") {
            continue;
        }
        if let Some(gw) = cols.next() {
            // A gateway can be a link (`link#4`) rather than an address, which
            // is a route out of an interface and not a machine to draw.
            if gw.split('.').filter(|o| o.parse::<u8>().is_ok()).count() == 4
                && !out.contains(&gw.to_string())
            {
                out.push(gw.to_string());
            }
        }
    }
    out
}

fn is_multicast(ip: &str) -> bool {
    ip.split('.')
        .next()
        .and_then(|o| o.parse::<u8>().ok())
        .is_some_and(|first| (224..=239).contains(&first))
}

fn sortable(ip: &str) -> Vec<u8> {
    ip.split('.').filter_map(|o| o.parse().ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real `arp -an` output, including the two lines that are not machines.
    const ARP: &str = "\
gateway.home (10.0.1.1) at 0:11:22:33:44:55 on en0 ifscope [ethernet]
? (10.0.1.15) at a4:83:e7:1b:0c:9d on en0 ifscope [ethernet]
? (10.0.1.9) at (incomplete) on en0 [ethernet]
? (224.0.0.251) at 1:0:5e:0:0:fb on en0 ifscope permanent [ethernet]
? (10.0.1.255) at ff:ff:ff:ff:ff:ff on en0 ifscope [ethernet]";

    const ROUTES: &str = "\
Routing tables

Internet:
Destination        Gateway            Flags        Netif Expire
default            10.0.1.1           UGScg          en0
default            link#14            UCSI         utun3
127                127.0.0.1          UCS            lo0";

    #[test]
    fn the_gateway_comes_from_the_routing_table() {
        assert_eq!(parse_gateways(ROUTES), vec!["10.0.1.1".to_string()]);
        // A route out of an interface names a link, not a machine.
        assert!(!parse_gateways(ROUTES).iter().any(|g| g.contains('#')));
    }

    #[test]
    fn arp_output_reads_as_neighbours() {
        let nodes = parse(ARP, &parse_gateways(ROUTES));
        let ips: Vec<&str> = nodes.iter().map(|n| n.ip()).collect();
        // The incomplete entry, the multicast one and the broadcast one are
        // not machines that answered.
        assert_eq!(ips, vec!["10.0.1.1", "10.0.1.15"], "{ips:?}");

        // BSD drops an octet's leading zero; the address it prints is the same
        // machine as everywhere else only once it is put back.
        assert_eq!(nodes[0].mac, "00:11:22:33:44:55");
        assert_eq!(nodes[1].mac, "A4:83:E7:1B:0C:9D");

        // The name `arp` already had, and none invented for the one it did not.
        assert_eq!(nodes[0].hostname.as_deref(), Some("gateway.home"));
        assert_eq!(nodes[1].hostname, None);

        // The way off the network is the one the routes named.
        assert!(matches!(nodes[0].device_type, DeviceType::Router));
        assert!(matches!(nodes[1].device_type, DeviceType::PC));
    }

    #[test]
    fn nothing_at_all_is_no_neighbours_rather_than_a_panic() {
        assert!(parse("", &[]).is_empty());
        assert!(parse("not arp output", &[]).is_empty());
        assert!(parse_gateways("").is_empty());
    }
}
