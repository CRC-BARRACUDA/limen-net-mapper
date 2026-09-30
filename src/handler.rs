use crate::*;
use crate::collector;
use crate::db;

use std::sync::mpsc::{channel, Receiver};
use std::thread;

#[derive(Default)]
pub struct NetMapper {
    scan_receiver: Option<Receiver<Vec<model::Node>>>,
}

impl Handler for NetMapper {
    fn capabilities(&self) -> Vec<String> {
        vec![CAP.into()]
    }

    fn invoke(
        &mut self,
        _capability: &str,
        method: &str,
        params: Value,
        host: &Host,
    ) -> Result<Value, RpcError> {
        let lang = host.locale();
        let lang = lang.as_str();

        match method {
            "ui" => Ok(view::main_view(lang, &stats(host))),
            
            "scan" => {
                let (tx, rx) = channel();
                self.scan_receiver = Some(rx);
                
                // Запуск тяжелого сбора в фоне
                thread::spawn(move || {
                    let nodes = collector::run_deep_scan();
                    let _ = tx.send(nodes);
                });
                
                Ok(view::scanning_view(lang))
            }
            
            "scan_poll" => {
                if let Some(rx) = &self.scan_receiver {
                    if let Ok(nodes) = rx.try_recv() {
                        self.scan_receiver = None;
                        if let Some(dir) = host.module_dir() {
                            db::upsert_nodes(&dir, &nodes).ok();
                        }
                        return Ok(view::main_view(lang, &stats(host)));
                    }
                    // Еще работаем — возвращаем окно загрузки[cite: 3]
                    return Ok(view::scanning_view(lang));
                }
                Ok(view::main_view(lang, &stats(host)))
            }
            
            // The map, drawn in the app.
            "topology" => {
                let nodes = host
                    .module_dir()
                    .and_then(|dir| db::get_all_nodes(&dir).ok())
                    .unwrap_or_default();
                Ok(view::topology_view(lang, &nodes))
            }

            // One host, opened by double-clicking its circle on the map. The id
            // is the hardware address, which is what the map names its nodes by.
            "host" => {
                let id = params.get("id").and_then(Value::as_str).unwrap_or_default();
                let nodes = host
                    .module_dir()
                    .and_then(|dir| db::get_all_nodes(&dir).ok())
                    .unwrap_or_default();
                Ok(view::host_view(lang, nodes.iter().find(|n| n.mac == id)))
            }

            other => Err(RpcError::new(rpc::METHOD_NOT_FOUND, format!("No method {other}"))),
        }
    }
}

/// What the database holds, read when it is about to be shown.
///
/// Not cached in the handler: the counts were read once at start-up, from a
/// function that returned zeros without opening the file, so every restart
/// showed an empty database that was not empty.
fn stats(host: &Host) -> model::Stats {
    host.module_dir()
        .and_then(|dir| db::get_stats_from(&dir).ok())
        .unwrap_or_default()
}
