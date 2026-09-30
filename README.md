# limen-net-mapper

A [Limen](https://github.com/CRC-BARRACUDA/Limen) module that maps the network
around the machine it runs on, from what that machine already knows. It provides
the `network.map` capability.

## What it does, and what it deliberately does not

It reads the **neighbour table** and the **routes** — the record the kernel keeps
of everything this machine has spoken to, and the way off the network — and draws
them as a map: the gateway in the middle, everything else around it.

**Nothing is sent.** No ping sweep, no port scan, no probe of any kind. That is
the point of it: a scan that probes announces the machine doing the scanning, and
this is a tool carried onto somebody else's network. It asks the operating system
what it has already learned and nothing more, which is also why it needs no
elevation — the neighbour table is readable by an ordinary user on all three
platforms.

## Per platform

| | how the neighbour table is read | routes |
|---|---|---|
| Linux | `/proc/net/arp` (plus `/etc/hosts` for names) | `/proc/net/route` |
| macOS, BSD | `arp -an` | `netstat -rn` |
| Windows | PowerShell `Get-NetNeighbor` | `route print -4` |

Linux spawns nothing at all. The other two need a command, so `subprocess` is
declared once for the module rather than per platform — that is what the user is
being asked to agree to.

Entries that say nothing are dropped: incomplete rows, the all-zero and all-ones
hardware addresses, and multicast. A broadcast address is not a machine.

## The map

Drawn by the host, with Limen's own diagram widget — circles joined by lines,
laid out deterministically so the same scan comes out the same way twice and two
people can compare what they are looking at. There is no physics and no settling.

- **Hover** a circle for what the scan learned about that host: address, hardware
  address, what kind of thing it looks like.
- **Double-click** one to open it in a tab of its own.
- **Drag** the map to move it; pinch or ctrl+wheel to zoom; double-click the map
  itself to put it back.

A dashed line is a link that was inferred rather than observed — a virtual
machine hanging off its host, say.

## What it keeps

`topology.db`, beside the module: the nodes of the last scan, so the map is there
when you come back to it without scanning again. It is what one machine saw of
one network, and it is not committed to this repository.

## Building

```sh
cargo build --release          # a cdylib: libnet_mapper.so / .dll / .dylib
scripts/package.sh             # the dist/ bundle Limen installs
```

Against a working-tree SDK rather than the published one:

```sh
cargo build --release --config \
  "patch.\"https://github.com/CRC-BARRACUDA/Limen.git\".limen-sdk-rust.path=\"$LIMEN_SDK_PATH\""
```

## Licence

GPL-3.0-or-later. See [LICENSE](LICENSE).
