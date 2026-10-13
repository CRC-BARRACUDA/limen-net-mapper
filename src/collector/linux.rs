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
use crate::SELF;

pub fn run_deep_scan() -> Vec<Node> {
    let Ok(arp) = std::fs::read_to_string("/proc/net/arp") else {
        return Vec::new();
    };
    neighbours(&arp, &default_gateways(), &etc_hosts(), &interfaces(), now())
}

/// Read a neighbour table: one machine per hardware address, whatever it
/// answers to.
///
/// The text is taken as an argument rather than read here so that the parsing
/// can be tested against a table, the way the Windows and macOS readers in
/// this module already are. What a kernel writes into `/proc/net/arp` on a
/// machine with two leases is not something a test can arrange by asking the
/// kernel for it.
pub(crate) fn neighbours(
    arp: &str,
    gateways: &[String],
    hosts: &HashMap<String, String>,
    cards: &[Iface],
    when: i64,
) -> Vec<Node> {
    // Every address this machine answers to on that card, and the card that
    // answers: the other end of a link is as much a list as this end is.
    let seen_at = |iface: &str| -> Vec<String> {
        cards
            .iter()
            .find(|c| c.name == iface)
            .map(|c| c.addrs.clone())
            .unwrap_or_default()
    };
    let answered_on = |iface: &str| -> Option<String> {
        cards.iter().find(|c| c.name == iface).map(|c| c.mac.clone())
    };
    let mut nodes: Vec<Node> = Vec::new();

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
        let device_type = if gateways.iter().any(|gw| gw == ip) {
            // The way off this network, named by the routing table rather than
            // guessed from an address ending in .1 — plenty of networks put
            // their gateway somewhere else.
            DeviceType::Router
        } else {
            DeviceType::PC
        };
        // One machine, however many addresses it answers to: a lease renewed
        // into a second while the first is still in the table, an alias, a
        // server on two. The table has a row for each and they are one card.
        if let Some(seen) = nodes.iter_mut().find(|n: &&mut Node| n.mac == mac) {
            if !seen.ips.iter().any(|known| known == ip) {
                seen.ips.push(ip.to_string());
            }
            // Being the way off the network is a fact about the machine, and
            // it may be the second of its addresses that is in the routes.
            if matches!(device_type, DeviceType::Router) {
                seen.device_type = DeviceType::Router;
            }
            if seen.hostname.is_none() {
                seen.hostname = hosts.get(ip).cloned();
            }
            // The same machine can be in the table twice over two cards — one
            // of them a card with no address of ours on it, left over from a
            // cable that is out or a network this machine has since left.
            // Keep the link that can be described from both ends: an entry
            // saying only "seen on enp1s0f0" names a connection this machine
            // cannot say its own side of.
            if seen.local_ips.is_empty() && !seen_at(cols[5]).is_empty() {
                seen.iface = Some(cols[5].to_string());
                seen.local_ips = seen_at(cols[5]);
                seen.local_mac = answered_on(cols[5]);
            }
            continue;
        }
        nodes.push(Node {
            mac,
            ips: vec![ip.to_string()],
            hostname: hosts.get(ip).cloned(),
            device_type,
            parent_mac: None,
            macs: Vec::new(),
            iface: Some(cols[5].to_string()),
            local_ips: seen_at(cols[5]),
            local_mac: answered_on(cols[5]),
            last_seen: Some(when),
        });
    }
    nodes.sort_by_key(|n| sortable(n.ip()));
    nodes
}

/// One of this machine's own network interfaces.
#[derive(Debug, Clone)]
pub struct Iface {
    pub name: String,
    pub mac: String,
    /// Whether the link is up — a card with a cable out of it is still a card.
    pub up: bool,
    /// The addresses it answers to, in order.
    pub addrs: Vec<String>,
    /// Whether this is the way off the machine.
    pub out: bool,
    /// Whether there is hardware behind it, rather than a bridge or a tunnel
    /// this machine made for itself.
    pub physical: bool,
    /// The networks attached to it, as (network, mask) — what a sweep would
    /// have to walk to find everything on them.
    pub nets: Vec<(u32, u32)>,
}

/// Every network this machine is on, card or not.
///
/// Loopback aside, all of them: a docker bridge and a libvirt bridge are real
/// networks with real machines on them, and on the machine being investigated
/// those are often the interesting ones. Whether there is hardware behind an
/// interface is still recorded — the kernel publishes that as a `device` link
/// in `/sys/class/net` — because "this is a card" and "this is a bridge to
/// things running here" are different facts about a network, and the map says
/// which is which rather than hiding one of them.
pub fn interfaces() -> Vec<Iface> {
    let Ok(dir) = std::fs::read_dir("/sys/class/net") else {
        return Vec::new();
    };
    let out_iface = default_iface();
    let mine = local_addresses();
    let mut found: Vec<Iface> = Vec::new();
    for entry in dir.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        // Loopback is this machine talking to itself, and it has no hardware
        // address worth the name.
        if name == "lo" {
            continue;
        }
        let Some(mac) = iface_mac(&name) else {
            continue;
        };
        let physical = entry.path().join("device").exists();
        let up = std::fs::read_to_string(format!("/sys/class/net/{name}/operstate"))
            .map(|s| s.trim() == "up")
            .unwrap_or(false);
        // Which of this machine's addresses are reachable on this card,
        // matched through the networks the routing table attaches to it.
        let nets = on_link(&name);
        let addrs: Vec<String> = mine
            .iter()
            .filter(|ip| nets.iter().any(|net| within(ip, net)))
            .cloned()
            .collect();
        found.push(Iface {
            out: out_iface.as_deref() == Some(name.as_str()),
            name,
            mac,
            up,
            addrs,
            physical,
            nets,
        });
    }
    // The way out first, then the ones that are up, then by name — the order
    // somebody reads their own machine in.
    found.sort_by(|a, b| {
        b.out
            .cmp(&a.out)
            .then(b.up.cmp(&a.up))
            .then(a.name.cmp(&b.name))
    });
    found
}

/// This machine, which is in nobody's neighbour table — least of all its own.
///
/// A neighbour table answers "who else is here", so the machine doing the
/// looking was the one thing the map never showed. Its addresses come from the
/// routing table in `/proc`, like everything else here: no `ip`, no `ifconfig`,
/// nothing sent.
///
/// One node, however many cards: a machine is a machine. Which address it
/// answers to depends on who is asking, so its addresses are not kept here —
/// they belong to the links, and the map writes each one on the line it
/// applies to.
pub fn this_machine() -> Option<Node> {
    let cards = interfaces();
    if cards.iter().all(|c| c.addrs.is_empty()) {
        return None;
    }
    Some(Node {
        // The machine, not one of its cards. Its own hardware addresses are
        // listed beside it.
        mac: SELF.to_string(),
        ips: Vec::new(),
        hostname: hostname(),
        device_type: DeviceType::This,
        parent_mac: None,
        macs: cards.iter().map(|c| c.mac.clone()).collect(),
        iface: None,
        local_ips: Vec::new(),
        local_mac: None,
        last_seen: Some(now()),
    })
}

/// Seconds since the epoch, for saying when something was last answered for.
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Every address this machine answers to, from the routing table's own record
/// of them — the `host LOCAL` entries of `/proc/net/fib_trie`.
///
/// Loopback is left out: it is not an address anybody is asking about.
fn local_addresses() -> Vec<String> {
    let Ok(trie) = std::fs::read_to_string("/proc/net/fib_trie") else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    // The address is on its own line, and what kind of address it is on the
    // next: `|-- 192.168.0.126` then `/32 host LOCAL`.
    let lines: Vec<&str> = trie.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let Some(ip) = line.split("|-- ").nth(1).map(str::trim) else {
            continue;
        };
        if !lines
            .get(i + 1)
            .is_some_and(|next| next.contains("/32 host LOCAL"))
        {
            continue;
        }
        if ip.starts_with("127.") || sortable(ip).len() != 4 {
            continue;
        }
        if !out.iter().any(|seen| seen == ip) {
            out.push(ip.to_string());
        }
    }
    out.sort_by_key(|ip| sortable(ip));
    out
}

/// The interface the default route leaves by.
fn default_iface() -> Option<String> {
    let routes = std::fs::read_to_string("/proc/net/route").ok()?;
    routes.lines().skip(1).find_map(|line| {
        let cols: Vec<&str> = line.split_whitespace().collect();
        // Destination 0.0.0.0 is the route everything else falls back to.
        (cols.len() > 2 && cols[1] == "00000000").then(|| cols[0].to_string())
    })
}

/// The networks reachable on an interface without a gateway — what is directly
/// attached to it.
fn on_link(iface: &str) -> Vec<(u32, u32)> {
    let Ok(routes) = std::fs::read_to_string("/proc/net/route") else {
        return Vec::new();
    };
    routes
        .lines()
        .skip(1)
        .filter_map(|line| {
            let cols: Vec<&str> = line.split_whitespace().collect();
            if cols.len() < 8 || cols[0] != iface || cols[2] != "00000000" {
                return None;
            }
            let net = u32::from_str_radix(cols[1], 16).ok()?.swap_bytes();
            let mask = u32::from_str_radix(cols[7], 16).ok()?.swap_bytes();
            (mask != 0).then_some((net, mask))
        })
        .collect()
}

/// Whether an address is on that network.
fn within(ip: &str, (net, mask): &(u32, u32)) -> bool {
    let o = sortable(ip);
    if o.len() != 4 {
        return false;
    }
    let addr = u32::from_be_bytes([o[0], o[1], o[2], o[3]]);
    addr & mask == *net & mask
}

/// An interface's hardware address, as the kernel publishes it.
fn iface_mac(iface: &str) -> Option<String> {
    let mac = std::fs::read_to_string(format!("/sys/class/net/{iface}/address")).ok()?;
    let mac = mac.trim().to_uppercase();
    (mac.len() == 17 && mac != "00:00:00:00:00:00").then_some(mac)
}

/// What this machine calls itself.
fn hostname() -> Option<String> {
    let name = std::fs::read_to_string("/proc/sys/kernel/hostname").ok()?;
    let name = name.trim().to_string();
    (!name.is_empty()).then_some(name)
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
            for ip in &node.ips {
                let octets: Vec<u8> = ip.split('.').filter_map(|o| o.parse().ok()).collect();
                assert_eq!(octets.len(), 4, "an address: {ip}");
                assert!(!(224..=239).contains(&octets[0]), "multicast: {ip}");
            }
        }
    }

    /// The machine running the test is on the map it draws — once, whatever
    /// it has for cards.
    ///
    /// Asserted on shape and on agreement with the kernel's own files rather
    /// than on this network's addresses: a test that expected 192.168.0.126
    /// would pass here and nowhere else. A machine with no network at all is
    /// allowed to answer nothing.
    #[test]
    fn this_machine_is_one_machine() {
        let cards = interfaces();
        let Some(me) = this_machine() else {
            assert!(cards.iter().all(|c| c.addrs.is_empty()));
            return;
        };
        assert!(matches!(me.device_type, DeviceType::This));
        assert_eq!(me.mac, SELF, "the machine, not one of its cards");
        assert_eq!(me.hostname, hostname(), "what it calls itself");
        // Every card it has, named once each — the one place its hardware
        // addresses belong now that the map draws it as one thing.
        assert_eq!(me.macs.len(), cards.len(), "{:?} of {cards:?}", me.macs);
        for card in &cards {
            assert!(me.macs.contains(&card.mac), "{} is missing", card.name);
        }
        // Its addresses are *not* on it: they belong to the links, which is
        // where the map writes them.
        assert!(me.ips.is_empty(), "an address on the machine itself: {:?}", me.ips);
    }

    /// A neighbour says which card it answered on and what this machine's own
    /// address is over there — the two ends of one link.
    #[test]
    fn a_neighbour_carries_both_ends_of_its_link() {
        let cards = interfaces();
        for n in run_deep_scan() {
            let Some(iface) = &n.iface else {
                panic!("a neighbour with no card: {n:?}");
            };
            let Some(card) = cards.iter().find(|c| &c.name == iface) else {
                // A card that has gone since the table was written.
                continue;
            };
            assert_eq!(
                n.local_ips, card.addrs,
                "{:?} was seen on {iface}, so this end is {:?}",
                n.ips, card.addrs
            );
            assert_eq!(n.local_mac.as_ref(), Some(&card.mac));
            // One row per machine, not per address it answers to.
            assert!(!n.ips.is_empty());
            assert!(n.last_seen.is_some_and(|t| t > 1_600_000_000), "{n:?}");
        }
    }

    /// A table with the same card twice is one machine with two addresses.
    ///
    /// A lease renewed into a second address while the first is still in the
    /// table, an alias, a server answering on two. Drawn as two machines, a
    /// map says there are two computers where there is one — and the second
    /// is the first, which is worse than missing it.
    #[test]
    fn one_card_with_two_addresses_is_one_machine() {
        let table = "\
IP address       HW type     Flags       HW address            Mask     Device
192.168.0.43     0x1         0x2         f3:6d:12:ce:91:e7     *        eth0
192.168.0.56     0x1         0x2         f3:6d:12:ce:91:e7     *        eth0
192.168.0.1      0x1         0x2         af:44:a9:b1:59:ae     *        eth0
192.168.0.99     0x1         0x0         00:00:00:00:00:00     *        eth0
224.0.0.251      0x1         0x2         01:00:5e:00:00:fb     *        eth0
";
        let card = Iface {
            name: "eth0".into(),
            mac: "AA:BB:CC:DD:EE:FF".into(),
            up: true,
            addrs: vec!["192.168.0.2".into()],
            out: true,
            physical: true,
            nets: Vec::new(),
        };
        let found = neighbours(
            table,
            &["192.168.0.1".to_string()],
            &HashMap::new(),
            std::slice::from_ref(&card),
            1_700_000_000,
        );

        assert_eq!(found.len(), 2, "two machines, not four: {found:#?}");
        let pc = found
            .iter()
            .find(|n| n.mac == "F3:6D:12:CE:91:E7")
            .expect("the machine with two addresses");
        assert_eq!(pc.ips, ["192.168.0.43", "192.168.0.56"]);
        assert!(matches!(pc.device_type, DeviceType::PC));
        // Both ends of the link it was seen over.
        assert_eq!(pc.local_ips, ["192.168.0.2"]);
        assert_eq!(pc.local_mac.as_deref(), Some("AA:BB:CC:DD:EE:FF"));

        // The gateway is still the gateway, and the two rows that are not
        // machines — an entry the kernel gave up on, and multicast — are not
        // on the map.
        let gw = found
            .iter()
            .find(|n| n.mac == "AF:44:A9:B1:59:AE")
            .expect("the gateway");
        assert!(matches!(gw.device_type, DeviceType::Router));
        assert!(!found.iter().any(|n| n.ips.iter().any(|ip| ip == "192.168.0.99")));
        assert!(!found.iter().any(|n| n.ips.iter().any(|ip| ip.starts_with("224."))));
    }

    /// One machine on two of this machine's cards: the link kept is the one
    /// this machine can say its own side of.
    ///
    /// A gateway answers over wifi and is also in the table against a wired
    /// card whose cable is out. Taking whichever row came first named the
    /// connection after the dead card — so the map said this machine reached
    /// its router through an interface with no address at all.
    #[test]
    fn a_machine_on_two_cards_keeps_the_link_that_can_be_described() {
        let table = "\
IP address       HW type     Flags       HW address            Mask     Device
192.168.0.1      0x1         0x2         bc:fc:e7:94:6b:90     *        eth-dead
192.168.0.1      0x1         0x2         bc:fc:e7:94:6b:90     *        wlan0
";
        let cards = [
            Iface {
                name: "eth-dead".into(),
                mac: "C4:EF:BB:51:D9:55".into(),
                up: false,
                addrs: Vec::new(),
                out: false,
                physical: true,
                nets: Vec::new(),
            },
            Iface {
                name: "wlan0".into(),
                mac: "42:40:62:FC:A8:DA".into(),
                up: true,
                addrs: vec!["192.168.0.126".into()],
                out: true,
                physical: true,
                nets: Vec::new(),
            },
        ];
        let found = neighbours(table, &[], &HashMap::new(), &cards, 0);
        assert_eq!(found.len(), 1, "one machine: {found:#?}");
        assert_eq!(found[0].iface.as_deref(), Some("wlan0"));
        assert_eq!(found[0].local_ips, ["192.168.0.126"]);
        assert_eq!(found[0].local_mac.as_deref(), Some("42:40:62:FC:A8:DA"));
    }

    /// Being the way off the network is a fact about the machine, whichever of
    /// its addresses the routing table names.
    #[test]
    fn a_gateway_known_by_its_second_address_is_still_a_gateway() {
        let table = "\
IP address       HW type     Flags       HW address            Mask     Device
10.0.0.9         0x1         0x2         af:44:a9:b1:59:ae     *        eth0
10.0.0.1         0x1         0x2         af:44:a9:b1:59:ae     *        eth0
";
        let found = neighbours(
            table,
            &["10.0.0.1".to_string()],
            &HashMap::new(),
            &[],
            0,
        );
        assert_eq!(found.len(), 1);
        assert!(matches!(found[0].device_type, DeviceType::Router), "{found:#?}");
    }

    /// Every network, bridges included — and which is which is still recorded.
    ///
    /// Virtual interfaces were left out once, and they are the interesting
    /// ones on a machine running containers or guests: that is a real network
    /// with real machines on it. What is dropped is loopback, which is this
    /// machine talking to itself.
    #[test]
    fn every_network_but_loopback() {
        let cards = interfaces();
        for card in &cards {
            assert_ne!(card.name, "lo");
            assert_eq!(card.mac.len(), 17);
            assert_eq!(card.mac, card.mac.to_uppercase());
            assert_eq!(
                card.physical,
                std::path::Path::new(&format!("/sys/class/net/{}/device", card.name)).exists(),
                "{} is {} a card",
                card.name,
                if card.physical { "" } else { "not" }
            );
            for ip in &card.addrs {
                assert!(local_addresses().contains(ip), "{ip} is not ours");
            }
        }
        // A bridge this machine made for itself is on the list, when there is
        // one: it is a network, and the things on it are machines.
        for bridge in ["docker0", "virbr0"] {
            if std::path::Path::new(&format!("/sys/class/net/{bridge}")).exists() {
                assert!(
                    cards.iter().any(|c| c.name == bridge),
                    "{bridge} is a network too: {:?}",
                    cards.iter().map(|c| &c.name).collect::<Vec<_>>()
                );
            }
        }
    }

    /// A network's own numbers, worked out the way the routing table writes    /// A network's own numbers, worked out the way the routing table writes    /// A network's own numbers, worked out the way the routing table writes
    /// them — little-endian hex, which is easy to get backwards.
    #[test]
    fn an_address_is_matched_against_its_network() {
        // 192.168.0.0/24
        let net = (u32::from_be_bytes([192, 168, 0, 0]), u32::from_be_bytes([255, 255, 255, 0]));
        assert!(within("192.168.0.126", &net));
        assert!(within("192.168.0.1", &net));
        assert!(!within("192.168.1.126", &net));
        assert!(!within("10.0.0.1", &net));
        assert!(!within("not an address", &net));
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
                node.ips.iter().any(|ip| gateways.contains(ip)),
                "{:?} is {:?} and the routes say {}",
                node.ips,
                node.device_type,
                node.ips.iter().any(|ip| gateways.contains(ip))
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
                "  {:<24} {}  {:<8} {}",
                n.ips.join(","),
                n.mac,
                n.device_type.as_str(),
                n.hostname.as_deref().unwrap_or("")
            );
        }
    }
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
                "  {:<16} {}  {:<6} {}{}",
                card.name,
                card.mac,
                if card.up { "up" } else { "down" },
                card.addrs.join(", "),
                if card.out { "   <- the way out" } else { "" }
            );
        }
        if let Some(me) = super::this_machine() {
            println!(
                "  this machine: {} · {:?} · cards {:?}",
                me.mac,
                me.hostname.as_deref().unwrap_or(""),
                me.macs
            );
        }
        println!("  neighbours:");
        for n in super::run_deep_scan() {
            println!(
                "  {:<24} {}  on {:<16} from {:?}({:?})",
                n.ips.join(","),
                n.mac,
                n.iface.as_deref().unwrap_or(""),
                n.local_ips,
                n.local_mac
            );
        }
    }
}
