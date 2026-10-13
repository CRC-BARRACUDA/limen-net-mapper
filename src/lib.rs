mod collector;
mod db;
mod discover;
mod handler;
mod model;
mod view;

pub(crate) use handler::*;
pub(crate) use limen_sdk_rust::ui::{button, label, row, separator, step, window, window_modal_sized};
pub(crate) use limen_sdk_rust::{json, rpc, Catalog, Handler, Host, RpcError, Value};
use limen_sdk_rust::export_module;

/// The id this machine goes by on its own map: one node, whatever it is
/// called and however many cards it has.
///
/// Linux and Windows both put this machine on the map; macOS has nothing
/// [`collector::this_machine`] can read from yet, so on that build nothing
/// outside the tests names it. That is the arrangement, not an oversight, so
/// it is marked the way `collector` marks the parsers each platform does not
/// call.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub(crate) const SELF: &str = "self";

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