use crate::*;
use crate::collector;
use crate::db;

use std::sync::mpsc::{channel, Receiver};
use std::thread;

#[derive(Default)]
pub struct NetMapper {
    scan_receiver: Option<Receiver<(Vec<model::Node>, Option<crate::discover::Swept>)>>,
    /// Whether the collection in flight is knocking on doors as well as
    /// reading, so the screen can say which of the two it is doing.
    sweeping: bool,
    /// What the last sweep came to, said once on the screen after it.
    swept: Option<crate::discover::Swept>,
    /// One line about what just happened — a map written out, a map thrown
    /// away. Shown once, on the screen that follows.
    note: Option<String>,
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
            "ui" => Ok(self.home(host, lang)),
            
            // Read what the kernel already knows. Sends nothing.
            "scan" => Ok(self.start(lang, false)),

            // Ask every address on every attached network, then read what
            // answered. This one sends — see `discover`.
            "discover" => Ok(self.start(lang, true)),
            
            "scan_poll" => {
                if let Some(rx) = &self.scan_receiver {
                    match rx.try_recv() {
                        Ok((nodes, swept)) => {
                            self.scan_receiver = None;
                            self.swept = swept;
                            if let Some(dir) = host.module_dir() {
                                db::upsert_nodes(&dir, &nodes).ok();
                            }
                        }
                        // Still working.
                        Err(std::sync::mpsc::TryRecvError::Empty) => {
                            return Ok(view::scanning_view(lang, self.sweeping))
                        }
                        // The thread holding the other end is gone without an
                        // answer: the scan died rather than finished, and
                        // waiting on a sender that no longer exists is waiting
                        // for ever.
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            self.scan_receiver = None;
                        }
                    }
                }
                Ok(self.home(host, lang))
            }
            
            // Carry the map off this machine, or be rid of it.
            "save_db" => Ok(self.save_db(host, lang)),
            "clear_db" => Ok(self.clear_db(host, lang)),

            // The map, drawn in the app.
            "topology" => Ok(view::topology_view(lang, &everyone(host))),

            // One host, opened by double-clicking its circle on the map. The id
            // is the hardware address, which is what the map names its nodes by.
            "host" => {
                let id = params.get("id").and_then(Value::as_str).unwrap_or_default();
                let nodes = everyone(host);
                Ok(view::host_view(lang, nodes.iter().find(|n| n.mac == id)))
            }

            other => Err(RpcError::new(rpc::METHOD_NOT_FOUND, format!("No method {other}"))),
        }
    }
}

impl NetMapper {
    /// The front screen: what is known, and this machine's own networks.
    fn home(&mut self, host: &Host, lang: &str) -> Value {
        view::main_view(
            lang,
            &stats(host),
            &collector::interfaces(),
            self.swept.take(),
            self.note.take(),
        )
    }

    /// Write everything that has been seen to a file somebody chooses.
    ///
    /// As text, not as the database: a `.db` is this module's own business and
    /// opens in nothing, while what somebody wants to carry off a machine is
    /// the list of what was on the network — into a report, a ticket, another
    /// tool.
    fn save_db(&mut self, host: &Host, lang: &str) -> Value {
        let nodes = everyone(host);
        if nodes.is_empty() {
            return self.say(host, lang, "db.empty", &[]);
        }
        let Some(path) = host.save_file("network-map.json") else {
            return self.say(host, lang, "db.save_cancelled", &[]);
        };
        let doc = json!({
            "saved": crate::view::stamp(collector::now()),
            "machines": nodes,
        });
        let text = serde_json::to_string_pretty(&doc).unwrap_or_default();
        match std::fs::write(&path, text) {
            Ok(()) => self.say(
                host,
                lang,
                "db.saved",
                &[("n", &nodes.len().to_string()), ("path", &path)],
            ),
            Err(e) => self.say(host, lang, "db.failed", &[("why", &e.to_string())]),
        }
    }

    /// Forget every machine this module has seen.
    fn clear_db(&mut self, host: &Host, lang: &str) -> Value {
        let Some(dir) = host.module_dir() else {
            return self.say(host, lang, "db.nowhere", &[]);
        };
        match db::clear(&dir) {
            Ok(had) => self.say(host, lang, "db.cleared", &[("n", &had.to_string())]),
            Err(e) => self.say(host, lang, "db.failed", &[("why", &e.to_string())]),
        }
    }

    /// The screen again, with one line saying what just happened.
    fn say(&mut self, host: &Host, lang: &str, key: &str, fill: &[(&str, &str)]) -> Value {
        let mut text = catalog().tr(lang, key);
        for (slot, value) in fill {
            text = text.replace(&format!("{{{slot}}}"), value);
        }
        self.note = Some(text);
        self.home(host, lang)
    }

    /// Start a collection, optionally knocking on every address first.
    fn start(&mut self, lang: &str, discover: bool) -> Value {
        let (tx, rx) = channel();
        self.scan_receiver = Some(rx);
        self.sweeping = discover;
        self.swept = None;
        thread::spawn(move || {
            // Asking first, so that what the neighbour table is read for
            // includes whoever answers. The answers arrive in the kernel's
            // table, not back here — see `discover`.
            let swept = discover.then(|| {
                let nets: Vec<(u32, u32)> = collector::interfaces()
                    .into_iter()
                    .flat_map(|i| i.nets)
                    .collect();
                crate::discover::sweep(&nets)
            });
            // This machine goes in with the neighbours: the scan is what the
            // user pressed, and what it learned — including this machine's
            // name — is what gets kept.
            let mut nodes: Vec<model::Node> = collector::this_machine().into_iter().collect();
            nodes.extend(collector::run_deep_scan());
            let _ = tx.send((nodes, swept));
        });
        view::scanning_view(lang, discover)
    }
}

/// Everyone on the map: what the scans have found, and this machine.
///
/// This machine is not in the database and never will be — it is worked out
/// afresh here, because which address it has is a fact about the network it is
/// plugged into right now. If a scan has recorded something under the same
/// hardware address, the live answer replaces it.
fn everyone(host: &Host) -> Vec<model::Node> {
    let mut nodes = host
        .module_dir()
        .and_then(|dir| db::get_all_nodes(&dir).ok())
        .unwrap_or_default();
    // This machine is never one of its own neighbours, and its own cards'
    // hardware addresses are not other machines either — a stored row for one
    // of them is this machine seen from a build that drew it card by card.
    let me = collector::this_machine();
    if let Some(me) = me {
        let mine: Vec<&String> = me.macs.iter().collect();
        nodes.retain(|n| n.mac != me.mac && !mine.contains(&&n.mac));
        // First, so the map is laid out around it.
        nodes.insert(0, me);
    }
    nodes
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
