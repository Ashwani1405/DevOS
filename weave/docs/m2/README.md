# weave — M2 docs

Same shape as the top-level [docs](../README.md) set, but for the M2 milestone:
the two-node TUN overlay that carries real IP packets. Read in this order.

| # | File | What it covers |
| - | ---- | -------------- |
| 1 | [01-intuition.md](./01-intuition.md) | Why a tunnel, what a TUN is, the mental model. |
| 2 | [02-code-flow.md](./02-code-flow.md) | Runtime flow: open TUN → split → 4 tasks → shutdown. |
| 3 | [03-syntax-flow.md](./03-syntax-flow.md) | New Rust/tokio syntax: RPITIT traits, channels, JoinSet. |
| 4 | [04-code-explanation.md](./04-code-explanation.md) | File-by-file, function-by-function. |
| 5 | [05-concepts.md](./05-concepts.md) | TUN, packet info, backpressure, deadlock, privileges. |
| 6 | [06-building.md](./06-building.md) | Privileges, run two nodes, routes, ping, tests. |

For the milestone spec and the "official" walkthrough, see
[../07-m2-tun-overlay.md](../07-m2-tun-overlay.md).

## What M2 added

```text
src/
  packet.rs   NEW  ip packet validation (ipv4/ipv6)
  tun.rs      NEW  tun interface; split into reader + writer
  overlay.rs  NEW  forwarding pipeline (tun <-> udp), bounded queues
  main.rs     CHG  --tun selects overlay mode
tests/
  tun_open.rs NEW  real-interface test (needs root, ignored)
Cargo.toml    CHG  tun crate (async) + tokio io-util
```

The M1 pieces (`wire.rs`, `transport.rs`) are reused unchanged: an IP packet is
just the frame payload.
