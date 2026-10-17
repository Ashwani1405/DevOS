# M2 — Code explanation

File-by-file reference for the M2 additions. M1 files are in
[../04-code-explanation.md](../04-code-explanation.md).

---

## `Cargo.toml`

```toml
tun = { version = "0.8", features = ["async"] }
```

- `tun` creates/configures TUN interfaces cross-platform. The `async` feature
  gives us `tun::AsyncDevice` (a tokio `AsyncFd` wrapper) and
  `create_as_async`.
- tokio gains the `io-util` feature for `AsyncReadExt`/`AsyncWriteExt`.
- On macOS the crate creates a `utun`; it internally strips the 4-byte packet
  information header, so `read`/`write` see pure IP packets.

---

## `src/packet.rs`

Pure validation, no IO, easy to test.

### Constants

| Name | Value | Meaning |
| ---- | ----- | ------- |
| `IPV4_MIN_HEADER_LEN` | 20 | smallest IPv4 header (IHL 5 × 4) |
| `IPV6_HEADER_LEN` | 40 | fixed IPv6 header |

### `validate(packet) -> Result<(), PacketError>` (`src/packet.rs:43`)

Looks at the high nibble and dispatches:

- `4` → `validate_ipv4`
- `6` → `validate_ipv6`
- anything else → `UnsupportedVersion`
- empty slice → `TooShort { len: 0 }`

### `validate_ipv4` (`src/packet.rs:55`)

1. `len >= 20` else `TooShort`.
2. `ihl = packet[0] & 0x0f`; require `ihl >= 5` and `ihl*4 <= len`, else
   `InvalidIpv4HeaderLen`.
3. `declared = u16::from_be_bytes(packet[2..4])`; require `declared == len`, else
   `LengthMismatch`.

### `validate_ipv6` (`src/packet.rs:80`)

1. `len >= 40` else `TooShort`.
2. `declared = 40 + payload_length(packet[4..6])`; require `declared == len`.

### `PacketError` + `Display` + `Error`

Five variants carry their own context. Derives `PartialEq, Eq` so tests assert
exact errors. `Display` gives human-readable log lines.

### Tests

Accept valid IPv4 (plain and with options), valid IPv6; reject empty/short,
unknown version, bad IHL (too small and past end), and length mismatches for both
versions.

---

## `src/tun.rs`

Owns the interface; knows nothing about UDP.

### Traits

```rust
pub trait PacketReader: Send + 'static {
    fn recv<'a>(&'a mut self, buf: &'a mut [u8]) -> impl Future<Output = io::Result<usize>> + Send + 'a;
}
pub trait PacketWriter: Send + 'static {
    fn send<'a>(&'a mut self, packet: &'a [u8]) -> impl Future<Output = io::Result<()>> + Send + 'a;
}
```

These are the seam that lets `overlay.rs` be tested without a real device: the
real code implements them over a `utun`, the tests over channels.

### `TunConfig` (`src/tun.rs:33`)

`name`, `address`, `destination`, `netmask`, `mtu`, `enable_routing`. The
point-to-point pair is `address` (us) / `destination` (peer).

### `TunDevice::open` (`src/tun.rs:55`)

Builds a `tun::Configuration`:

- optional `tun_name` (macOS needs `utunN`, or omit for auto-pick)
- `.address/.destination/.netmask/.mtu/.up()`
- `platform_config(|p| p.enable_routing(...))`

then `tun::create_as_async(&config)`, mapping the crate error to `io::Error`.
Needs root.

### `TunDevice::split` (`src/tun.rs:76`)

Calls the crate's `split()` → `(DeviceWriter, DeviceReader)`, and wraps them in
`TunReader`/`TunWriter`. Splitting lets the two directions run as separate tasks
over one fd.

### `TunReader` / `TunWriter` impls

- `recv` → `AsyncReadExt::read` (one packet per call).
- `send` → `AsyncWriteExt::write_all` (never a partial packet).

---

## `src/overlay.rs`

The forwarding policy. Bounded queues + task group.

### `OverlayConfig` (`src/overlay.rs:23`)

`peer` (UDP addr), `mtu` (read buffer size), `queue_depth` (per-direction bound).

### `run` (`src/overlay.rs:36`)

1. Wrap the socket in `Arc`.
2. Create two bounded `mpsc` channels (`outbound`, `inbound`).
3. Create an internal `watch` stop channel.
4. `JoinSet` + spawn the four tasks (each gets a `stop` clone).
5. Spawn a bridge that watches the caller's `external` shutdown and flips `stop`.
6. `join_next` until empty, remembering the first error; on each completion,
   `stop.send(true)`.
7. Abort the bridge; return the first error or `Ok`.

### The four tasks

| Fn | Line | Direction | Fatal on |
| -- | ---- | --------- | -------- |
| `tun_to_queue` | `114` | reader → outbound | TUN read error |
| `queue_to_udp` | `149` | outbound → socket→peer | — (send errors logged) |
| `udp_to_queue` | `174` | socket→filter→inbound | socket read error |
| `queue_to_tun` | `220` | inbound → writer | TUN write error |

Shared structure: `select! { stop.changed() => return Ok(()), work => ... }`.
`tun_to_queue` and `udp_to_queue` call `packet::validate` before enqueuing;
`udp_to_queue` also checks `from == peer`.

### Tests

A channel-backed `FakeReader`/`FakeWriter` stand in for the device, driving the
real `run` over real loopback UDP. Cases: bidirectional forwarding, malformed
outbound drop (loop survives), malformed inbound drop, stranger-address
rejection, shutdown-on-sender-drop. No root needed.

---

## `src/main.rs` (overlay additions)

### `Args`

New options: `--tun` (bool), `--tun-name`, `--address`, `--peer-address`,
`--mtu`, `--queue-depth`. All except `--tun` use `requires = "tun"`, so clap
rejects them without `--tun`. `--queue-depth` has `default_value_t = 1024`.

### `run` (`src/main.rs:66`)

Dispatches: `args.tun ? run_overlay : run_message`.

### `run_overlay` (`src/main.rs:108`)

1. Require `--peer`, `--address`, `--peer-address` (via `ok_or_else`).
2. `mtu` default `DEFAULT_MTU` (1420); reject `mtu > wire::MAX_PAYLOAD_LEN`.
3. Build `TunConfig` (netmask `/32`, destination = peer overlay IP,
   `enable_routing: false`).
4. `TunDevice::open` → `split`.
5. `transport::bind`, build `OverlayConfig`.
6. `watch` channel, spawn `overlay::run`.
7. Same `select!` Ctrl+C / handle shutdown as M1; report via `report_receiver`.

---

## `tests/tun_open.rs`

One `#[ignore]`d test that opens a real interface and splits it. Run with
`sudo cargo test --test tun_open -- --ignored`. Ignored so the default `cargo
test` stays root-free and CI-friendly.
