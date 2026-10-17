# weave — docs

Study notes for the `weave` codebase, written to make the code, the syntax, and
the reasoning behind it easy to follow. Read them in this order.

| # | File | What it covers |
| - | ---- | -------------- |
| 1 | [01-intuition.md](./01-intuition.md) | The big picture. What problem we're solving and the mental model. |
| 2 | [02-code-flow.md](./02-code-flow.md) | Runtime execution flow: what runs, in what order, and why. |
| 3 | [03-syntax-flow.md](./03-syntax-flow.md) | Rust/tokio syntax walked through piece by piece. |
| 4 | [04-code-explanation.md](./04-code-explanation.md) | File-by-file, function-by-function explanation. |
| 5 | [05-concepts.md](./05-concepts.md) | Concept glossary: async, tokio, UDP, frames, channels, MTU. |
| 6 | [06-building.md](./06-building.md) | Build, run two peers, test, and lint. |
| 7 | [07-m2-tun-overlay.md](./07-m2-tun-overlay.md) | M2: the TUN overlay — design, privileges, routes, testing. |
| 8 | [08-m4-routing.md](./08-m4-routing.md) | M4: multi-hop routing — routing table, forwarding, TTL, testing. |
| 9 | [09-m5-topology.md](./09-m5-topology.md) | M5: topology exchange, Dijkstra, failure detection, failover, convergence. |

The M2 additions have their own study set mirroring this one, in
[`m2/`](./m2/README.md) (intuition, code flow, syntax, explanation, concepts,
building).

## Where we are right now

The **UDP transport** (M1), the **two-node TUN overlay** (M2), **multi-hop
routing** (M4) and **topology-driven routing with failover** (M5). Two processes
can bind a UDP socket and swap framed dev messages, bring up a TUN interface and
move IP packets, and route packets through intermediate peers. With M5 they also
exchange link state, calculate routes with Dijkstra, and recover over an
alternate path when a link fails. **M3 (peer lifecycle) is not a separate
service**; a minimal peer-availability stand-in lives in `src/peer.rs` and the
control plane derives liveness from its own heartbeats. No crypto, no discovery,
no endpoint migration.

The source:

```text
src/
  lib.rs        crate root, re-exports the modules
  wire.rs       frame layout: packet + control frames, encode / decode
  transport.rs  UDP socket: bind / send_message / send_control / recv_datagram
  packet.rs     ip packet validation, destination, hop limit, ttl decrement
  tun.rs        tun interface: open/configure, split into reader/writer
  route.rs      routing table: prefix, route, lookup, RouteSnapshot
  forward.rs    forwarding decision: deliver / forward / drop
  peer.rs       minimal peer availability (M3 stand-in)
  topology.rs   graph model, link validation, expiry, dijkstra routes
  control.rs    topology message encode / decode
  router.rs     control plane: flood, detect failures, recompute, publish
  overlay.rs    packet pipeline (tun <-> udp) with routing and bounded queues
  main.rs       CLI (clap) + startup wiring + shutdown
tests/
  transport.rs  integration tests over real sockets
  routing.rs    three-node multi-hop forwarding over real sockets
  topology.rs   deterministic failover simulation + real-socket failover
  tun_open.rs   real-interface test (needs root, ignored by default)
```

## Reference

- `README.md` (repo root) — user-facing description and the frame layout table.
- Frame layout: `[ version 1B ][ len 2B BE ][ payload 0..N ]`, cap 1500 bytes.
