# 05 — Concepts

The ideas and vocabulary behind the code. Each entry: what it is, why it's here,
and where to see it.

## Networking

### UDP (User Datagram Protocol)

A connectionless transport. You fire a **datagram** at an address; it may arrive,
arrive out of order, arrive twice, or not at all. No handshake, no retransmit, no
ordering guarantee. Low overhead, which is exactly what a VPN wants as a base —
reliability (if any) is layered on top.

- **See:** `tokio::net::UdpSocket`, `send_to`, `recv_from`.

### Datagram

One UDP packet — a self-contained message with a length. `recv_from` tells you
how many bytes arrived; anything beyond that in your buffer is stale and must be
ignored (`&buf[..len]`).

### Socket / bind

A socket is the OS handle for sending/receiving. `bind` attaches it to a local
`IP:port`. `127.0.0.1:0` means "loopback, any free port" — tests use it to avoid
port clashes.

### SocketAddr

`IP address + port`, rendered as `127.0.0.1:4000`. `recv_from` returns the sender
as a `SocketAddr`, which is how we know *who* sent a datagram.

### MTU and fragmentation

MTU (Maximum Transmission Unit) is the largest frame a link carries at once —
1500 bytes on typical Ethernet. Exceed it and IP splits ("fragments") the packet,
which is fragile and can be dropped. `MAX_DATAGRAM_SIZE = 1500` keeps whole
frames under that line so a LAN doesn't need to fragment.

### Overlay network / Layer 3

An **overlay** is a virtual network running on top of a physical one. **Layer 3**
means the IP layer — routing packets by IP address. `weave`'s end goal is an
overlay that carries IP packets between peers that have no direct link.

## Framing / protocols

### Frame / header + body

A self-describing envelope: a fixed header followed by a payload. The header
carries metadata the receiver needs to interpret the bytes. Here:
`[version 1B][len 2B BE]` + payload.

### Version byte

First byte of every frame. Lets the receiver reject frames from a different
protocol revision instead of misparsing them. Currently `1`.

### Big-endian (network byte order)

Multi-byte integers stored most-significant-byte first. Both ends agree on this
regardless of CPU architecture (x86 is little-endian; some others aren't). `len`
is written with `to_be_bytes` and read with `from_be_bytes`.

### Length prefix

The 2-byte field saying how many payload bytes follow. It's how the receiver
knows the true message boundary inside a datagram — and how it detects corruption
when the declared length doesn't match the bytes present.

### Zero-copy parsing

`decode` returns a *slice* into the input rather than copying the payload into a
new `Vec`. Cheaper and lifetime-checked by Rust.

### Defensive parsing

Validate everything before using it: size, minimum length, version, then declared
length. Reject (log + drop) rather than trust. Never let one bad packet crash the
listener.

## Rust & async

### Future / `.await`

A future is a value representing work that completes later. `.await` suspends the
current task until it resolves, letting the runtime run other tasks meanwhile. It
does *not* block the OS thread.

### tokio

The async runtime. `#[tokio::main]` builds it; `#[tokio::test]` builds one per
test. Provides the async `UdpSocket`, tasks, channels, and timers.

### Task / `tokio::spawn`

A task is a lightweight, cooperatively-scheduled unit of async work (like a
green thread). `spawn` hands a future to the runtime and returns a `JoinHandle`.

### `JoinHandle` / `JoinError`

The handle lets you `.await` a task's result. `JoinError` means the task panicked
or was cancelled — distinct from the task returning an `Err` value.

### `tokio::select!`

Waits on several futures at once and runs the branch for whichever completes
first, cancelling the others. Used in two places: the main event race (Ctrl+C vs
recv task) and the receive loop (shutdown vs datagram).

### `watch` channel

A one-to-many broadcast of the **latest value**. Senders do `send(true)`; all
receivers see it via `changed()`. Dropping the sender also wakes receivers (with
an error) — used deliberately in tests. Chosen over other channels because we only
care about the *current* state ("should we stop?"), not a queue of messages.

### `Result`, `?`, `map_err`, and error enums

Rust errors are values. `Result<T, E>` is `Ok(T)` or `Err(E)`. `?` propagates
early; `map_err` converts error types at boundaries; custom enums (`WireError`)
carry structured context. No exceptions, no hidden control flow.

### Ownership and borrowing

Each value has one owner; references (`&T`) borrow without taking ownership.
Function signatures show it: `decode(&[u8]) -> &[u8]` borrows input and output;
`send_message(&UdpSocket, ...)` borrows the socket so the caller keeps it.

### `Send` / `'static` in `spawn`

`tokio::spawn` requires the future to be `Send` (safe to move across threads) and
`'static` (owns its data). `recv_loop` takes `socket` and `shutdown` **by value**
so the spawned task owns everything it needs — no dangling references.

## Design vocabulary

### Layering / separation of concerns

`wire` = pure encoding. `transport` = socket IO. `main` = policy and lifecycle.
Each layer depends only on the one beneath it, so each is independently testable.

### Library + binary crate

The same package exposes a `lib` (for reuse/testing) and a `bin` (the CLI). Tests
import the lib; users run the bin.

### Graceful shutdown

A cooperative stop: signal intent (`watch` → `true`), let the task finish its
current step and return `Ok(())`, then `.await` the handle to confirm it exited.
No abrupt killing.

### Integration vs unit tests

Unit tests (`src/wire.rs`) cover pure logic, no OS. Integration tests
(`tests/transport.rs`) exercise real sockets end to end and clean up after
themselves.
