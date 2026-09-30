use crate::model::{Node, Stats};
use crate::*;

pub(crate) fn main_view(lang: &str, stats: &Stats) -> Value {
    let t = |k: &str| catalog().tr(lang, k);
    
    let total_str = t("ui.stats_total").replace("{n}", &stats.total.to_string());
    let detail_str = t("ui.stats_routers")
        .replace("{r}", &stats.routers.to_string())
        .replace("{s}", &stats.servers.to_string());

    window(
        t("ui.title"),
        vec![
            label(t("ui.subtitle")).weak(),
            separator(),
            label(total_str).strong(),
            label(detail_str),
            separator(),
            row(vec![
                button(t("ui.scan"), CAP, "scan").primary(),
                button(t("ui.map"), CAP, "topology"),
            ])
        ],
    )
}

pub(crate) fn scanning_view(lang: &str) -> Value {
    let t = |k: &str| catalog().tr(lang, k);
    let mut v = window_modal_sized(
        t("scan.title"),
        "net-mapper.scanning",
        520.0,
        vec![
            step(t("scan.in_progress"), "loading"), //[cite: 3]
            label(t("scan.hint")).weak(),
        ],
    );
    // Авто-опрос интерфейсом Limen[cite: 3, 8]
    if let Value::Object(m) = &mut v {
        m.insert(
            "auto".into(),
            json!({ "capability": CAP, "method": "scan_poll", "args": {} }),
        );
    }
    v
}
/// The map itself, drawn by the host.
///
/// In the app rather than in an exported page: the page pulls its graph library
/// off a CDN, and this is a tool for a machine on somebody else's network —
/// often one with no route out of it, where that page opens empty.
pub(crate) fn topology_view(lang: &str, nodes: &[Node]) -> Value {
    let t = |k: &str| catalog().tr(lang, k);
    if nodes.is_empty() {
        return window(
            t("map.title"),
            vec![label(t("map.empty")).weak(), separator(),
                 button(t("ui.scan"), CAP, "scan").primary()],
        );
    }

    // The gateway is what everything else hangs from; it is also the node the
    // collector already identified, so the map agrees with the table.
    let gateway = nodes
        .iter()
        .find(|n| matches!(n.device_type, model::DeviceType::Router))
        .map(|n| n.mac.clone());

    // Under the circle goes the least that identifies it; the rest is on the
    // card the app draws when the pointer is on it, so a crowded map stays
    // readable without losing anything.
    let circles: Vec<limen_sdk_rust::ui::DiagramNode> = nodes
        .iter()
        .map(|n| {
            // With no name to show, the address becomes the name and the
            // hardware address goes under it — saying the address twice tells
            // nobody anything.
            let (name, under) = match &n.hostname {
                Some(host) => (host.clone(), n.ip.clone()),
                None => (n.ip.clone(), n.mac.clone()),
            };
            let mut node = limen_sdk_rust::ui::node(n.mac.clone(), name)
                .kind(n.device_type.as_str())
                .detail(under)
                .info(t("field.th_ip"), n.ip.clone())
                .info(t("field.th_mac"), n.mac.clone())
                .info(t("field.th_kind"), t(&format!("field.key_{}", n.device_type.as_str())));
            if let Some(host) = &n.hostname {
                node = node.info(t("field.th_host"), host.clone());
            }
            if let Some(parent) = &n.parent_mac {
                node = node.info(t("field.behind"), parent.clone());
            }
            node
        })
        .collect();

    let mut lines: Vec<(String, String, bool)> = Vec::new();
    for n in nodes {
        if let Some(parent) = &n.parent_mac {
            // Dashed: a link that was inferred rather than observed.
            lines.push((n.mac.clone(), parent.clone(), true));
        } else if let Some(gw) = gateway.as_ref().filter(|gw| **gw != n.mac) {
            lines.push((n.mac.clone(), gw.clone(), false));
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

/// One host, in a tab of its own: everything the scan learned about it.
///
/// The hover card says as much as fits beside a circle. This is the same facts
/// with room to read them, somewhere that stays open while the map is used.
pub(crate) fn host_view(lang: &str, node: Option<&Node>) -> Value {
    let t = |k: &str| catalog().tr(lang, k);
    let Some(n) = node else {
        return window(t("host.unknown_title"), vec![label(t("host.unknown")).weak()]);
    };
    let name = n.hostname.clone().unwrap_or_else(|| n.ip.clone());
    let mut rows = vec![
        vec![t("field.th_ip"), n.ip.clone()],
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
    window(
        name,
        vec![
            limen_sdk_rust::ui::table(vec![t("host.field"), t("host.value")], rows),
            separator(),
            label(t("host.what")).weak(),
        ],
    )
}
