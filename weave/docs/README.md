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
| 10 | [10-m6-migration-relay.md](./10-m6-migration-relay.md) | M6: authenticated endpoint migration, direct/relay fallback, relay envelopes. |
| 11 | [11-m7-hardening.md](./11-m7-hardening.md) | M7: audit, security review, fault injection, benchmarks, engineering report. |

The M2 additions have their own study set mirroring this one, in
[`m2/`](./m2/README.md) (intuition, code flow, syntax, explanation, concepts,
building).

## Where we are right now

The **UDP transport** (M1), the **two-node TUN overlay** (M2), **multi-hop
routing** (M4), **topology-driven routing with failover** (M5),
**authenticated endpoint migration with relay fallback** (M6), and the **M7
hardening pass** are implemented. Two processes can bind a UDP socket and swap
framed dev messages, bring up a TUN interface and move IP packets, route packets
through intermediate peers, exchange link state and calculate routes with
Dijkstra, recover over an alternate path when a link fails, authenticate with a
Noise IK handshake, encrypt control messages, migrate a peer's endpoint with an
authenticated control message, and fall back to a relay when a direct path is
dead. Peer lifecycle/probing (`src/lifecycle.rs`) is an explicit state machine
(`Connected`/`Suspect`/`Disconnected`) driving probes and link up/down events.

**M7** audited the above, fixed a set of correctness, robustness and security
defects (see [`11-m7-hardening.md`](./11-m7-hardening.md)), and added fault
injection, lifecycle, rate-limiting, fuzzing and benchmark coverage. Control
messages are authenticated per peer and encrypted with Noise IK; data packets
are still plaintext.

The source:

```text
src/
  lib.rs            crate root, re-exports the modules
  wire.rs           frame layout: packet + control + relay + handshake frames
  transport.rs      UDP socket: bind / send_message / send_control / send_handshake / recv_datagram
  packet.rs         ip packet validation, destination, hop limit, ttl decrement
  tun.rs            tun interface: open/configure, split into reader/writer
  route.rs          routing table: prefix, route, lookup, RouteSnapshot
  forward.rs        forwarding decision: deliver / forward / drop
  peer.rs           peer availability
  topology.rs       graph model, link validation, expiry, dijkstra routes
  control.rs        topology / endpoint message encode / decode
  router.rs         control plane: handshake, decrypt, flood, migrate, recompute, publish
  noise.rs          Noise IK handshake and encrypted record layer (M6/M7)
  rate.rs           per-source and global token-bucket control rate limiting (M7)
  lifecycle.rs      peer Connected/Suspect/Disconnected probing state machine (M3)
  endpoints.rs      endpoint candidates, migration, epochs (M6)
  connectivity.rs   per-peer Direct-vs-Relay link and mode (M6)
  relay.rs          relay envelope encode / decode and forward decision (M6)
  overlay.rs        packet pipeline (tun <-> udp) with routing, relay and bounded queues
  main.rs           CLI (clap) + startup wiring + shutdown
benches/
  baseline.rs       micro-, Noise- and loopback end-to-end benchmarks (harness = false)
tests/
  transport.rs      integration tests over real sockets
  routing.rs        three-node multi-hop forwarding + lifecycle/overload tests
  topology.rs       deterministic failover/partition simulation + real-socket tests
  connectivity.rs   direct-vs-relay fallback and relay authorization
  fuzz.rs           deterministic mutational fuzzing of every decoder
  tun_open.rs       real-interface test (needs root, ignored by default)
```

## Reference

- `README.md` (repo root) — user-facing description and the frame layout table.
- Frame layout: `[ version 1B ][ len 2B BE ][ payload 0..N ]`, cap 1500 bytes.
