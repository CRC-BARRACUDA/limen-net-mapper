use crate::collector;
use crate::model::{Node, Stats};
use crate::*;

pub(crate) fn main_view(
    lang: &str,
    stats: &Stats,
    cards: &[collector::Iface],
    swept: Option<crate::discover::Swept>,
    note: Option<String>,
) -> Value {
    let t = |k: &str| catalog().tr(lang, k);

    let total_str = t("ui.stats_total").replace("{n}", &stats.total.to_string());
    let detail_str = t("ui.stats_routers")
        .replace("{r}", &stats.routers.to_string())
        .replace("{s}", &stats.servers.to_string());

    // Where the machine doing the looking sits, which is the one address a
    // neighbour table can never tell anybody — card by card, because a machine
    // with two cards is on two networks and only one of them is "my IP".
    let mut mine = Vec::new();
    if !cards.is_empty() {
        mine.push(label(t("ui.this_machine")).strong());
        for card in cards {
            let address = if card.addrs.is_empty() {
                t("ui.no_address")
            } else {
                card.addrs.join(", ")
            };
            let mut line = t("ui.card")
                .replace("{iface}", &card.name)
                .replace("{ip}", &address)
                .replace("{mac}", &card.mac);
            // A bridge to things running on this machine is a network like any
            // other and is swept like any other — but it is not a card, and
            // the screen says which is which rather than leaving the reader to
            // know that `virbr0` is not a socket on the back of the computer.
            if !card.physical {
                line.push_str(&t("ui.virtual"));
            }
            if card.out {
                line.push_str(&t("ui.the_way_out"));
            } else if !card.up {
                line.push_str(&t("ui.down"));
            }
            mine.push(label(line).mono());
        }
        mine.push(separator());
    }

    let mut widgets = Vec::new();
    // What just happened, once, above the screen it happened to.
    if let Some(note) = note {
        widgets.push(label(note).strong());
        widgets.push(separator());
    }
    widgets.extend([label(t("ui.subtitle")).weak(), separator()]);
    widgets.extend(mine);
    // What the last sweep came to, said once, after it.
    if let Some(swept) = swept {
        let mut said = t("ui.swept").replace("{n}", &swept.asked.to_string());
        if swept.skipped > 0 {
            said.push_str(&t("ui.swept_skipped").replace("{n}", &swept.skipped.to_string()));
        }
        widgets.push(label(said).strong());
        widgets.push(separator());
    }
    widgets.extend(vec![
            label(total_str).strong(),
            label(detail_str),
            separator(),
            row(vec![
                button(t("ui.scan"), CAP, "scan").primary(),
                // Sends. Its own button, and its own question before it runs:
                // on a network being investigated, announcing yourself to
                // every address on it is a decision, not a refresh.
                button(t("ui.discover"), CAP, "discover").confirm_labelled(
                    &t("ui.discover_title"),
                    &t("ui.discover_what"),
                    // Named, so the button says what pressing it does rather
                    // than agreeing to something unstated.
                    &t("ui.discover_yes"),
                    &t("ui.discover_no"),
                ),
                button(t("ui.map"), CAP, "topology"),
            ]),
            label(t("ui.discover_help")).weak(),
            separator(),
            // What has been learnt is a file on this machine, and both things
            // somebody wants to do with such a file are here: carry it off, or
            // be rid of it.
            row(vec![
                button(t("db.save"), CAP, "save_db"),
                button(t("db.clear"), CAP, "clear_db")
                    .danger()
                    .confirm_labelled(
                        &t("db.clear_title"),
                        &t("db.clear_what"),
                        &t("db.clear_yes"),
                        &t("db.clear_no"),
                    ),
            ]),
    ]);
    window(t("ui.title"), widgets)
}

pub(crate) fn scanning_view(lang: &str, discover: bool) -> Value {
    let t = |k: &str| catalog().tr(lang, k);
    let (doing, hint) = if discover {
        ("scan.sweeping", "scan.sweeping_hint")
    } else {
        ("scan.in_progress", "scan.hint")
    };
    let mut v = window_modal_sized(
        t("scan.title"),
        "net-mapper.scanning",
        520.0,
        vec![step(t(doing), "loading"), label(t(hint)).weak()],
    );
    // Авто-опрос интерфейсом Limen[cite: 3, 8]
    if let Value::Object(m) = &mut v {
        // A few times a second, not as fast as the module can answer: the
        // scan is the work and this screen is the report on it.
        m.insert(
            "auto".into(),
            json!({ "capability": CAP, "method": "scan_poll", "args": {}, "every_ms": 200 }),
        );
    }
    v
}
/// The map itself, drawn by the host.
///
/// One circle per machine, named by what it is — a hostname if anybody knows
/// one, its hardware address otherwise — and the addresses written on the
/// lines rather than under the circles.
///
/// That is where an address actually belongs. A machine does not *have* an
/// address; it answers to one *on a link*, and a machine with two cards
/// answers to two. Written under the circle, one of them has to be chosen and
/// the map then says the machine is that address. Written on the line, both
/// are true at once and the reader can see which applies to which connection.
pub(crate) fn topology_view(lang: &str, nodes: &[Node]) -> Value {
    let t = |k: &str| catalog().tr(lang, k);
    if nodes.is_empty() {
        return window(
            t("map.title"),
            vec![label(t("map.empty")).weak(), separator(),
                 button(t("ui.scan"), CAP, "scan").primary()],
        );
    }

    let circles: Vec<limen_sdk_rust::ui::DiagramNode> = nodes.iter().map(circle(lang)).collect();
    let me = nodes
        .iter()
        .find(|n| matches!(n.device_type, model::DeviceType::This));

    let mut lines: Vec<limen_sdk_rust::ui::DiagramLink> = Vec::new();
    for n in nodes {
        if matches!(n.device_type, model::DeviceType::This) {
            continue;
        }
        // Everything in a neighbour table was seen *by* this machine, over one
        // of its cards: that is the connection, and the two addresses on it
        // are what each end answered to.
        if let Some(me) = me {
            let mut line = limen_sdk_rust::ui::link(me.mac.clone(), n.mac.clone())
                // What the connection is, said when the pointer is on it. Two
                // machines and what each answers to *here*: written along the
                // line instead, as this first did, they crowded the middle of
                // a map where every line meets at one hub.
                // Both ends named the way everything else on this map names a
                // machine: what a person calls it, and the card that answers
                // for it here. This machine has several cards, so which one
                // is half the answer.
                .label(format!(
                    "{} ↔ {}",
                    who(
                        &me.hostname.clone().or_else(|| Some(t("field.key_self"))),
                        &n.local_mac,
                    ),
                    who(&n.hostname, &Some(n.mac.clone())),
                ))
                .info(t("link.that_end"), end(&n.ips, &Some(n.mac.clone())));
            // Only when there is something to say: a machine kept from an
            // older scan was not seen over any link this machine still has,
            // and a card reading "This end: —" says less than nothing.
            let here = end(&n.local_ips, &n.local_mac);
            if !here.is_empty() {
                line = line.info(t("link.this_end"), here);
            }
            if let Some(iface) = &n.iface {
                line = line.info(t("field.th_iface"), iface.clone());
            }
            if let Some(when) = n.last_seen {
                line = line.info(t("field.last_seen"), stamp(when));
            }
            // Dashed: a machine kept from an older scan, which was not seen
            // over any link this machine still has.
            if n.local_ips.is_empty() {
                line = line.dashed();
            }
            lines.push(line);
        } else if let Some(hub) = n.parent_mac.clone().or_else(|| hub_for(n, nodes)) {
            // No machine of our own on the map — a platform that cannot yet
            // say which machine it is running on. Everything hangs from the
            // gateway of its own network instead, which is the best the
            // neighbour table alone can say.
            lines.push(
                limen_sdk_rust::ui::link(n.mac.clone(), hub)
                    .info(t("link.that_end"), end(&n.ips, &Some(n.mac.clone())))
                    .dashed(),
            );
        }
    }

    window(
        t("map.title"),
        vec![
            // No title on the diagram and nothing above it: the tab is already
            // headed "Network map", and a line saying so again is a line of the
            // map's own room spent saying nothing.
            // Double-clicking a circle opens that host in a tab of its own.
            limen_sdk_rust::ui::diagram("", circles, lines)
                .fills()
                .on_activate(CAP, "host"),
            separator(),
            row(vec![
                button(t("ui.back"), CAP, "ui"),
                button(t("ui.scan"), CAP, "scan"),
            ]),
        ],
    )
}

/// A machine, as a name and the card answering for it: `PC-HOME(F3:6D:…)`.
///
/// Without a name there is only the card, which is the machine's only name as
/// far as the network is concerned — and `F3:6D:…(F3:6D:…)` says one thing
/// twice. Without a card there is only the name.
fn who(name: &Option<String>, mac: &Option<String>) -> String {
    match (name.as_deref().unwrap_or("").trim(), mac.as_deref().unwrap_or("").trim()) {
        ("", "") => String::new(),
        (name, "") => name.to_string(),
        ("", mac) => mac.to_string(),
        (name, mac) => format!("{name}({mac})"),
    }
}

/// One end of a link, as the two things that identify it: the address it
/// answers to there, and the card that answers.
///
/// `192.168.0.43(F3:6D:12:CE:91:E7)` — the address first, because that is what
/// a reader is looking for, and the hardware behind it in brackets, because
/// that is the part that cannot be changed by moving to another network. Each
/// on its own is half an answer: an address without a card does not say which
/// machine, and a card without an address does not say where.
fn end(ips: &[String], mac: &Option<String>) -> String {
    // Every address it answers to there, then the one card that answers them:
    // a machine renumbered by DHCP while the old lease is still in the table
    // has two addresses and one card, and saying only one of them would make
    // two readings of the same machine look like a contradiction.
    let ips = ips.join(",");
    match (ips.trim(), mac.as_deref().unwrap_or("").trim()) {
        ("", "") => String::new(),
        (ips, "") => ips.to_string(),
        ("", mac) => mac.to_string(),
        (ips, mac) => format!("{ips}({mac})"),
    }
}

/// One machine as a circle: its name above, its hardware address under it.
fn circle(lang: &str) -> impl Fn(&Node) -> limen_sdk_rust::ui::DiagramNode + '_ {
    let t = move |k: &str| catalog().tr(lang, k);
    move |n: &Node| {
        let is_me = matches!(n.device_type, model::DeviceType::This);
        // What this machine is called, and what it is. A hostname when
        // anybody knows one — that is what a person calls a machine — and the
        // hardware address under it, which is what the network calls it and
        // the one name it cannot change by moving.
        let (name, under) = match (&n.hostname, is_me) {
            (Some(host), _) => (host.clone(), macs_of(n)),
            (None, true) => (t("field.key_self"), macs_of(n)),
            (None, false) => (n.mac.clone(), String::new()),
        };
        let mut node = limen_sdk_rust::ui::node(n.mac.clone(), name)
            .kind(n.device_type.as_str())
            .detail(under)
            .info(
                t("field.th_kind"),
                t(&format!("field.key_{}", n.device_type.as_str())),
            );
        if is_me {
            node = node.info(t("field.th_mac"), macs_of(n));
        } else {
            node = node.info(t("field.th_ip"), n.ips.join(", "));
            node = node.info(t("field.th_mac"), n.mac.clone());
            if !n.local_ips.is_empty() {
                node = node.info(t("field.seen_from"), end(&n.local_ips, &n.local_mac));
            }
            if let Some(iface) = &n.iface {
                node = node.info(t("field.th_iface"), iface.clone());
            }
        }
        if let Some(host) = &n.hostname {
            node = node.info(t("field.th_host"), host.clone());
        }
        if let Some(when) = n.last_seen {
            node = node.info(t("field.last_seen"), stamp(when));
        }
        node
    }
}

/// Every hardware address a machine has, which for this one is every card.
fn macs_of(n: &Node) -> String {
    if n.macs.is_empty() {
        return n.mac.clone();
    }
    n.macs.join(" · ")
}

/// A time as a date somebody can read.
pub(crate) fn stamp(secs: i64) -> String {
    let (days, rest) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}", rest / 3600, (rest % 3600) / 60)
}

/// The civil date of a day number, by Howard Hinnant's algorithm.
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Which network a node is on, as far as the map can tell.
///
/// The interface it was seen on when that is known — the neighbour table says
/// which card each answer arrived on, and that is the real answer. Otherwise
/// the first three numbers of the address, which is a guess at a /24 and is
/// only ever needed for rows kept by a build that did not record the card.
fn network_of(n: &Node) -> String {
    if let Some(iface) = &n.iface {
        return format!("on:{iface}");
    }
    let mut octets: Vec<&str> = n.ip().split('.').collect();
    octets.truncate(3);
    format!("in:{}", octets.join("."))
}

/// What a node hangs from.
///
/// The gateway **of its own network** — not whichever router happens to be
/// first in the database. A map that hung every machine off one router drew a
/// picture of a network that does not exist: a laptop on wifi joined to a
/// phone it was tethered to last week, by way of a line that was never there.
///
/// Failing a gateway, this machine's own card on that network: everything in
/// the neighbour table was seen *by* that card, so it is the honest centre of
/// what it saw. Failing both, nothing — a circle on its own is the truth about
/// a machine whose network nothing else on the map belongs to.
fn hub_for(n: &Node, nodes: &[Node]) -> Option<String> {
    // A gateway is the top of its own network here. What it hangs from is off
    // this map entirely, and pointing it back at a machine it serves would
    // draw the same line twice, in both directions.
    if matches!(n.device_type, model::DeviceType::Router) {
        return None;
    }
    let network = network_of(n);
    let here = |other: &&Node| network_of(other) == network && other.mac != n.mac;
    let gateway = nodes
        .iter()
        .filter(here)
        .find(|other| matches!(other.device_type, model::DeviceType::Router));
    let card = nodes
        .iter()
        .filter(here)
        .find(|other| matches!(other.device_type, model::DeviceType::This));
    gateway.or(card).map(|hub| hub.mac.clone())
}

/// One host, in a tab of its own: everything the scan learned about it.
///
/// The hover card says as much as fits beside a circle. This is the same facts
/// with room to read them, somewhere that stays open while the map is used.
pub(crate) fn host_view(lang: &str, node: Option<&Node>) -> Value {
    let t = |k: &str| catalog().tr(lang, k);
    let Some(n) = node else {
        return window(t("host.unknown_title"), vec![label(t("host.unknown")).weak()]);
    };
    let name = match &n.device_type {
        model::DeviceType::This => t("field.key_self"),
        _ => n.hostname.clone().unwrap_or_else(|| n.ips.join(", ")),
    };
    let mut rows = vec![
        vec![t("field.th_ip"), n.ips.join(", ")],
        vec![t("field.th_mac"), n.mac.clone()],
        vec![
            t("field.th_kind"),
            t(&format!("field.key_{}", n.device_type.as_str())),
        ],
    ];
    if let Some(host) = &n.hostname {
        rows.insert(0, vec![t("field.th_host"), host.clone()]);
    }
    if let Some(parent) = &n.parent_mac {
        rows.push(vec![t("field.behind"), parent.clone()]);
    }
    if n.ips.len() > 1 {
        rows.push(vec![t("field.also"), n.ips[1..].join(", ")]);
    }
    if !n.macs.is_empty() {
        rows.push(vec![t("field.th_mac"), n.macs.join(", ")]);
    }
    if !n.local_ips.is_empty() {
        rows.push(vec![t("field.seen_from"), end(&n.local_ips, &n.local_mac)]);
    }
    if let Some(iface) = &n.iface {
        rows.push(vec![t("field.th_iface"), iface.clone()]);
    }
    window(
        name,
        vec![
            limen_sdk_rust::ui::table(vec![t("host.field"), t("host.value")], rows),
            separator(),
            label(t("host.what")).weak(),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::DeviceType;

    fn node(ip: &str, mac: &str, kind: DeviceType, iface: Option<&str>) -> Node {
        Node {
            mac: mac.into(),
            ips: ip.split(',').map(str::to_string).collect(),
            hostname: None,
            device_type: kind,
            parent_mac: None,
            macs: Vec::new(),
            iface: iface.map(str::to_string),
            local_ips: Vec::new(),
            local_mac: None,
            last_seen: None,
        }
    }

    /// This machine as the collector now returns it: one node, its cards
    /// listed beside it, and no address of its own.
    fn me(cards: &[&str]) -> Node {
        Node {
            mac: crate::SELF.into(),
            ips: Vec::new(),
            hostname: Some("PC-HOME".into()),
            device_type: DeviceType::This,
            parent_mac: None,
            macs: cards.iter().map(|m| m.to_string()).collect(),
            iface: None,
            local_ips: Vec::new(),
            local_mac: None,
            last_seen: None,
        }
    }

    /// A neighbour seen over one of this machine's cards.
    fn seen(
        ip: &str,
        mac: &str,
        kind: DeviceType,
        iface: &str,
        from: (&str, &str),
    ) -> Node {
        Node {
            local_ips: vec![from.0.into()],
            local_mac: Some(from.1.into()),
            last_seen: Some(1_700_000_000),
            ..node(ip, mac, kind, Some(iface))
        }
    }

    /// What the map sends for a machine with two cards and three neighbours.
    fn drawn() -> Value {
        let nodes = vec![
            me(&["42:40:62:FC:A8:DA", "20:7B:D5:1A:3E:04"]),
            seen(
                "192.168.0.1",
                "BC:FC:E7:94:6B:90",
                DeviceType::Router,
                "wlp2s0",
                ("192.168.0.126", "42:40:62:FC:A8:DA"),
            ),
            seen(
                "192.168.10.1",
                "00:E2:59:00:FD:D0",
                DeviceType::Router,
                "enp198s0",
                ("192.168.10.5", "20:7B:D5:1A:3E:04"),
            ),
            // Kept from an older scan: no link this machine still has.
            node("10.218.99.236", "D6:D3:31:41:5C:C5", DeviceType::PC, None),
        ];
        topology_view("en", &nodes)
    }

    fn diagram(view: &Value) -> &Value {
        view["widgets"]
            .as_array()
            .expect("widgets")
            .iter()
            .find(|w| w["kind"] == "diagram")
            .expect("a diagram")
    }

    /// One circle per machine. A machine with two cards is one machine, which
    /// is the whole point: it was drawn twice, and the two halves of one
    /// laptop looked like two computers on two networks.
    #[test]
    fn a_machine_with_two_cards_is_one_circle() {
        let view = drawn();
        let nodes = diagram(&view)["nodes"].as_array().expect("nodes");
        assert_eq!(nodes.len(), 4, "one each: {nodes:#?}");
        let mine: Vec<&Value> = nodes.iter().filter(|n| n["kind"] == "self").collect();
        assert_eq!(mine.len(), 1, "this machine, once: {mine:#?}");

        // Named by what a person calls it, with what the network calls it
        // underneath — both cards, because it has both.
        assert_eq!(mine[0]["label"], "PC-HOME");
        let under = mine[0]["detail"].as_str().expect("the hardware addresses");
        assert!(under.contains("42:40:62:FC:A8:DA"), "{under}");
        assert!(under.contains("20:7B:D5:1A:3E:04"), "{under}");
    }

    /// What a fact on a line says, by name.
    fn fact<'a>(edge: &'a Value, name: &str) -> &'a str {
        edge["info"]
            .as_array()
            .expect("the line's facts")
            .iter()
            .find(|f| f["name"] == name)
            .unwrap_or_else(|| panic!("no {name} on {edge}"))["value"]
            .as_str()
            .unwrap_or_default()
    }

    /// The addresses belong to the connection, and the connection says them:
    /// this machine's at this end, the other machine's at the other.
    ///
    /// Not written under the circles, because an address is not a property of
    /// a machine — one with two cards has two — and not written along the
    /// lines either, because on a map where every line meets at one hub that
    /// is two labels per line crowding the same middle. The line holds them
    /// and hands them over when it is pointed at.
    #[test]
    fn each_line_says_what_answers_at_each_end() {
        let view = drawn();
        let edges = diagram(&view)["edges"].as_array().expect("edges");
        let to = |mac: &str| {
            edges
                .iter()
                .find(|e| e["to"] == mac)
                .unwrap_or_else(|| panic!("no line to {mac}: {edges:#?}"))
        };

        let wifi = to("BC:FC:E7:94:6B:90");
        assert_eq!(wifi["from"], "self");
        assert_eq!(
            fact(wifi, "This end"), "192.168.0.126(42:40:62:FC:A8:DA)",
            "this end: the address, and the card that answers there"
        );
        assert_eq!(fact(wifi, "That end"), "192.168.0.1(BC:FC:E7:94:6B:90)");
        assert_eq!(fact(wifi, "Interface"), "wlp2s0", "and which card it was");
        // Named for the two machines it joins, so the card says whose line it
        // is before it says anything else.
        assert_eq!(
            wifi["label"], "PC-HOME(42:40:62:FC:A8:DA) ↔ BC:FC:E7:94:6B:90",
            "both ends named, each with the card answering for it — and a \
             machine nobody has a name for is named by its card alone, once"
        );
        assert_eq!(wifi["dashed"], false, "it was seen, not guessed");

        // The second card is a second link with a second pair of addresses —
        // and neither address appears on the machine itself.
        let wired = to("00:E2:59:00:FD:D0");
        assert_eq!(fact(wired, "This end"), "192.168.10.5(20:7B:D5:1A:3E:04)");
        assert_eq!(fact(wired, "That end"), "192.168.10.1(00:E2:59:00:FD:D0)");

        let nodes = diagram(&view)["nodes"].as_array().expect("nodes");
        let me = nodes.iter().find(|n| n["id"] == "self").expect("this machine");
        let said = me.to_string();
        assert!(!said.contains("192.168.0.126"), "an address on the machine: {said}");
        assert!(!said.contains("192.168.10.5"), "an address on the machine: {said}");
    }

    /// One card, several addresses: a lease renewed into a second address
    /// while the old one is still in the table, an alias, a server answering
    /// on two. One machine, one circle, and the line says all of them.
    #[test]
    fn a_card_with_several_addresses_says_all_of_them() {
        let nodes = vec![
            me(&["F3:6D:12:CE:91:E7"]),
            Node {
                ips: vec!["192.168.0.43".into(), "192.168.0.56".into()],
                local_ips: vec!["192.168.0.2".into(), "192.168.0.3".into()],
                local_mac: Some("F3:6D:12:CE:91:E7".into()),
                ..node("", "AF:44:A9:B1:59:AE", DeviceType::Router, Some("eth0"))
            },
        ];
        let view = topology_view("en", &nodes);
        let edges = diagram(&view)["edges"].as_array().expect("edges");
        assert_eq!(edges.len(), 1, "one machine, one line: {edges:#?}");
        assert_eq!(
            fact(&edges[0], "This end"),
            "192.168.0.2,192.168.0.3(F3:6D:12:CE:91:E7)"
        );
        assert_eq!(
            fact(&edges[0], "That end"),
            "192.168.0.43,192.168.0.56(AF:44:A9:B1:59:AE)"
        );
        // And still one circle for it, not one per address.
        assert_eq!(diagram(&view)["nodes"].as_array().expect("nodes").len(), 2);
    }

    /// A machine kept from an older scan is drawn, and the line to it is
    /// dashed: nothing this machine has now saw it.
    #[test]
    fn a_machine_from_an_older_scan_is_joined_by_a_dashed_line() {
        let view = drawn();
        let edges = diagram(&view)["edges"].as_array().expect("edges");
        let old = edges
            .iter()
            .find(|e| e["to"] == "D6:D3:31:41:5C:C5")
            .expect("the old one");
        assert_eq!(old["dashed"], true, "{old}");
        assert_eq!(fact(old, "That end"), "10.218.99.236(D6:D3:31:41:5C:C5)");
        assert!(
            old["info"].as_array().expect("facts").iter().all(|f| f["name"] != "This end"),
            "nothing of ours was at that end: {old}"
        );
    }

    /// A neighbour nobody has a name for is named by the one name it cannot
    /// change: its hardware address.
    #[test]
    fn a_machine_with_no_name_is_called_by_its_hardware() {
        let view = drawn();
        let nodes = diagram(&view)["nodes"].as_array().expect("nodes");
        let gw = nodes
            .iter()
            .find(|n| n["id"] == "BC:FC:E7:94:6B:90")
            .expect("the gateway");
        assert_eq!(gw["label"], "BC:FC:E7:94:6B:90");
    }

    /// What a saved map is: text somebody can read, with every machine in it
    /// and both ends of every link.
    ///
    /// Written as the module writes it rather than as this test imagines it,
    /// so the shape asserted here is the shape a reader gets.
    #[test]
    fn a_saved_map_is_readable_text_with_everything_in_it() {
        let nodes = vec![
            me(&["42:40:62:FC:A8:DA"]),
            seen(
                "192.168.0.1",
                "BC:FC:E7:94:6B:90",
                DeviceType::Router,
                "wlp2s0",
                ("192.168.0.126", "42:40:62:FC:A8:DA"),
            ),
        ];
        let doc = json!({ "saved": stamp(1_700_000_000), "machines": nodes });
        let text = serde_json::to_string_pretty(&doc).expect("it serialises");

        // It reads back as what it is.
        let back: Value = serde_json::from_str(&text).expect("valid json");
        assert_eq!(back["machines"].as_array().expect("machines").len(), 2);
        assert!(
            back["saved"].as_str().expect("when").starts_with("2023-11-"),
            "when it was written: {}",
            back["saved"]
        );

        // This machine, with its cards and no address of its own.
        let mine = &back["machines"][0];
        assert_eq!(mine["hostname"], "PC-HOME");
        assert_eq!(mine["macs"][0], "42:40:62:FC:A8:DA");

        // And a neighbour with both ends of its link, so the file says what
        // was reached from where rather than a list of addresses.
        let other = &back["machines"][1];
        assert_eq!(other["ips"][0], "192.168.0.1");
        assert_eq!(other["local_ips"][0], "192.168.0.126");
        assert_eq!(other["local_mac"], "42:40:62:FC:A8:DA");
        assert_eq!(other["iface"], "wlp2s0");
        assert_eq!(other["device_type"], "Router");
    }

    /// The map as the bug report had it: this machine on wifi, this machine on
    /// a wired card, each network's own gateway, a libvirt guest nobody is on
    /// any more, and a phone that was tethered last week.
    fn a_real_map() -> Vec<Node> {
        vec![
            node("192.168.0.126", "42:40:62:FC:A8:DA", DeviceType::This, Some("wlp2s0")),
            node("192.168.10.5", "20:7B:D5:1A:3E:04", DeviceType::This, Some("enp198s0")),
            node("192.168.0.1", "BC:FC:E7:94:6B:90", DeviceType::Router, Some("wlp2s0")),
            node("192.168.10.1", "00:E2:59:00:FD:D0", DeviceType::Router, Some("enp198s0")),
            // Kept by an older build, so no card was recorded.
            node("10.218.99.236", "D6:D3:31:41:5C:C5", DeviceType::Router, None),
            node("10.218.99.236", "0E:08:48:F0:75:28", DeviceType::PC, None),
            node("192.168.122.182", "52:54:00:7E:4F:B6", DeviceType::PC, None),
        ]
    }

    fn hub(ip: &str) -> Option<String> {
        let map = a_real_map();
        let n = map.iter().find(|n| n.ip() == ip).expect("a node");
        hub_for(n, &map)
    }

    /// Each machine hangs from the gateway of *its own* network.
    ///
    /// The bug this is written for: the map took whichever router came first
    /// out of the database and hung everything off it, so a laptop on wifi was
    /// drawn joined to a phone it had been tethered to a week earlier — a line
    /// that was never there, on a network that is not.
    #[test]
    fn a_machine_hangs_from_its_own_gateway() {
        assert_eq!(hub("192.168.0.126").as_deref(), Some("BC:FC:E7:94:6B:90"));
        assert_eq!(hub("192.168.10.5").as_deref(), Some("00:E2:59:00:FD:D0"));
    }

    /// A gateway is not hung from itself, and two gateways on two networks are
    /// not joined to each other.
    #[test]
    fn a_gateway_is_the_end_of_its_own_line() {
        assert_eq!(hub("192.168.0.1"), None, "nothing above it on this map");
        assert_eq!(hub("192.168.10.1"), None);
    }

    /// Rows kept before the card was recorded still find their own network, by
    /// the only thing left to go on — the address.
    #[test]
    fn an_older_row_is_placed_by_its_address() {
        // The phone's own gateway is the other 10.218.99 entry, not this
        // machine's wifi router.
        let map = a_real_map();
        let phone = map
            .iter()
            .find(|n| n.mac == "0E:08:48:F0:75:28")
            .expect("the phone");
        assert_eq!(hub_for(phone, &map).as_deref(), Some("D6:D3:31:41:5C:C5"));
    }

    /// And a machine on a network nothing else on the map belongs to stands
    /// alone, rather than being drawn onto somebody else's wire.
    #[test]
    fn a_machine_with_no_network_here_stands_alone() {
        assert_eq!(hub("192.168.122.182"), None);
    }

    /// Two cards of this machine are two networks, and neither is joined to
    /// the other's.
    #[test]
    fn this_machines_two_cards_are_on_two_networks() {
        assert_ne!(hub("192.168.0.126"), hub("192.168.10.5"));
    }

    /// With no gateway at all, the neighbours hang from the card that saw
    /// them: everything in a neighbour table was seen *by* something.
    #[test]
    fn without_a_gateway_the_card_that_saw_them_is_the_centre() {
        let map = vec![
            node("192.168.5.2", "AA:AA:AA:AA:AA:01", DeviceType::This, Some("eth0")),
            node("192.168.5.7", "AA:AA:AA:AA:AA:02", DeviceType::PC, Some("eth0")),
        ];
        assert_eq!(
            hub_for(&map[1], &map).as_deref(),
            Some("AA:AA:AA:AA:AA:01")
        );
        assert_eq!(hub_for(&map[0], &map), None, "and it is the centre");
    }
}

#[cfg(test)]
mod words {
    /// Every `a.b` key a locale file defines, read from the file rather than
    /// through the catalogue: `tr` falls back to English for a key Ukrainian
    /// is missing, so asking it would hide exactly what this looks for.
    fn keys(src: &str) -> Vec<String> {
        let mut table = String::new();
        let mut out = Vec::new();
        for line in src.lines() {
            let line = line.trim();
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                table = name.to_string();
            } else if let Some((key, _)) = line.split_once(" = ") {
                out.push(format!("{table}.{key}"));
            }
        }
        out
    }

    #[test]
    fn both_languages_say_the_same_things() {
        let en = keys(include_str!("../locales/en.toml"));
        let uk = keys(include_str!("../locales/uk.toml"));
        assert!(en.len() > 30, "the catalogue is suspiciously small");
        for key in &en {
            assert!(uk.contains(key), "uk.toml is missing {key}");
        }
        for key in uk.iter().filter(|k| !k.starts_with("module.")) {
            assert!(en.contains(key), "en.toml is missing {key}");
        }
    }

    /// A screen asked for in Ukrainian comes back in Ukrainian.
    #[test]
    fn the_screens_are_translated() {
        let home = super::main_view("uk", &crate::model::Stats::default(), &[], None, None)
            .to_string();
        for word in [
            "Знайти вузли",
            "Прочитати таблицю сусідів",
            "Показати мапу",
            "Зберегти мапу…",
            "Забути мапу",
        ] {
            assert!(home.contains(word), "{word} is missing from the screen");
        }
        assert!(!home.contains("Find hosts"), "English survived: {home}");
    }
}
