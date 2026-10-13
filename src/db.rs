use crate::model::{DeviceType, Node, Stats};
use rusqlite::{params, Connection, Result};
use std::path::Path;

/// A stored list, as the columns keep them: comma separated, blanks dropped.
fn split(text: Option<String>) -> Vec<String> {
    text.unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

fn get_db_path(module_dir: &str) -> std::path::PathBuf {
    Path::new(module_dir).join("topology.db")
}

fn init_db(conn: &Connection) -> Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS nodes (
            mac TEXT PRIMARY KEY,
            ip TEXT NOT NULL,
            hostname TEXT,
            device_type TEXT NOT NULL,
            parent_mac TEXT,
            iface TEXT,
            local_ip TEXT,
            local_mac TEXT,
            last_seen INTEGER,
            macs TEXT
        )",
        [],
    )?;
    migrate(conn)
}

/// Bring a database written by an older build up to date.
///
/// A column at a time, because that is how this schema grows and because
/// SQLite will not add one that is already there. The alternative — dropping
/// the table and starting again — would throw away every machine the user has
/// scanned, which is the one thing in here they cannot get back by pressing a
/// button.
fn migrate(conn: &Connection) -> Result<()> {
    let have: Vec<String> = conn
        .prepare("PRAGMA table_info(nodes)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .filter_map(|c| c.ok())
        .collect();
    for (column, kind) in [
        ("iface", "TEXT"),
        ("local_ip", "TEXT"),
        ("local_mac", "TEXT"),
        ("macs", "TEXT"),
        ("last_seen", "INTEGER"),
    ] {
        if !have.iter().any(|c| c == column) {
            conn.execute(&format!("ALTER TABLE nodes ADD COLUMN {column} {kind}"), [])?;
        }
    }
    Ok(())
}

pub fn upsert_nodes(module_dir: &str, nodes: &[Node]) -> Result<()> {
    let conn = Connection::open(get_db_path(module_dir))?;
    init_db(&conn)?;

    let mut stmt = conn.prepare(
        "INSERT INTO nodes (mac, ip, hostname, device_type, parent_mac, iface, local_ip,
                            local_mac, last_seen, macs)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(mac) DO UPDATE SET
            ip = excluded.ip,
            hostname = COALESCE(excluded.hostname, nodes.hostname),
            -- What a thing is may be learnt later, but this machine knowing
            -- itself outranks anything a scan guessed about the same card.
            device_type = CASE
                WHEN excluded.device_type = 'self' THEN excluded.device_type
                WHEN nodes.device_type = 'unknown' THEN excluded.device_type
                ELSE nodes.device_type END,
            parent_mac = COALESCE(excluded.parent_mac, nodes.parent_mac),
            iface = COALESCE(excluded.iface, nodes.iface),
            local_ip = COALESCE(excluded.local_ip, nodes.local_ip),
            local_mac = COALESCE(excluded.local_mac, nodes.local_mac),
            macs = COALESCE(excluded.macs, nodes.macs),
            last_seen = COALESCE(excluded.last_seen, nodes.last_seen)",
    )?;

    for node in nodes {
        stmt.execute(params![
            node.mac,
            // Joined, because one card can answer to several and they are one
            // machine: the column holds the list the way the screen shows it.
            node.ips.join(","),
            node.hostname,
            node.device_type.as_str(),
            node.parent_mac,
            node.iface,
            node.local_ips.join(","),
            node.local_mac,
            node.last_seen,
            node.macs.join(",")
        ])?;
    }
    Ok(())
}

pub fn get_stats_from(module_dir: &str) -> Result<Stats> {
    let conn = Connection::open(get_db_path(module_dir))?;
    let total: usize = conn.query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get(0)).unwrap_or(0);
    let routers: usize = conn.query_row("SELECT COUNT(*) FROM nodes WHERE device_type = 'router'", [], |r| r.get(0)).unwrap_or(0);
    let servers: usize = conn.query_row("SELECT COUNT(*) FROM nodes WHERE device_type = 'server'", [], |r| r.get(0)).unwrap_or(0);
    
    Ok(Stats { total, routers, servers })
}

/// Forget everything that has been seen.
///
/// The rows, not the file: the file is this module's own and keeping it means
/// the next scan writes into a database that is already the right shape. What
/// goes is what was learnt, which is the thing somebody is asking to be rid
/// of — a map of a network they were on, on a machine that may be carried
/// somewhere else.
///
/// Returns how many machines were forgotten, so the screen can say.
pub fn clear(module_dir: &str) -> Result<usize> {
    let conn = Connection::open(get_db_path(module_dir))?;
    init_db(&conn)?;
    let had: usize = conn
        .query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get(0))
        .unwrap_or(0);
    conn.execute("DELETE FROM nodes", [])?;
    // The space back as well: a map of a network is the sort of thing worth
    // being actually gone rather than marked deleted inside a file.
    let _ = conn.execute("VACUUM", []);
    Ok(had)
}

pub fn get_all_nodes(module_dir: &str) -> Result<Vec<Node>> {
    let conn = Connection::open(get_db_path(module_dir))?;
    init_db(&conn)?;
    let mut stmt =
        conn.prepare(
            "SELECT mac, ip, hostname, device_type, parent_mac, iface, local_ip, local_mac,
                    last_seen, macs
             FROM nodes",
        )?;
    
    let node_iter = stmt.query_map([], |row| {
        let dtype_str: String = row.get(3)?;
        let device_type = match dtype_str.as_str() {
            "server" => DeviceType::Server,
            "router" => DeviceType::Router,
            "switch" => DeviceType::Switch,
            "pc" => DeviceType::PC,
            "self" => DeviceType::This,
            _ => DeviceType::Unknown,
        };

        Ok(Node {
            mac: row.get(0)?,
            ips: split(row.get::<_, Option<String>>(1)?),
            hostname: row.get(2)?,
            device_type,
            parent_mac: row.get(4)?,
            macs: split(row.get::<_, Option<String>>(9)?),
            iface: row.get(5)?,
            local_ips: split(row.get::<_, Option<String>>(6)?),
            local_mac: row.get(7)?,
            last_seen: row.get(8)?,
        })
    })?;

    let mut nodes = Vec::new();
    for n in node_iter {
        nodes.push(n?);
    }
    Ok(nodes)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::DeviceType;

    fn somewhere(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("limen-netmap-db-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a place for the database");
        dir.display().to_string()
    }

    fn node(mac: &str, ip: &str, kind: DeviceType, iface: Option<&str>) -> Node {
        Node {
            mac: mac.into(),
            ips: ip.split(',').map(str::to_string).collect(),
            hostname: Some("the-machine".into()),
            device_type: kind,
            parent_mac: None,
            macs: Vec::new(),
            iface: iface.map(str::to_string),
            local_ips: Vec::new(),
            local_mac: None,
            last_seen: None,
        }
    }

    /// This machine goes in with its name and the card it was seen on, and
    /// comes back as itself.
    #[test]
    fn this_machine_is_kept_with_its_name_and_its_card() {
        let dir = somewhere("self");
        upsert_nodes(
            &dir,
            &[node("AA:BB:CC:DD:EE:01", "192.168.0.126", DeviceType::This, Some("wlp2s0"))],
        )
        .expect("written");

        let back = get_all_nodes(&dir).expect("read");
        assert_eq!(back.len(), 1);
        assert!(matches!(back[0].device_type, DeviceType::This), "{back:?}");
        assert_eq!(back[0].hostname.as_deref(), Some("the-machine"));
        assert_eq!(back[0].iface.as_deref(), Some("wlp2s0"));

        // The address moves with the network; the card does not. A second
        // scan on another network updates it rather than adding a stranger.
        upsert_nodes(
            &dir,
            &[node("AA:BB:CC:DD:EE:01", "10.0.0.8", DeviceType::This, Some("wlp2s0"))],
        )
        .expect("written again");
        let back = get_all_nodes(&dir).expect("read");
        assert_eq!(back.len(), 1, "one card, one row");
        assert_eq!(back[0].ips, ["10.0.0.8"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A scan that finds this machine where it had previously found a
    /// stranger's card says so: knowing yourself outranks a guess.
    #[test]
    fn knowing_this_machine_overrides_what_a_scan_guessed() {
        let dir = somewhere("override");
        upsert_nodes(&dir, &[node("AA:BB:CC:DD:EE:02", "192.168.0.126", DeviceType::PC, None)])
            .expect("written");
        upsert_nodes(
            &dir,
            &[node("AA:BB:CC:DD:EE:02", "192.168.0.126", DeviceType::This, Some("eth0"))],
        )
        .expect("written");
        let back = get_all_nodes(&dir).expect("read");
        assert!(matches!(back[0].device_type, DeviceType::This), "{back:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Forgetting the map leaves nothing behind, and the database still
    /// works afterwards: it is the rows that go, not the file.
    #[test]
    fn forgetting_the_map_empties_it_and_leaves_it_usable() {
        let dir = somewhere("clear");
        upsert_nodes(
            &dir,
            &[
                node("AA:BB:CC:DD:EE:05", "192.168.0.1", DeviceType::Router, None),
                node("AA:BB:CC:DD:EE:06", "192.168.0.9", DeviceType::PC, Some("eth0")),
            ],
        )
        .expect("written");
        assert_eq!(get_all_nodes(&dir).expect("read").len(), 2);

        let had = clear(&dir).expect("cleared");
        assert_eq!(had, 2, "it says how many it forgot");
        assert!(get_all_nodes(&dir).expect("read").is_empty());

        // Nothing is left to find in the file itself — a map of a network is
        // worth being actually gone.
        let bytes = std::fs::read(get_db_path(&dir)).expect("the file is still there");
        let raw = String::from_utf8_lossy(&bytes);
        assert!(!raw.contains("192.168.0.9"), "an address survived the file");

        // And the next scan writes into it as before.
        upsert_nodes(
            &dir,
            &[node("AA:BB:CC:DD:EE:07", "10.0.0.1", DeviceType::Router, None)],
        )
        .expect("written again");
        assert_eq!(get_all_nodes(&dir).expect("read").len(), 1);

        // Forgetting an empty map is not an error.
        assert_eq!(clear(&dir).expect("cleared"), 1);
        assert_eq!(clear(&dir).expect("cleared"), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A database written by an older build is brought up to date rather than
    /// thrown away: what is in it is every machine the user has scanned, and
    /// that is the one thing they cannot get back by pressing a button.
    #[test]
    fn an_older_database_keeps_everything_it_had() {
        let dir = somewhere("migrate");
        {
            // The schema as it was before the interface column existed.
            let conn = Connection::open(get_db_path(&dir)).expect("a database");
            conn.execute(
                "CREATE TABLE nodes (
                    mac TEXT PRIMARY KEY,
                    ip TEXT NOT NULL,
                    hostname TEXT,
                    device_type TEXT NOT NULL,
                    parent_mac TEXT
                )",
                [],
            )
            .expect("the old table");
            conn.execute(
                "INSERT INTO nodes VALUES ('AA:BB:CC:DD:EE:03', '192.168.0.1', 'router', 'router', NULL)",
                [],
            )
            .expect("a row from before");
        }

        // Read through the new code, which migrates on the way.
        let back = get_all_nodes(&dir).expect("read");
        assert_eq!(back.len(), 1, "the old row survived");
        assert_eq!(back[0].ips, ["192.168.0.1"]);
        assert_eq!(back[0].iface, None, "it never had one");

        // And the new column works from then on.
        upsert_nodes(
            &dir,
            &[node("AA:BB:CC:DD:EE:04", "192.168.0.126", DeviceType::This, Some("wlp2s0"))],
        )
        .expect("written");
        let back = get_all_nodes(&dir).expect("read");
        assert_eq!(back.len(), 2);
        assert_eq!(
            back.iter().find(|n| n.mac.ends_with("04")).unwrap().iface.as_deref(),
            Some("wlp2s0")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
