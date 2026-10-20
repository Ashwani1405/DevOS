# 06 — Building, running, testing

## Prerequisites

- **Rust 1.75+** (see `rust-version` in `Cargo.toml`). Install via
  [rustup](https://rustup.rs): `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- No external services. Everything is in-process on loopback.

## Build

```sh
cargo build            # debug build -> target/debug/weave
cargo build --release  # optimized     -> target/release/weave
```

## Run two peers

Build once, then start two nodes in separate terminals. Each points `--peer` at
the *other's* `--bind` so they greet each other.

Terminal 1 (listens on 4000, messages 4001):

```sh
cargo run -- --bind 127.0.0.1:4000 --peer 127.0.0.1:4001
```

Terminal 2 (listens on 4001, messages 4000):

```sh
cargo run -- --bind 127.0.0.1:4001 --peer 127.0.0.1:4000
```

Expected: each side logs `sent initial data message` and `received message`
(address + payload length). `Ctrl+C` stops a node and the receive task exits
cleanly (`receive task stopped cleanly`).

### Receive-only mode

Omit `--peer` to just listen:

```sh
cargo run -- --bind 127.0.0.1:4000
```

### CLI options

```text
--bind <ADDR:PORT>   local UDP address to bind (required)
--peer <ADDR:PORT>   peer to send one message to at startup (optional)
-h, --help           print help
-V, --version        print version
```

## Test

```sh
cargo test             # unit tests (src/wire.rs) + integration tests (tests/transport.rs)
cargo test -- --nocapture   # to see log output from tests
```

What's covered:

- `src/wire.rs` — encode/decode round trips, empty payload, over-max payload,
  short datagram, bad version, length mismatch, oversized datagram, truncated
  payload.
- `tests/transport.rs` — binding, two-endpoint exchange with sender identity,
  malformed and oversized input survival, clean shutdown, and shutdown-on-drop.

The integration tests use random free ports (`127.0.0.1:0`) and 2-second
timeouts, so they don't collide and fail fast.

## Format and lint

```sh
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
```

`-D warnings` promotes clippy lints to errors — run this before committing so CI
(or the next reader) isn't surprised.

## Manual smoke test (no second terminal)

Send a raw framed frame to a running node with `nc`. A frame for `"hi"` is
`01 00 02 68 69`:

```sh
printf '\x01\x00\x02\x68\x69' | nc -u -w1 127.0.0.1 4000
```

The node should log `received message` with `payload_len=2`.

Malformed input is safe to try too — the loop logs a warning and keeps running:

```sh
printf '\x01\x02' | nc -u -w1 127.0.0.1 4000          # too short -> rejected
```

## Troubleshooting

| Symptom | Likely cause |
| ------- | ------------ |
| `Address already in use` | Another process holds that UDP port. Pick another or stop the old one. |
| No `received message` | Peer address wrong, or the other node isn't running. UDP drops silently — there's no error to see. |
| `received message` but empty/log-less payload | Check the frame's length field matches the body; a mismatch is rejected. |
| Tests flaky on CI | Usually port contention or a slow machine; the tests use free ports and timeouts to avoid this. |

## Where to go next (matching the roadmap)

The transport is the foundation. The README "Not done yet" list maps to future
layers, roughly in dependency order:

1. **Real wire protocol** — replace the placeholder 3-byte header.
2. **Crypto** — encrypt/authenticate the data plane (control messages already use Noise IK).
3. **TUN device + IP forwarding** — carry actual IP packets.
4. **Peer discovery** — stop hand-configuring addresses.
5. **Routing / multi-hop** — reach peers with no direct link.
6. **Failure detection / failover.**
