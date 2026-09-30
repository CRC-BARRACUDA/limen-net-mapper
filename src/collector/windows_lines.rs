//! The lines the Windows script prints, read without Windows.
//!
//! `node|<ip>|<mac>|<hostname>`, one per neighbour. Compiled everywhere for the
//! same reason as [`super::bsd`]: the format is text, and a parser only the
//! Windows build compiles is a parser no test suite ever runs.

use crate::model::{DeviceType, Node};

/// The script's output into neighbours.
pub(crate) fn parse(stdout: &str, gateways: &[String]) -> Vec<Node> {
    let mut nodes = Vec::new();
    for line in stdout.lines() {
        let parts: Vec<&str> = line.trim().split('|').collect();
        if parts.len() < 4 || parts[0] != "node" {
            continue;
        }
        let (ip, mac, name) = (parts[1].trim(), parts[2].trim().to_uppercase(), parts[3].trim());
        // The zero address is an entry that never completed and the all-ones
        // one is the link's broadcast; neither is a machine that answered.
        if ip.is_empty()
            || matches!(mac.as_str(), "" | "00:00:00:00:00:00" | "FF:FF:FF:FF:FF:FF")
        {
            continue;
        }
        nodes.push(Node {
            mac,
            ip: ip.to_string(),
            hostname: (name != "Unknown" && !name.is_empty()).then(|| name.to_string()),
            // The way off this network, named by the routing table rather than
            // guessed from an address ending in .1 or .254 — plenty of networks
            // put their gateway somewhere else, and plenty of ordinary machines
            // sit on .1.
            device_type: if gateways.contains(&ip.to_string()) {
                DeviceType::Router
            } else {
                DeviceType::PC
            },
            parent_mac: None,
        });
    }
    nodes.sort_by_key(|n| sortable(&n.ip));
    nodes
}

/// `route print -4` names the gateway of the default route.
pub(crate) fn parse_gateways(stdout: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in stdout.lines() {
        let cols: Vec<&str> = line.split_whitespace().collect();
        // `0.0.0.0  0.0.0.0  <gateway>  <interface>  <metric>`
        if cols.len() >= 3 && cols[0] == "0.0.0.0" && cols[1] == "0.0.0.0" {
            let gw = cols[2];
            if gw.split('.').filter(|o| o.parse::<u8>().is_ok()).count() == 4
                && gw != "0.0.0.0"
                && !out.contains(&gw.to_string())
            {
                out.push(gw.to_string());
            }
        }
    }
    out
}

fn sortable(ip: &str) -> Vec<u8> {
    ip.split('.').filter_map(|o| o.parse().ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINES: &str = "\
node|10.0.1.1|00-11-22-33-44-55|gateway
node|10.0.1.15|A4:83:E7:1B:0C:9D|Unknown
node|10.0.1.20|00:00:00:00:00:00|dead
rubbish
node|10.0.1.9|aa:bb:cc:dd:ee:ff|";

    const ROUTES: &str = "\
IPv4 Route Table
===========================================================================
Network Destination        Netmask          Gateway       Interface  Metric
          0.0.0.0          0.0.0.0       10.0.1.1      10.0.1.15     35
        127.0.0.0        255.0.0.0         On-link       127.0.0.1    331";

    #[test]
    fn the_scripts_lines_read_as_neighbours() {
        let nodes = parse(LINES, &parse_gateways(ROUTES));
        let ips: Vec<&str> = nodes.iter().map(|n| n.ip.as_str()).collect();
        // The unresolved entry and the line that is not one are not machines.
        assert_eq!(ips, vec!["10.0.1.1", "10.0.1.9", "10.0.1.15"], "{ips:?}");

        // A name the script found, and none invented for the two it did not.
        assert_eq!(nodes[0].hostname.as_deref(), Some("gateway"));
        assert_eq!(nodes[2].hostname, None, "`Unknown` is not a name");
        assert_eq!(nodes[1].hostname, None, "an empty field is not a name");

        // The gateway is the one the routes named.
        assert!(matches!(nodes[0].device_type, DeviceType::Router));
        assert!(matches!(nodes[2].device_type, DeviceType::PC));
    }

    #[test]
    fn the_default_route_names_the_gateway() {
        assert_eq!(parse_gateways(ROUTES), vec!["10.0.1.1".to_string()]);
        // `On-link` is not a machine.
        assert!(!parse_gateways(ROUTES).iter().any(|g| g.contains("On-link")));
        assert!(parse_gateways("").is_empty());
    }

    #[test]
    fn nothing_at_all_is_no_neighbours_rather_than_a_panic() {
        assert!(parse("", &[]).is_empty());
        assert!(parse("node|", &[]).is_empty());
        assert!(parse("node|1.2.3.4", &[]).is_empty());
    }
}
