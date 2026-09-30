use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub enum DeviceType {
    PC,
    Server,
    Router,
    Switch,
    Unknown,
}

impl DeviceType {
    pub fn as_str(&self) -> &'static str {
        match self {
            DeviceType::PC => "pc",
            DeviceType::Server => "server",
            DeviceType::Router => "router",
            DeviceType::Switch => "switch",
            DeviceType::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Node {
    pub mac: String,
    pub ip: String,
    pub hostname: Option<String>,
    pub device_type: DeviceType,
    pub parent_mac: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Stats {
    pub total: usize,
    pub routers: usize,
    pub servers: usize,
}