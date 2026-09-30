use crate::model::{DeviceType, Node, Stats};
use rusqlite::{params, Connection, Result};
use std::path::Path;

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
            parent_mac TEXT
        )",
        [],
    )?;
    Ok(())
}

pub fn upsert_nodes(module_dir: &str, nodes: &[Node]) -> Result<()> {
    let conn = Connection::open(get_db_path(module_dir))?;
    init_db(&conn)?;

    let mut stmt = conn.prepare(
        "INSERT INTO nodes (mac, ip, hostname, device_type, parent_mac) 
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(mac) DO UPDATE SET 
            ip = excluded.ip,
            hostname = COALESCE(excluded.hostname, nodes.hostname),
            device_type = CASE WHEN nodes.device_type = 'unknown' THEN excluded.device_type ELSE nodes.device_type END,
            parent_mac = COALESCE(excluded.parent_mac, nodes.parent_mac)",
    )?;

    for node in nodes {
        stmt.execute(params![
            node.mac,
            node.ip,
            node.hostname,
            node.device_type.as_str(),
            node.parent_mac
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

pub fn get_all_nodes(module_dir: &str) -> Result<Vec<Node>> {
    let conn = Connection::open(get_db_path(module_dir))?;
    let mut stmt = conn.prepare("SELECT mac, ip, hostname, device_type, parent_mac FROM nodes")?;
    
    let node_iter = stmt.query_map([], |row| {
        let dtype_str: String = row.get(3)?;
        let device_type = match dtype_str.as_str() {
            "server" => DeviceType::Server,
            "router" => DeviceType::Router,
            "switch" => DeviceType::Switch,
            "pc" => DeviceType::PC,
            _ => DeviceType::Unknown,
        };

        Ok(Node {
            mac: row.get(0)?,
            ip: row.get(1)?,
            hostname: row.get(2)?,
            device_type,
            parent_mac: row.get(4)?,
        })
    })?;

    let mut nodes = Vec::new();
    for n in node_iter {
        nodes.push(n?);
    }
    Ok(nodes)
}