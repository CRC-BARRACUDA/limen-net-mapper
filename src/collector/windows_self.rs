//! This machine's own cards, as the Windows script prints them.
//!
//! Three kinds of line, in no particular order:
//!
//! ```text
//! card|<index>|<name>|<mac>|<up>|<physical>
//! addr|<index>|<ip>|<mask or prefix length>
//! out|<index>|<metric>
//! ```
//!
//! Compiled everywhere for the same reason as [`super::windows_lines`]: the
//! format is text, and a parser only the Windows build compiles is a parser no
//! test suite ever runs. Only the command that produces it needs Windows.

use super::Iface;

/// The script's output into one [`Iface`] per card.
///
/// Keyed by interface index rather than by name: a name can be renamed, repeat
/// across adapters that come and go, or arrive in a form the shell has already
/// mangled, while the index is what every one of the three lines is joined on.
pub(crate) fn parse(stdout: &str) -> Vec<Iface> {
    let mut cards: Vec<(u32, Iface)> = Vec::new();
    let mut addrs: Vec<(u32, String, u8)> = Vec::new();
    // Every card that offers a way off this machine, and what it costs to go
    // that way. Several can: a laptop on Wi-Fi with a cable in has two.
    let mut ways_out: Vec<(u32, u32)> = Vec::new();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.trim().split('|').collect();
        match parts.first().copied() {
            Some("card") if parts.len() >= 6 => {
                let Ok(idx) = parts[1].trim().parse::<u32>() else {
                    continue;
                };
                let mac = normalise_mac(parts[3]);
                // A card with no hardware address is not one this map can
                // name: every neighbour is identified by its MAC, and so is
                // this machine's side of each link.
                if mac.is_empty() {
                    continue;
                }
                cards.push((
                    idx,
                    Iface {
                        name: parts[2].trim().to_string(),
                        mac,
                        up: truthy(parts[4]),
                        addrs: Vec::new(),
                        out: false,
                        physical: truthy(parts[5]),
                        nets: Vec::new(),
                    },
                ));
            }
            Some("addr") if parts.len() >= 4 => {
                let Ok(idx) = parts[1].trim().parse::<u32>() else {
                    continue;
                };
                let Some(bits) = prefix_bits(parts[3]) else {
                    continue;
                };
                // IPv4 only, and only an address somebody could be asking
                // about. IPv6 arrives on the same cards and this map has
                // nowhere to put it: every neighbour here came out of an IPv4
                // neighbour table. Loopback is this machine talking to itself,
                // and link-local is what a card gives itself when nothing told
                // it otherwise.
                let ip = parts[2].trim();
                if to_u32(ip).is_none() || ip.starts_with("127.") || ip.starts_with("169.254.") {
                    continue;
                }
                addrs.push((idx, ip.to_string(), bits));
            }
            Some("out") if parts.len() >= 2 => {
                if let Ok(idx) = parts[1].trim().parse::<u32>() {
                    // A route that does not say what it costs still counts; it
                    // only loses to one that says it is cheaper.
                    let metric = parts
                        .get(2)
                        .and_then(|m| m.trim().parse::<u32>().ok())
                        .unwrap_or(u32::MAX);
                    ways_out.push((idx, metric));
                }
            }
            _ => {}
        }
    }

    for (idx, ip, bits) in addrs {
        let Some((_, card)) = cards.iter_mut().find(|(i, _)| *i == idx) else {
            continue;
        };
        if let Some(net) = network_of(&ip, bits)
            && !card.nets.contains(&net)
        {
            card.nets.push(net);
        }
        card.addrs.push(ip);
    }

    // The cheapest way out is the one Windows would itself take, and so it is
    // the one the screen calls the way out.
    let out_index = ways_out.iter().min_by_key(|(_, m)| *m).map(|(i, _)| *i);

    let mut found: Vec<Iface> = cards
        .into_iter()
        .map(|(idx, mut c)| {
            c.out = Some(idx) == out_index;
            c
        })
        .collect();

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

/// How many bits of network, written either way: `24`, or the dotted mask
/// `255.255.255.0` that the adapter configuration reports.
///
/// A mask with gaps in it is not a mask, and nothing on a network described
/// that way can be swept.
fn prefix_bits(raw: &str) -> Option<u8> {
    let raw = raw.trim();
    if !raw.contains('.') {
        return raw.parse::<u8>().ok().filter(|b| *b <= 32);
    }
    let mask = to_u32(raw)?;
    let bits = mask.leading_ones();
    // Contiguous, or it is not a netmask: the ones have to be at the top with
    // nothing below them.
    let rebuilt = if bits == 0 { 0 } else { u32::MAX << (32 - bits) };
    (mask == rebuilt).then_some(bits as u8)
}

/// `AA-BB-CC-DD-EE-FF` or `aabbccddeeff` as this map writes a hardware
/// address. Empty when there is nothing usable, including the all-zero address
/// a card with no hardware reports.
fn normalise_mac(raw: &str) -> String {
    let hex: String = raw
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .collect::<String>()
        .to_uppercase();
    if hex.len() != 12 || hex.chars().all(|c| c == '0') {
        return String::new();
    }
    hex.as_bytes()
        .chunks(2)
        .map(|p| String::from_utf8_lossy(p).into_owned())
        .collect::<Vec<_>>()
        .join(":")
}

/// `(network, mask)` for an address and its prefix length, as the sweep wants
/// them: both as numbers, host bits cleared.
fn network_of(ip: &str, bits: u8) -> Option<(u32, u32)> {
    let addr = to_u32(ip)?;
    // A /0 would be every address there is, and `u32::MAX << 32` is undefined
    // besides; a /31 or /32 has no hosts to sweep.
    if bits == 0 || bits > 30 {
        return None;
    }
    let mask = u32::MAX << (32 - bits);
    Some((addr & mask, mask))
}

/// A dotted-quad as a number.
fn to_u32(ip: &str) -> Option<u32> {
    let mut parts = ip.split('.');
    let mut out: u32 = 0;
    for _ in 0..4 {
        out = (out << 8) | parts.next()?.trim().parse::<u8>().ok()? as u32;
    }
    // Four and no more: 1.2.3.4.5 is not an address.
    parts.next().is_none().then_some(out)
}

/// What PowerShell writes for a boolean, whichever way it was produced.
fn truthy(raw: &str) -> bool {
    matches!(raw.trim().to_ascii_lowercase().as_str(), "true" | "1")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the script prints on a machine with a cable, a radio, and a bridge
    /// to things running on itself.
    const SAMPLE: &str = "\
card|12|Ethernet|AA-BB-CC-DD-EE-01|True|True
card|7|Wi-Fi|AA-BB-CC-DD-EE-02|True|True
card|20|vEthernet (WSL)|AA-BB-CC-DD-EE-03|True|False
addr|12|192.168.0.126|255.255.255.0
addr|7|10.0.0.8|255.255.255.0
addr|20|172.20.112.1|255.255.240.0
addr|12|127.0.0.1|255.0.0.0
addr|7|169.254.3.4|255.255.0.0
addr|7|fe80::7de:c342:3a90:d026|64
out|7|25
out|12|35
";

    /// Every card, with its own address, and the networks the sweep needs.
    #[test]
    fn each_card_gets_its_own_address_and_network() {
        let cards = parse(SAMPLE);
        let names: Vec<&str> = cards.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(cards.len(), 3, "{names:?}");

        let wifi = cards.iter().find(|c| c.name == "Wi-Fi").expect("Wi-Fi");
        assert_eq!(wifi.addrs, vec!["10.0.0.8"]);
        assert_eq!(wifi.mac, "AA:BB:CC:DD:EE:02");
        assert_eq!(wifi.nets, vec![(0x0A00_0000, 0xFFFF_FF00)]);
        assert!(wifi.up && wifi.physical);
    }

    /// Several cards can offer a way off this machine. The cheapest is the one
    /// Windows would take, and the only one the screen calls the way out.
    #[test]
    fn the_way_out_is_the_cheapest_route_offered() {
        let cards = parse(SAMPLE);
        assert_eq!(cards[0].name, "Wi-Fi", "the way out is read first");
        assert!(cards[0].out, "metric 25 beats metric 35");
        assert_eq!(cards.iter().filter(|c| c.out).count(), 1);
    }

    /// A route that does not say what it costs still counts — it just loses to
    /// one that says it is cheaper.
    #[test]
    fn a_route_with_no_metric_still_counts() {
        let one = parse("card|3|Only|AA-BB-CC-DD-EE-01|True|True\nout|3\n");
        assert!(one[0].out, "nothing else offered a way out");

        let two = parse(
            "card|3|Vague|AA-BB-CC-DD-EE-01|True|True\n\
             card|4|Cheap|AA-BB-CC-DD-EE-02|True|True\n\
             out|3\n\
             out|4|10\n",
        );
        let cheap = two.iter().find(|c| c.name == "Cheap").expect("Cheap");
        assert!(cheap.out, "a stated cost beats an unstated one");
        assert_eq!(two.iter().filter(|c| c.out).count(), 1);
    }

    /// A bridge to things running on this machine is a network, and is swept
    /// like any other — but it is not a card on the back of the computer, and
    /// the screen says so.
    #[test]
    fn a_virtual_adapter_is_kept_and_marked() {
        let cards = parse(SAMPLE);
        let wsl = cards.iter().find(|c| c.name.contains("WSL")).expect("WSL");
        assert!(!wsl.physical);
        assert_eq!(wsl.addrs, vec!["172.20.112.1"]);
        assert_eq!(
            wsl.nets,
            vec![(0xAC14_7000, 0xFFFF_F000)],
            "a /20 of 172.20.112.1 is 172.20.112.0"
        );
    }

    /// Loopback is this machine talking to itself and link-local is what a card
    /// gives itself when nothing told it otherwise. IPv6 arrives on the same
    /// cards, and every neighbour on this map came out of an IPv4 table.
    #[test]
    fn loopback_link_local_and_ipv6_are_left_out() {
        let cards = parse(SAMPLE);
        for c in &cards {
            for a in &c.addrs {
                assert!(!a.starts_with("127."), "{a}");
                assert!(!a.starts_with("169.254."), "{a}");
                assert!(!a.contains(':'), "{a}");
            }
        }
        let eth = cards.iter().find(|c| c.name == "Ethernet").expect("eth");
        assert_eq!(eth.addrs, vec!["192.168.0.126"], "its real address is kept");
    }

    /// A hardware address is how everything on this map is named, so a card
    /// without a usable one is not one this map can draw.
    #[test]
    fn a_card_with_no_hardware_address_is_dropped() {
        let cards = parse(
            "card|1|Loopback Pseudo-Interface|00-00-00-00-00-00|True|False\n\
             card|2|Tunnel||True|False\n\
             card|3|Real|AA-BB-CC-DD-EE-09|True|True\n",
        );
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].name, "Real");
    }

    /// Hardware addresses arrive in whatever shape the shell produced.
    #[test]
    fn a_hardware_address_is_read_however_it_is_written() {
        for raw in ["AA-BB-CC-DD-EE-01", "aa:bb:cc:dd:ee:01", "AABBCCDDEE01"] {
            assert_eq!(normalise_mac(raw), "AA:BB:CC:DD:EE:01", "{raw}");
        }
        for bad in ["", "AA-BB-CC", "not a mac", "00-00-00-00-00-00"] {
            assert!(normalise_mac(bad).is_empty(), "{bad}");
        }
    }

    /// How much network, written either way the system reports it.
    #[test]
    fn a_prefix_is_read_as_a_count_or_as_a_mask() {
        assert_eq!(prefix_bits("24"), Some(24));
        assert_eq!(prefix_bits("255.255.255.0"), Some(24));
        assert_eq!(prefix_bits("255.255.240.0"), Some(20));
        assert_eq!(prefix_bits("255.0.0.0"), Some(8));
        assert_eq!(prefix_bits("0.0.0.0"), Some(0));
        assert_eq!(prefix_bits("255.255.255.255"), Some(32));
        // Not a mask: the ones have to be at the top with nothing below them.
        assert_eq!(prefix_bits("255.0.255.0"), None);
        assert_eq!(prefix_bits("255.255.1.0"), None);
        // Not a prefix either.
        assert_eq!(prefix_bits("64"), None, "an IPv6 prefix is not ours");
        assert_eq!(prefix_bits(""), None);
        assert_eq!(prefix_bits("wide"), None);
    }

    /// A network is the address with its host bits cleared, and the sizes that
    /// have no hosts to sweep are not networks at all.
    #[test]
    fn a_network_is_the_address_with_its_host_bits_cleared() {
        assert_eq!(
            network_of("192.168.0.126", 24),
            Some((0xC0A8_0000, 0xFFFF_FF00))
        );
        assert_eq!(network_of("10.1.2.3", 8), Some((0x0A00_0000, 0xFF00_0000)));
        // Nothing to sweep, and a /0 is every address there is.
        for bits in [0, 31, 32, 33] {
            assert_eq!(network_of("192.168.0.1", bits), None, "/{bits}");
        }
        assert_eq!(network_of("not.an.address.here", 24), None);
        assert_eq!(to_u32("1.2.3.4.5"), None, "four and no more");
    }

    /// Lines that are not one of the three are not an error: PowerShell says
    /// things of its own, and a truncated run is worth all but its last line.
    #[test]
    fn anything_that_is_not_a_line_we_know_is_skipped() {
        let cards = parse(
            "WARNING: something the shell wanted to say\n\
             \n\
             card|1|Ethernet|AA-BB-CC-DD-EE-01|True|True\n\
             addr|1|192.168.0.2|24\n\
             addr|999|10.0.0.1|24\n\
             card|notanumber|Broken|AA-BB-CC-DD-EE-02|True|True\n",
        );
        assert_eq!(cards.len(), 1);
        assert_eq!(
            cards[0].addrs,
            vec!["192.168.0.2"],
            "an address for no card is dropped"
        );
    }

    /// Nothing at all is no cards, not a panic.
    #[test]
    fn nothing_is_no_cards() {
        assert!(parse("").is_empty());
    }
}
