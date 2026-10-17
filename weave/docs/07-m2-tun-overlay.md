# 07 — M2: two-node TUN overlay

M2 turns the M1 message transport into an actual tunnel: two nodes read IP
packets from a virtual interface, ship them over UDP, and inject them into the
peer's interface. This doc explains the design, then how to run and test it.

## Goal

```text
Node A                                      Node B
------                                      ------

Application                                Application
     |                                          ^
     v                                          |
 TUN Interface                                  |
     |                                          |
 Read IP Packet                                 |
     |                                          |
 Validate Packet                                |
     |                                          |
 UDP Transport  --------------------->  UDP Transport
                                                |
                                         Validate Packet
                                                |
                                                v
                                           TUN Interface
```

Both directions use the same path.

## Modules

| File | Role |
| ---- | ---- |
| `src/packet.rs` | IP packet validation. Pure, no IO. |
| `src/tun.rs` | Opens/configures the interface and exposes read/write halves. |
| `src/overlay.rs` | The forwarding pipeline. Glues TUN to UDP. |
| `src/wire.rs` | Frame layout (unchanged from M1). An IP packet is just the payload. |
| `src/transport.rs` | UDP socket helpers (unchanged). |
| `src/main.rs` | `--tun` selects overlay mode; otherwise M1 message mode. |

The split is deliberate: `tun.rs` knows nothing about UDP, `overlay.rs` knows
nothing about utun ioctls, and `packet.rs` knows nothing about sockets. Each is
testable on its own.

## Packet validation (`packet.rs`)

Before a packet is forwarded we check it is well-formed:

- **IPv4:** at least 20 bytes, `ihl >= 5`, header fits inside the slice, and the
  header's total-length field equals the bytes present.
- **IPv6:** at least 40 bytes and `40 + payload-length` equals the bytes present.
- Anything whose first nibble isn't 4 or 6 is rejected.

Failures come back as a typed `PacketError` and the packet is dropped, not
forwarded. This is the only place packet rules live.

## The forwarding pipeline (`overlay.rs`)

Four tasks joined by two **bounded** queues, one per direction:

```text
tun reader ──> [outbound queue] ──> udp sender ──> peer
tun writer <── [inbound queue]  <── udp receiver <── peer
```

- Each queue has `queue_depth` slots (default 1024). When full, the producer
  *awaits* on the send, so a slow side slows the fast side instead of growing
  memory without bound. That is the backpressure.
- No task waits on two queues, and each queue has exactly one producer and one
  consumer, so the pipeline cannot deadlock.
- Only the configured `--peer` address is trusted; datagrams from anyone else are
  logged and dropped.
- Malformed frames, oversized datagrams, and invalid IP packets are logged and
  dropped individually. One bad packet never stops the tunnel.
- When any task ends (error, or shutdown), the rest are signalled to stop.

### Shutdown

`main` owns a `watch` channel. Pressing Ctrl+C flips it; the bridge task inside
`overlay::run` forwards that to the internal stop signal, every task returns, and
`run` joins them. Dropping the sender has the same effect.

## Running it

### Privileges

Creating a TUN interface needs **root** on every platform. On macOS the device is
a `utun`. Run the command with `sudo`, or grant the binary the capability.

### Node A

```sh
sudo cargo run -- \
  --tun \
  --bind 127.0.0.1:4000 \
  --peer 127.0.0.1:4001 \
  --address 10.77.0.1 \
  --peer-address 10.77.0.2
```

### Node B (second terminal)

```sh
sudo cargo run -- \
  --tun \
  --bind 127.0.0.1:4001 \
  --peer 127.0.0.1:4000 \
  --address 10.77.0.2 \
  --peer-address 10.77.0.1
```

Flags:

| Flag | Meaning |
| ---- | ------- |
| `--bind` | local UDP address |
| `--peer` | the other node's UDP endpoint (required with `--tun`) |
| `--tun` | enable overlay mode |
| `--tun-name` | request a specific `utunN` (macOS); omit to auto-pick |
| `--address` | this node's overlay IPv4 |
| `--peer-address` | the peer's overlay IPv4 (utun is point-to-point) |
| `--mtu` | overlay MTU (default 1420, must be ≤ 1497) |
| `--queue-depth` | bounded queue size per direction (default 1024) |

### Interface and routes

Our code sets the address, the point-to-point destination, the netmask, the MTU,
and brings the interface up. On macOS you can confirm it:

```sh
ifconfig utunN | grep -E 'inet|mtu'
netstat -rn | grep utun
```

Because the destination is set, the kernel routes the peer's overlay address
(`10.77.0.2`) through the interface. To reach a larger prefix behind the peer,
add a route by hand, e.g.:

```sh
sudo route -n add -net 10.77.0.0/24 -interface utunN
# linux equivalent:
# sudo ip route add 10.77.0.0/24 dev weave0
```

The exact interface name is printed in the startup log.

### Verify connectivity

```sh
ping 10.77.0.2        # from node A
ping 10.77.0.1        # from node B
```

You should see the packets flow in both nodes' logs as `received message`. Ping
may need the overlay route above to be present.

## Testing

Root-free (runs in CI):

- `packet.rs` unit tests — valid/invalid IPv4 and IPv6.
- `overlay.rs` integration tests using a **channel-backed fake TUN** and real
  UDP on loopback: bidirectional forwarding, malformed outbound drop, malformed
  inbound drop, stranger-address rejection, shutdown-on-drop.

Needs root:

- `tests/tun_open.rs::opens_and_splits_a_tun_interface` — opens a real interface
  and splits it. Ignored by default:

```sh
sudo cargo test --test tun_open -- --ignored
```

## Out of scope

Multi-hop routing, dynamic discovery, failover, NAT traversal, and encryption
are later milestones. This is a two-node, directly-connected tunnel.
