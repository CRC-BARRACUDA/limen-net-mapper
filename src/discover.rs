//! Finding the machines that have not spoken to this one yet.
//!
//! The rest of this module reads what the kernel already knows: the neighbour
//! table holds the machines this one has exchanged packets with, and a machine
//! that has been quiet is simply not in it. That is a map of who this computer
//! talks to, not a map of what is on the network.
//!
//! This is the other half, and it is a different kind of act: it **sends**.
//! One datagram to every address on the networks this machine is attached to,
//! which makes the kernel ask "who has this address?" out loud — and the
//! machines that answer end up in the neighbour table, where the rest of the
//! module can read them like any other neighbour.
//!
//! A datagram to a port nothing is listening on, because the point is never
//! the datagram: it is the question the kernel has to ask before it can send
//! one. Nothing is connected to, nothing is logged in to, and no port is
//! examined. What this is, plainly: **this machine announcing itself to every
//! address on the network**, which on a network being investigated is a thing
//! to do deliberately and not by accident. It is its own button, off by
//! default, and the module asks for the network permission because of it.

use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Networks larger than this are not swept.
///
/// A /16 is sixty-five thousand addresses and an afternoon; a /8 is a fortnight.
/// The networks people actually run — a /24, a /22 — are well inside this, and
/// anything bigger is a question this tool should not be answering one address
/// at a time.
const MAX_ADDRESSES: u32 = 4096;

/// The port the probe is addressed to: `discard`, which is defined to be a
/// port that throws away what it receives, and which nothing has listened on
/// in thirty years.
const DISCARD: u16 = 9;

/// How many threads knock at once. Enough to cross a /24 in under a second,
/// few enough that the burst does not look like something it is not.
const THREADS: usize = 64;

/// What a sweep came to.
#[derive(Debug, Default, Clone, Copy)]
pub struct Swept {
    /// Addresses asked.
    pub asked: usize,
    /// Networks that were too large to walk.
    pub skipped: usize,
}

/// Knock on every address of these networks.
///
/// Returns once every address has been asked. What answered is not reported
/// here: the answers arrive in the kernel's neighbour table, and reading that
/// is the job of the collector, which does it the same way whether a machine
/// spoke first or was asked.
pub fn sweep(nets: &[(u32, u32)]) -> Swept {
    let mut out = Swept::default();
    let mut todo: Vec<Ipv4Addr> = Vec::new();
    for (net, mask) in nets {
        let size = (!mask).wrapping_add(1);
        if !(2..=MAX_ADDRESSES).contains(&size) {
            out.skipped += 1;
            continue;
        }
        // Neither the network address nor the broadcast: one is nobody and the
        // other is everybody, and asking everybody is a different thing again.
        for host in 1..size - 1 {
            todo.push(Ipv4Addr::from((net & mask) + host));
        }
    }
    out.asked = todo.len();
    if todo.is_empty() {
        return out;
    }

    let todo = Arc::new(todo);
    let next = Arc::new(AtomicUsize::new(0));
    std::thread::scope(|scope| {
        for _ in 0..THREADS.min(todo.len()) {
            let (todo, next) = (Arc::clone(&todo), Arc::clone(&next));
            scope.spawn(move || {
                // One socket per thread, bound to everything: the kernel picks
                // the card by the routing table, which is the same decision it
                // makes for any other packet to that address.
                let Ok(socket) = UdpSocket::bind("0.0.0.0:0") else {
                    return;
                };
                let _ = socket.set_broadcast(false);
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(ip) = todo.get(i) else {
                        return;
                    };
                    // Nothing in it. The question is asked by the sending, not
                    // by anything sent.
                    let _ = socket.send_to(&[], SocketAddrV4::new(*ip, DISCARD));
                }
            });
        }
    });
    // The answers come back as ARP replies, which the kernel writes into the
    // neighbour table — not to us. Long enough for the last of them to arrive
    // before the table is read.
    std::thread::sleep(std::time::Duration::from_millis(600));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn net(a: u8, b: u8, c: u8, d: u8, bits: u8) -> (u32, u32) {
        let mask = if bits == 0 { 0 } else { u32::MAX << (32 - bits) };
        (u32::from_be_bytes([a, b, c, d]) & mask, mask)
    }

    /// Every address on the network, and neither of the two that are not a
    /// machine: the network itself, and everybody at once.
    #[test]
    fn a_sweep_asks_every_host_and_only_hosts() {
        let swept = sweep(&[]);
        assert_eq!(swept.asked, 0);

        // Counted rather than sent, by asking for a network with nothing on
        // it: 10.255.255.0/30 is two usable addresses, and no route to them.
        let tiny = sweep(&[net(10, 255, 255, 0, 30)]);
        assert_eq!(tiny.asked, 2, "four addresses, two of them machines");
        assert_eq!(tiny.skipped, 0);
    }

    /// A network nobody could walk is left alone and said to be.
    #[test]
    fn a_network_too_large_to_walk_is_skipped() {
        let huge = sweep(&[net(10, 0, 0, 0, 8)]);
        assert_eq!(huge.asked, 0, "sixteen million addresses is not a sweep");
        assert_eq!(huge.skipped, 1);

        // And a /22 — a thousand addresses — is walked.
        let big = sweep(&[net(10, 255, 252, 0, 22)]);
        assert_eq!(big.asked, 1022);
    }
}

#[cfg(test)]
mod live {
    /// A real sweep of this machine's own networks, run by hand:
    /// `cargo test -- --ignored live_sweep --nocapture`.
    ///
    /// **This sends.** One empty datagram to every address on every network
    /// this machine is attached to — which is why it is not run by `cargo
    /// test` and has to be asked for by name.
    #[test]
    #[ignore = "sends to every address on this machine's networks; run with --ignored"]
    fn live_sweep() {
        let before = crate::collector::run_deep_scan().len();
        let nets: Vec<(u32, u32)> = crate::collector::interfaces()
            .into_iter()
            .flat_map(|i| i.nets)
            .collect();
        let started = std::time::Instant::now();
        let swept = super::sweep(&nets);
        let after = crate::collector::run_deep_scan();
        println!(
            "  asked {} addresses on {} networks in {:?}; {} skipped",
            swept.asked,
            nets.len(),
            started.elapsed(),
            swept.skipped
        );
        println!("  neighbours: {before} before, {} after", after.len());
        for n in &after {
            println!(
                "  {:<24} {}  on {:<16}",
                n.ips.join(","),
                n.mac,
                n.iface.as_deref().unwrap_or("")
            );
        }
    }
}
