use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub enum DeviceType {
    PC,
    Server,
    Router,
    Switch,
    /// The machine doing the looking.
    This,
    Unknown,
}

impl DeviceType {
    pub fn as_str(&self) -> &'static str {
        match self {
            DeviceType::PC => "pc",
            DeviceType::Server => "server",
            DeviceType::Router => "router",
            DeviceType::Switch => "switch",
            // The word the toolkit draws in its own colour.
            DeviceType::This => "self",
            DeviceType::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Node {
    /// What the network calls it. For a machine with several cards this is
    /// the one it is keyed by; the rest are in `macs`.
    pub mac: String,
    /// Every address it answers to, in the order they were seen.
    ///
    /// A list, because one card can hold several: a machine renumbered by DHCP
    /// while the old lease is still in the table, an interface with an alias,
    /// a server answering on two. They are one machine and they are drawn as
    /// one, saying all of them.
    pub ips: Vec<String>,
    pub hostname: Option<String>,
    pub device_type: DeviceType,
    pub parent_mac: Option<String>,
    /// Every hardware address this machine has, when the node *is* this
    /// machine: one node, however many cards.
    pub macs: Vec<String>,
    /// The interface it was seen on — the card a neighbour answered to.
    pub iface: Option<String>,
    /// This machine's own addresses on the link this one was seen over.
    ///
    /// An address belongs to a connection, not to a machine: this one has as
    /// many as it has cards, and which of them applies depends on who is being
    /// talked to. So they are kept with the neighbour they apply to, and the
    /// map writes them at this end of the line.
    pub local_ips: Vec<String>,
    /// And the card that answers there.
    pub local_mac: Option<String>,
    /// When it was last answered for, in seconds since the epoch.
    pub last_seen: Option<i64>,
}

impl Node {
    /// The address it is best known by — the first it answered to.
    pub fn ip(&self) -> &str {
        self.ips.first().map(String::as_str).unwrap_or_default()
    }
}

#[derive(Debug, Clone, Default)]
pub struct Stats {
    pub total: usize,
    pub routers: usize,
    pub servers: usize,
}