# 03 — Syntax flow

A walkthrough of the Rust and tokio syntax used in this repo, in the order you
meet it reading `main.rs` → `transport.rs` → `wire.rs`. The goal is that every
token stops being a mystery.

## `main` and attributes

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
```

- `#[tokio::main]` — a **proc-macro attribute**. It rewrites `async fn main`
  into a normal `fn main` that builds a tokio runtime and blocks on the future.
  Without it, `async fn main` isn't a legal entry point.
- `async fn` — a function that returns a **future** instead of running eagerly.
  Calling it does nothing until the future is `.await`ed.
- `Result<(), Box<dyn Error>>` — `()` is the success value (nothing). `Box<dyn Error>`
  is "any error type, boxed". This lets `?` convert and propagate many error
  types from one function without naming each one.
- `?` — the **try operator**. On `Err`, it returns early with that error
  (converting it into `Box<dyn Error>` via `From`). On `Ok`, it unwraps.

## The `Args` struct and clap derive

```rust
#[derive(Debug, Parser)]
#[command(name = "weave", version, about, long_about)]
struct Args {
    #[arg(long, value_name = "ADDR:PORT")]
    bind: SocketAddr,

    #[arg(long, value_name = "ADDR:PORT")]
    peer: Option<SocketAddr>,
}
```

- `#[derive(Debug, Parser)]` — `Debug` is the standard debug formatter.
  `Parser` is clap's derive macro; it generates the argument parser *from the
  struct shape*.
- Each field becomes a flag. `bind: SocketAddr` → required `--bind <ADDR:PORT>`.
  `peer: Option<SocketAddr>` → optional `--peer`; clap understands `Option` means
  "may be absent" and defaults it to `None`.
- `SocketAddr` implements `FromStr`, so clap parses `"127.0.0.1:4000"` for free
  and errors out on garbage.
- Doc comments (`///`) above fields/struct become the `--help` text.
- `value_name = "ADDR:PORT"` just controls how the placeholder reads in help.

`Args::parse()` returns an owned `Args` or exits the program with a usage error.

## Modules and visibility

```rust
pub mod transport;
pub mod wire;
```

`lib.rs` is the crate root. `pub mod x;` declares a module and makes it public,
so both the binary (`main.rs`) and the integration tests (`tests/*.rs`) can use
`weave::transport` / `weave::wire`. Inside a file, `pub fn` / `pub const`
exports items; `fn` (no `pub`) stays private to the module.

Imports use paths:

```rust
use weave::transport;                    // crate name, in main.rs / tests
use crate::wire::{self, WireError, MAX_DATAGRAM_SIZE};  // inside the crate
```

- `crate::` means "this crate".
- `self` in a `use` group means "bring the module itself in" (so `wire::decode`
  works) alongside the named items.

## Constants and types

```rust
pub const VERSION: u8 = 1;
pub const HEADER_LEN: usize = 3;
pub const MAX_DATAGRAM_SIZE: usize = 1500;
pub const MAX_PAYLOAD_LEN: usize = MAX_DATAGRAM_SIZE - HEADER_LEN;
```

- `const` is a compile-time constant, inlined at use sites. Expressions like
  `1500 - 3` are folded at compile time.
- Types matter: `usize` is the platform's index/pointer width, used for lengths
  and slices. `u8` is one byte.

## `encode`: building bytes

```rust
pub fn encode(payload: &[u8]) -> Result<Vec<u8>, WireError> {
    if payload.len() > MAX_PAYLOAD_LEN {
        return Err(WireError::PayloadTooLarge { len: payload.len(), max: MAX_PAYLOAD_LEN });
    }
    let payload_len = payload.len() as u16;
    let mut bytes = Vec::with_capacity(HEADER_LEN + payload.len());
    bytes.push(VERSION);
    bytes.extend_from_slice(&payload_len.to_be_bytes());
    bytes.extend_from_slice(payload);
    Ok(bytes)
}
```

- `&[u8]` — a **borrowed slice**: a view into bytes someone else owns. No copy.
- `as u16` — an explicit cast. `len` is `usize`; the header only holds 16 bits.
  The earlier bound check guarantees it fits.
- `Vec::with_capacity(n)` — heap buffer pre-sized to `n`, avoiding reallocations.
- `bytes.push(byte)` appends one byte.
- `extend_from_slice(&[..])` appends a slice.
- `.to_be_bytes()` — converts the `u16` to a 2-element **big-endian** array.
  Big-endian ("network byte order") means the receiver reads the same value on
  any CPU architecture. Little-endian machines would otherwise disagree.
- `Ok(bytes)` — wrap the success value in the `Result`.
- The `return Err(...)` is an early exit; note we return a **typed** enum variant,
  not a string. Errors carry data here.

### The `WireError` enum

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WireError {
    TooShort { len: usize },
    DatagramTooLarge { len: usize, max: usize },
    PayloadTooLarge { len: usize, max: usize },
    UnsupportedVersion(u8),
    LengthMismatch { declared: usize, actual: usize },
}
```

- An **enum with data** (a tagged union). Each variant carries exactly the
  context needed to build a message. This is idiomatic Rust: errors are values,
  not exceptions.
- `TooShort { len }` is a **struct variant** (named fields); `UnsupportedVersion(u8)`
  is a **tuple variant** (positional).
- Derived traits: `Debug` (print), `Clone`, `PartialEq`/`Eq` (compare — which is
  why tests can `assert_eq!` on errors).

### `Display` and `Error`

```rust
impl fmt::Display for WireError { ... }
impl Error for WireError {}
```

- `impl Trait for Type` is how you attach behavior.
- Implementing `Display` lets `WireError` be formatted with `{}` (used in the
  `warn!(error = %err, ...)` logs).
- `impl Error for WireError {}` is an empty impl that marks it as a real error,
  enabling `?` conversion and boxed-error interop.

## `decode`: reading bytes back

```rust
pub fn decode(datagram: &[u8]) -> Result<&[u8], WireError> {
    if datagram.len() > MAX_DATAGRAM_SIZE { return Err(WireError::DatagramTooLarge { .. }); }
    if datagram.len() < HEADER_LEN { return Err(WireError::TooShort { .. }); }

    let version = datagram[0];
    if version != VERSION { return Err(WireError::UnsupportedVersion(version)); }

    let declared = u16::from_be_bytes([datagram[1], datagram[2]]) as usize;
    let body = &datagram[HEADER_LEN..];
    if declared != body.len() {
        return Err(WireError::LengthMismatch { declared, actual: body.len() });
    }
    Ok(body)
}
```

- Returns a **borrowed slice** `&[u8]` into the input — zero-copy. The lifetime is
  tied to `datagram`, so the payload can't outlive the buffer.
- `datagram[0]` indexes a byte (panics if out of range, but we checked length).
- `[datagram[1], datagram[2]]` builds a `[u8; 2]` array for `from_be_bytes`.
- `&datagram[HEADER_LEN..]` is a **range slice**: everything from offset 3 on.
- Order matters: check size → check min length → check version → check declared
  length. Each guard is cheap and prevents a later step from reading bad data.

## Async functions and `.await` (transport.rs)

```rust
pub async fn bind(addr: SocketAddr) -> io::Result<UdpSocket> {
    UdpSocket::bind(addr).await
}
pub async fn send_message(socket: &UdpSocket, dest: SocketAddr, payload: &[u8]) -> io::Result<()> {
    let bytes = wire::encode(payload)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))?;
    socket.send_to(&bytes, dest).await?;
    Ok(())
}
```

- `async fn` returning `io::Result<T>` is tokio convention.
- `.await` yields control to the runtime while the IO operation is in flight,
  instead of blocking the OS thread.
- `.map_err(|err| ...)` — `Result` combinators. `encode` returns `WireError`; the
  map converts it into an `io::Error` so this function has a single error type.
  `|err| ...` is a **closure**.
- `&UdpSocket` — we borrow the socket so the caller keeps ownership.

### The `recv_loop` and `tokio::select!`

```rust
pub async fn recv_loop(socket: UdpSocket, mut shutdown: watch::Receiver<bool>) -> io::Result<()> {
    if *shutdown.borrow() { return Ok(()); }
    let mut buf = vec![0u8; MAX_DATAGRAM_SIZE + 1];
    loop {
        tokio::select! {
            _ = shutdown.changed() => { return Ok(()); }
            received = recv_datagram(&socket, &mut buf) => {
                match received { ... }
            }
        }
    }
}
```

- `mut shutdown` — we mutate it by awaiting `changed()`, which needs `&mut self`,
  so the binding must be `mut`.
- `*shutdown.borrow()` — `borrow()` gives a guard; `*` dereferences it to the
  `bool`. Early return if we're already told to stop.
- `vec![0u8; N]` — a heap buffer of `N` zero bytes.
- `loop { tokio::select! { ... } }` — the core async pattern: block on multiple
  futures, act on whichever finishes first, then loop again.
- `_ = shutdown.changed() => { ... }` — `_` discards the result; we only care
  that it fired. `changed()` resolves when the sender sends a new value (or the
  sender is dropped).
- `received = recv_datagram(...) => ...` — binds the resolved `Result` to
  `received`, then a `match` handles `Ok`/`Err`.

## Slicing and matches (transport.rs)

```rust
Ok((len, from)) => handle_datagram(&buf[..len], from),
Err(err) => warn!(error = %err, "failed to receive datagram"),
```

- `Ok((len, from))` — **destructuring** a tuple inside the `Ok`. `recv_from`
  returns `(usize, SocketAddr)`.
- `&buf[..len]` — slice from 0 up to `len` (exclusive). We ignore whatever is
  past the real datagram length.
- `error = %err` — tracing's **field syntax**; `%` means "format with `Display`".

## The testing syntax (`#[cfg(test)]`, `#[tokio::test]`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_payload() {
        let bytes = encode(payload).expect("encode");
        assert_eq!(decode(&bytes).expect("decode"), payload);
    }
}
```

- `#[cfg(test)]` — the module only compiles under `cargo test`.
- `use super::*;` — pull in everything from the parent module so tests can call
  private functions like the internal `encode`/`decode`.
- `#[test]` — a plain sync test; `#[tokio::test]` (integration tests) makes an
  async test with its own runtime.
- `.expect("msg")` — unwrap a `Result`, panicking with a message on `Err`.
- `assert_eq!` / `assert_ne!` — compare and panic on mismatch.
