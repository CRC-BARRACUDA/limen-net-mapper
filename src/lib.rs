mod collector;
mod db;
mod handler;
mod model;
mod view;

pub(crate) use handler::*;
pub(crate) use limen_sdk_rust::ui::{button, label, row, separator, step, window, window_modal_sized};
pub(crate) use limen_sdk_rust::{json, rpc, Catalog, Handler, Host, RpcError, Value};
use limen_sdk_rust::export_module;

pub(crate) const CAP: &str = "network.map";

pub(crate) fn catalog() -> &'static Catalog {
    static C: std::sync::OnceLock<Catalog> = std::sync::OnceLock::new();
    C.get_or_init(|| {
        Catalog::new(&[
            ("en", include_str!("../locales/en.toml")),
            ("uk", include_str!("../locales/uk.toml")),
        ])
    })
}

export_module!(NetMapper);