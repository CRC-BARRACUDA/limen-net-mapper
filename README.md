# limen-net-mapper

A [Limen](https://github.com/CRC-BARRACUDA/Limen) module that maps the network
around the machine it runs on, from what that machine already knows. It provides
the `network.map` capability.

## Two buttons, and the difference between them

**Read the neighbour table** — the default, and what this module has always
been. It reads the record the kernel keeps of everything this machine has
spoken to, plus the routes and this machine's own interfaces, and draws them.
**Nothing is sent.** A machine that has been quiet is simply not in the table
yet.

**Find hosts** — the other half, and a different kind of act. One empty UDP
datagram to every address on every network this machine is attached to, which
makes the kernel ask *who has this address?* out loud; the machines that answer
land in the neighbour table and are then read like any other neighbour. No port
is examined, nothing is connected to, and the datagram is addressed to
`discard` (port 9) because the point is never the datagram — it is the question
the kernel has to ask before it can send one.

It is a separate button, it asks before it runs, and it is why the module
declares the `network` permission. On a network you are only looking at, this
is **the machine announcing itself to every address on it**. That is sometimes
exactly what you want and sometimes the last thing you want, which is why it is
a decision rather than a refresh.

Networks larger than 4096 addresses are not swept — a /16 is an afternoon and a
/8 is a fortnight — and the screen says how many were skipped.

Neither button needs elevation: the neighbour table is readable by an ordinary
user on all three platforms, and the probe is an ordinary datagram.

## One circle per machine

A machine is one thing on the map, however many cards it has, named by its
hostname when anybody knows one and by its hardware address when nobody does.

The **addresses are written on the lines**, one at each end. That is where an
address belongs: a machine does not *have* an address, it answers to one *on a
link*, and a machine with two cards answers to two. Written under the circle,
one of them has to be chosen and the map then says the machine *is* that
address — which stops being true the moment it is plugged into anything else.

```
                         PC-HOME
                  42:40:62:FC:A8:DA · 20:7B:D5:1A:3E:04
                              ( )
        192.168.0.126 /         \ 192.168.10.5
                     /           \
          192.168.0.1             192.168.10.1
              ( )                     ( )
      BC:FC:E7:94:6B:90        00:E2:59:00:FD:D0
```

Every network is included, bridges and tunnels too — a docker or libvirt bridge
is a real network with real machines on it, and on the machine being
investigated those are often the interesting ones. Whether there is hardware
behind an interface is still recorded and shown, because "this is a card" and
"this is a bridge to things running here" are different facts.

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
