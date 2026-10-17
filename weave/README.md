# weave

Weave is a peer-to-peer Layer-3 overlay network written in Rust. The idea is a
virtual network on top of the physical one, so peers can exchange IP packets
without a direct link between them.

This repo has the UDP transport (M1), the two-node TUN overlay (M2), multi-hop
routing with static routes (M4), and topology-driven routing with automatic
failover (M5). Peers now exchange link state, calculate routes with Dijkstra, and
switch to an alternate path when a link fails. Peer lifecycle/probing (M3) is
still not a separate service; M5 derives liveness from the control plane's
heartbeats. Encryption, discovery, and endpoint migration aren't built yet.

See [`docs/08-m4-routing.md`](./docs/08-m4-routing.md) for static multi-hop
routing and [`docs/09-m5-topology.md`](./docs/09-m5-topology.md) for topology
exchange, failure detection, and recovery.

## The transport

A small UDP transport that lets two processes exchange framed messages. Each
process can:

- bind to a UDP address (`--bind`),
- send one message to a peer at startup (`--peer`),
- receive datagrams and see who sent them,
- drop malformed or oversized datagrams instead of crashing,
- shut down on Ctrl+C.

This is a local development transport, not the real wire protocol. It doesn't
encrypt anything, doesn't guarantee delivery or ordering, and shouldn't be used
outside local testing.

## Frame layout

Frames are a fixed 3-byte header followed by the payload:

```text
 0        1                 2                2 + N
+--------+--------+--------+------------------+
| version|  payload length |     payload      |
| 1 byte | 2 bytes big-end |   0..=N bytes    |
+--------+--------+--------+------------------+
```

| Field            | Size    | Notes                                |
| ---------------- | ------- | ------------------------------------ |
| `version`        | 1 byte  | protocol version, `1` for now        |
| `payload length` | 2 bytes | big-endian byte count of the payload |
| `payload`        | 0–N     | message body                         |

Size limits:

- biggest datagram: **1500 bytes** (`MAX_DATAGRAM_SIZE`) — keeps us under a
  typical Ethernet MTU so we don't rely on IP fragmentation on a LAN
- biggest payload: **1497 bytes** (`MAX_PAYLOAD_LEN` = 1500 − 3)

On receive we drop anything that isn't a valid frame: too short, too big, wrong
version, or a length that doesn't match the payload.

## Running two peers

Build once:

```sh
cargo build
```

Then, in terminal 1, start the first node (listen on 4000, message peer on 4001):

```sh
cargo run -- --bind 127.0.0.1:4000 --peer 127.0.0.1:4001
```

In terminal 2, start the second node (listen on 4001, message peer on 4000):

```sh
cargo run -- --bind 127.0.0.1:4001 --peer 127.0.0.1:4000
```

Each node sends one message at startup and logs a `received message` line when it
gets a datagram. Ctrl+C stops a node and the receive task exits.

CLI options:

```text
--bind <ADDR:PORT>   local UDP address to bind (required)
--peer <ADDR:PORT>   peer endpoint, repeatable (message mode sends to the first)
--tun                 run the TUN overlay instead of message mode
--address <IP>        this node's overlay address (with --tun)
--peer-address <IP>   the peer's overlay address (with --tun)
--route <P=ADDR:PORT> static route, repeatable (with --tun)
--neighbor <IP=ADDR:PORT> topology neighbour, repeatable; enables dynamic
                      routing and automatic failover (with --tun)
--heartbeat-ms <N>    control-plane heartbeat interval (with --tun)
--link-timeout-ms <N> how long a silent neighbour is trusted (with --tun)
-h, --help            print help
-V, --version         print version
```

## Development

Format, lint, and test:

```sh
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

Tests live beside the modules in `src/` (`wire`, `packet`, `route`, `forward`,
`peer`, `overlay`, `topology`, `control`, `router`) and in `tests/`
(`transport.rs`, `routing.rs`, `topology.rs`, and the root-only `tun_open.rs`).
They use random free ports, a fake TUN, and short timeouts. `topology.rs`
includes a deterministic no-sleep simulation of a four-node failover.

## Not done yet

- no encryption or authentication, including for topology advertisements
- no delivery, ordering, or duplicate protection
- peer addresses are set by hand, no discovery
- no peer lifecycle/probing (M3) as its own service; liveness comes from the
  control-plane heartbeat
- convergence is eventual, not instantaneous or strongly consistent
- no endpoint migration, NAT traversal, or relay fallback (M6)
- the frame format is a placeholder and will change

## License

MIT OR Apache-2.0.
