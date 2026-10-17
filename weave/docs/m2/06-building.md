# M2 — Building & running the overlay

## Prerequisites

- Rust 1.75+ (see `Cargo.toml`).
- **Root** to open the TUN interface. Everything else is loopback.

## Build

```sh
cargo build
cargo build --release
```

## Run two nodes

The overlay needs the other node's **UDP endpoint** (`--peer`) and its **overlay
IP** (`--peer-address`).

Terminal 1:

```sh
sudo cargo run -- \
  --tun \
  --bind 127.0.0.1:4000 \
  --peer 127.0.0.1:4001 \
  --address 10.77.0.1 \
  --peer-address 10.77.0.2
```

Terminal 2:

```sh
sudo cargo run -- \
  --tun \
  --bind 127.0.0.1:4001 \
  --peer 127.0.0.1:4000 \
  --address 10.77.0.2 \
  --peer-address 10.77.0.1
```

Each node logs the created interface name (e.g. `utun4`) at startup.

### Flags

```text
--tun                run the overlay (otherwise the M1 dev message)
--tun-name <NAME>    requested interface name (macos: utunN; omit to auto-pick)
--address <IP>       this node's overlay IPv4 (required with --tun)
--peer-address <IP>  the peer's overlay IPv4 (required with --tun)
--mtu <N>            overlay MTU (default 1420, max 1497)
--queue-depth <N>    bounded queue per direction (default 1024)
```

Missing-required combinations fail fast with a clear message, e.g.
`--address is required together with --tun`.

## Interface and routes

We set the address, point-to-point destination, netmask, MTU, and bring the
interface up. Verify:

```sh
ifconfig utunN | grep -E 'inet|mtu'
netstat -rn | grep utun
```

To reach a subnet behind the peer:

```sh
sudo route -n add -net 10.77.0.0/24 -interface utunN
# linux:
# sudo ip route add 10.77.0.0/24 dev weave0
```

## Verify

```sh
ping 10.77.0.2     # from node A
ping 10.77.0.1     # from node B
```

Both nodes' logs should show `received message` for the ICMP packets. Ping needs
the overlay route to be present.

## Tests

Root-free (default `cargo test`):

- `packet.rs` unit tests — IPv4/IPv6 validation.
- `overlay.rs` tests — fake TUN + real loopback UDP: forwarding both ways,
  malformed outbound/inbound drops, stranger rejection, shutdown.

Needs root, ignored by default:

```sh
sudo cargo test --test tun_open -- --ignored
```

## Format & lint

```sh
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
```

## Troubleshooting

| Symptom | Likely cause |
| ------- | ------------ |
| `Operation not permitted` on start | not root |
| `Address already in use` | UDP port taken, or a stale utun |
| `InvalidName` | `--tun-name` isn't `utunN` (macOS) |
| Ping gets no reply | overlay route missing, or the peer address is wrong |
| Logs show drops | malformed packet, wrong `--queue-depth`, or high MTU causing oversized frames |
| Interface left behind | remove with `sudo ifconfig utunN down` |
