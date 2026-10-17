# M2 — Syntax flow

New Rust/tokio syntax introduced by M2, in reading order. The M1 syntax primer is
[../03-syntax-flow.md](../03-syntax-flow.md).

## Async methods in traits (RPITIT)

```rust
pub trait PacketReader: Send + 'static {
    fn recv<'a>(
        &'a mut self,
        buf: &'a mut [u8],
    ) -> impl Future<Output = io::Result<usize>> + Send + 'a;
}
```

- A trait method can return `impl Future` (a "return-position impl trait in
  trait"). The concrete future type is decided by each implementor.
- `+ Send` on the return is the whole reason we don't just write `async fn`:
  the tasks are `spawn`ed, so every future they await **must** be `Send`.
  `async fn` in a trait does not promise that, so we spell it out.
- `'a` ties the returned future to the borrows of `self` and `buf`, so the
  borrow checker knows the future can't outlive them.
- `Send + 'static` on the trait bounds make implementors usable across tasks.

Implementing it:

```rust
#[allow(clippy::manual_async_fn)] // need the explicit `Send` bound for spawning
impl PacketReader for TunReader {
    fn recv<'a>(&'a mut self, buf: &'a mut [u8])
        -> impl Future<Output = io::Result<usize>> + Send + 'a {
        async move { tokio::io::AsyncReadExt::read(&mut self.reader, buf).await }
    }
}
```

- `async move { ... }` blocks capture `self` and `buf`; the block *is* the
  returned future. `clippy::manual_async_fn` flags this pattern and we allow it
  because the explicit `Send` bound is required.

## Generics over the two halves

```rust
pub async fn run<R, W>(reader: R, writer: W, ...) -> io::Result<()>
where
    R: PacketReader,
    W: PacketWriter,
```

- `R`/`W` are type parameters, resolved at the call site. `main` passes the real
  `TunReader`/`TunWriter`; tests pass fakes. Same code, no `dyn`.

## Sharing the socket with `Arc`

```rust
let socket = Arc::new(socket);
tasks.spawn(queue_to_udp(outbound_rx, socket.clone(), ...));
tasks.spawn(udp_to_queue(socket, inbound_tx, ...));
```

- Tokio's `UdpSocket::recv_from`/`send_to` take `&self`, so two tasks can share
  one socket. `Arc<UdpSocket>` gives each task an owned, thread-safe handle; the
  socket closes when the last `Arc` drops.

## Bounded channels = backpressure

```rust
let (outbound_tx, outbound_rx) = mpsc::channel(config.queue_depth);
...
outbound.send(buf[..len].to_vec()).await   // waits when the queue is full
```

- `mpsc::channel(n)` is **bounded**: at most `n` buffered values.
- `Sender::send(...).await` resolves immediately if there's room, otherwise it
  *waits*. That waiting is what slows a fast producer to the consumer's pace.
- `Receiver::recv().await` returns `None` once all senders are dropped, which the
  tasks use as "producer gone, stop cleanly".

## `watch` as a shutdown signal

```rust
let (stop_tx, stop_rx) = watch::channel(false);
...
_ = stop.changed() => { return Ok(()); }
```

- `watch` broadcasts the **latest** value. `changed().await` resolves when the
  value changes (or the sender is dropped, yielding `Err`).
- Binding the result to `_` in `select!` means we ignore which it was — either
  way, "something happened, stop".

## `tokio::select!` race

```rust
tokio::select! {
    _ = stop.changed() => { return Ok(()); }
    result = reader.recv(&mut buf) => { /* ... */ }
}
```

- Polls both futures in one task; runs the branch that completes first.
- `result = future => ...` binds the future's output. The other branch's future
  is dropped (cancelled) for that iteration; the loop rebuilds it next time.
- Both branches borrow different variables (`stop` vs `reader`/`buf`), so no
  borrow conflict.

## `JoinSet` for the task group

```rust
let mut tasks = JoinSet::new();
tasks.spawn(tun_to_queue(reader, outbound_tx, stop_rx.clone(), config.mtu));
// ... 3 more ...
while let Some(joined) = tasks.join_next().await {
    match joined { Ok(Ok(())) => {}, Ok(Err(e)) => { /* keep first */ }, Err(e) => { /* panic */ } }
    let _ = stop_tx.send(true);   // any finish stops the rest
}
```

- A `JoinSet<T>` owns spawned tasks that all return the same type `T` (here
  `io::Result<()>`). `join_next().await` yields whichever finished next, or
  `None` when the set is empty.
- The double `Result`: `Ok(Ok(()))` = task returned `Ok`; `Ok(Err(e))` = task
  returned an error; `Err(JoinError)` = task panicked/was cancelled.
- Calling `stop_tx.send(true)` on *every* completion means one failure or one
  clean exit winds down the rest.

## Reading/writing packets (`AsyncReadExt`/`AsyncWriteExt`)

```rust
async move { tokio::io::AsyncReadExt::read(&mut self.reader, buf).await }
async move { tokio::io::AsyncWriteExt::write_all(&mut self.writer, packet).await }
```

- `read` fills the buffer once (one packet from a TUN) and returns its length.
- `write_all` loops until the whole packet is written, so a partial write can't
  cut a packet in half.
- Calling them as `AsyncReadExt::read(&mut x, ...)` avoids importing the trait
  into scope.

## Parsing IP headers with byte math

```rust
let ihl = packet[0] & 0x0f;                 // low nibble
let version = first >> 4;                    // high nibble
let declared = usize::from(u16::from_be_bytes([packet[2], packet[3]]));
```

- `>> 4` / `& 0x0f` split the two nibbles of the first byte.
- `u16::from_be_bytes([a, b])` turns two bytes into the big-endian integer that
  IP uses for lengths. `usize::from(...)` widens for index math.

## Building errors with `ok_or_else`

```rust
let peer = args.peer.ok_or_else(|| missing("--peer is required together with --tun"))?;

fn missing(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.to_owned())
}
```

- `Option::ok_or_else(closure)` converts `None` into an `Err`, lazily.
- Returning `io::Error` (not `&str`) keeps the `?` → `Box<dyn Error>` conversion
  unambiguous.

## Platform-specific config

```rust
tun_config.platform_config(|platform| {
    platform.enable_routing(config.enable_routing);
});
```

- `platform_config` takes a closure that mutates the platform struct (macOS-only
  fields like `enable_routing`), keeping the shared builder cross-platform.
