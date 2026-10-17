# 04 — Code explanation

File-by-file, item-by-item reference. For execution order see
[02-code-flow.md](./02-code-flow.md); for syntax see [03-syntax-flow.md](./03-syntax-flow.md).

---

## `src/lib.rs` — crate root

```rust
pub mod transport;
pub mod wire;
```

Declares the two public modules. The crate is both a **library** (`weave`) and a
**binary** (`weave`), per `Cargo.toml`:

```toml
[lib]
name = "weave"
path = "src/lib.rs"
[[bin]]
name = "weave"
path = "src/main.rs"
```

Why both? The binary needs the logic, and the integration tests need to import
`weave::transport` / `weave::wire`. A lib + bin split is the standard way to make
code testable from `tests/`.

---

## `src/wire.rs` — the frame format

The pure, side-effect-free half of the system: no sockets, no async. Just bytes
in and bytes out. Easy to test exhaustively.

### Constants

| Name | Value | Meaning |
| ---- | ----- | ------- |
| `VERSION` | `1` | protocol version; bump when the layout changes |
| `HEADER_LEN` | `3` | 1 byte version + 2 bytes length |
| `MAX_DATAGRAM_SIZE` | `1500` | whole-frame cap (under Ethernet MTU) |
| `MAX_PAYLOAD_LEN` | `1497` | `MAX_DATAGRAM_SIZE - HEADER_LEN` — body cap |

### `encode(payload: &[u8]) -> Result<Vec<u8>, WireError>`

Turns a payload into `[VERSION][len BE][payload]`.

1. **Guard:** if `payload.len() > MAX_PAYLOAD_LEN`, return `PayloadTooLarge`.
2. Cast length to `u16`, write it big-endian after the version byte.
3. Append the payload.

Returns an owned `Vec<u8>` because we're building new bytes.

### `decode(datagram: &[u8]) -> Result<&[u8], WireError>`

Validates a datagram and returns a **borrow** of its payload (zero-copy).

Checks, in order:

1. `len > MAX_DATAGRAM_SIZE` → `DatagramTooLarge` (catches truncated reads too).
2. `len < HEADER_LEN` → `TooShort`.
3. `version != VERSION` → `UnsupportedVersion`.
4. declared length != actual body length → `LengthMismatch`.

If all pass, `Ok(&datagram[HEADER_LEN..])`.

> Design note: validation lives *only* here. `transport.rs` never re-implements
> framing rules — it just calls `decode` and reacts to the result.

### `WireError`

Five variants, each carrying its own context so messages are self-contained:

- `TooShort { len }`
- `DatagramTooLarge { len, max }`
- `PayloadTooLarge { len, max }`
- `UnsupportedVersion(u8)`
- `LengthMismatch { declared, actual }`

`Display` renders a human-readable line per variant; `impl Error` marks it as an
error. It derives `PartialEq, Eq` so tests can assert exact error values.

### Tests (`src/wire.rs:135`)

| Test | Asserts |
| ---- | ------- |
| `round_trips_payload` | encode then decode returns the payload; length is header + body |
| `encodes_empty_payload` | empty body → `[1, 0, 0]`, decodes to empty |
| `encode_rejects_payload_over_max` | one over the cap → `PayloadTooLarge` |
| `decode_rejects_short_datagram` | `[]`, `[1]`, `[1, 0]` → `TooShort` |
| `decode_rejects_bad_version` | version `0x09` → `UnsupportedVersion` |
| `decode_rejects_length_mismatch` | declared 5, actual 2 → mismatch |
| `decode_rejects_oversized_datagram` | `MAX+1` bytes → `DatagramTooLarge` |
| `decode_rejects_truncated_payload` | declared 200, none present → mismatch |

These are **unit tests** because they touch no OS resources — fast and deterministic.

---

## `src/transport.rs` — the UDP side

Wraps tokio's `UdpSocket`. Thin by design: it frames via `wire` and moves bytes.

### `bind(addr) -> io::Result<UdpSocket>`

One line: `UdpSocket::bind(addr).await`. Async because binding is tagged
`.await`, matching the runtime.

### `send_message(socket, dest, payload) -> io::Result<()>`

1. `wire::encode(payload)`, mapping `WireError` → `io::Error::InvalidInput`.
   This keeps the transport's error type uniform (`io::Error`).
2. `socket.send_to(&bytes, dest).await`.

Note it only ever sends **one** datagram; there is no retry or chunking.

### `recv_datagram(socket, buf) -> io::Result<(usize, SocketAddr)>`

`recv_from`. Returns how many bytes arrived **and** the sender's address. The
caller owns the buffer, which is why `buf` is a parameter rather than allocated
inside.

### `recv_loop(socket, shutdown) -> io::Result<()>`

The long-running task.

- **Early exit:** if `shutdown` is already `true`, return immediately.
- **Buffer:** `vec![0; MAX_DATAGRAM_SIZE + 1]` — the extra byte is deliberate so
  an oversized datagram is *truncated* (and therefore caught by `decode`) rather
  than accepted whole.
- **Loop:** `select!` between `shutdown.changed()` and `recv_datagram`.
  - shutdown fires → log debug and `return Ok(())`.
  - datagram arrives → slice `&buf[..len]` and call `handle_datagram`; IO error →
    log a warning and keep looping.

### `handle_datagram(datagram, from)`

Decides what to log; never aborts.

- `Ok(payload)` → `info!` with sender and payload length.
- `Err(DatagramTooLarge)` → `warn!` "ignored oversized or truncated".
- any other `Err` → `warn!` "rejected malformed".

Returns `()`. The loop's survival is the point.

---

## `src/main.rs` — CLI and lifecycle

### `Args`

clap-derive struct. `bind` is required; `peer` is optional. Doc comments become
help text. `SocketAddr` gives free parsing and validation.

### `main()`

Initializes tracing, parses args, delegates to `run`. Thin on purpose.

### `run(args) -> Result<(), Box<dyn Error>>`

The orchestrator:

1. `transport::bind(args.bind)`.
2. If `--peer`: format a hello string and `send_message`.
3. Create the `watch` shutdown channel.
4. `tokio::spawn(recv_loop(socket, shutdown_rx))` → returns `recv_handle`.
5. `tokio::select!` on Ctrl+C vs the handle.
6. `shutdown_tx.send(true)`, then report the receiver's outcome.

A `watcher` detail: the handle is polled as `&mut recv_handle` in `select!` so
it's still available to `.await` afterward in the Ctrl+C branch.

### `report_receiver(result)`

Takes the awkward `Result<io::Result<()>, JoinError>` and logs one of:

- `Ok(Ok(()))` → "stopped cleanly"
- `Ok(Err(e))` → "ended with error"
- `Err(e)` → "failed to join" (the task panicked)

---

## `tests/transport.rs` — integration tests

These spin up **real sockets** on random free ports (`127.0.0.1:0`) with short
2s timeouts. Each test cleans up its task.

| Test | Scenario |
| ---- | -------- |
| `binds_to_a_udp_socket` | `bind` assigns a non-zero port |
| `two_endpoints_exchange_a_message_and_identify_the_sender` | send `"ping"`, receive it, verify the sender address and decoded payload |
| `recv_loop_ignores_malformed_input` | send a too-short frame; loop survives; shutdown works |
| `recv_loop_ignores_oversized_datagram` | send `MAX+64` bytes; loop survives; shutdown works |
| `shutdown_terminates_the_recv_loop_cleanly` | signal shutdown → task joins `Ok` |
| `dropping_the_shutdown_sender_stops_the_recv_loop` | drop the sender → `changed()` errors → loop exits |

The last one pins an important behavior: `watch::Receiver::changed()` resolves
with an error when the sender is dropped, and we treat that as "stop".

---

## Cross-cutting patterns worth naming

- **Layering:** `wire` (pure) ← `transport` (IO) ← `main` (policy/lifecycle).
  Each layer only knows the one below it.
- **Errors as values:** typed enum in `wire`, `io::Error` in `transport`, boxed
  in `main`. Converted at the boundary with `map_err`/`?`.
- **Borrowed where possible:** `decode` returns a slice; send takes `&[u8]`.
- **No panics on bad input:** malformed/oversized datagrams are logged, never fatal.
