# M2 — Concepts

Vocabulary behind the overlay. M1 concepts (UDP, framing, async, ownership) are
in [../05-concepts.md](../05-concepts.md).

## TUN vs TAP

- **TUN** carries **Layer 3** — raw IP packets. This is what `weave` uses.
- **TAP** carries **Layer 2** — Ethernet frames (with MAC addresses).

We want to route IP, so TUN.

## utun (macOS)

On macOS there is no `/dev/net/tun`; interfaces are `utunN`, created through a
PF_SYSTEM control socket. Names must be `utun` + a number; the low-level details
are hidden by the `tun` crate. Linux equivalent is `tun0` etc.

## The two addresses of a tunnel endpoint

A point-to-point TUN has a **local** address (our overlay IP) and a
**destination/peer** address (the other node's overlay IP). The kernel uses the
peer address to install a host route, so traffic for the peer goes out our TUN.

## Packet information (PI) header

utun devices prefix each packet with a small header (4 bytes on macOS) describing
family/protocol. It is an interface detail, not part of the IP packet. The `tun`
crate strips it on read and prepends it on write, so our code sees pure IP.

## IP header fields we check

- **Version** — top nibble of byte 0 (4 or 6).
- **IHL** (IPv4) — bottom nibble of byte 0; header length in 32-bit words
  (min 5 → 20 bytes).
- **Total length** (IPv4, bytes 2–3) — the whole packet size; must equal what we
  actually read.
- **Payload length** (IPv6, bytes 4–5) — bytes after the 40-byte header.

Validating these is how we know a blob of bytes is a packet and not garbage.

## MTU

Largest packet an interface will emit. Ours defaults to 1420 to leave room under
the 1500 Ethernet MTU for the outer UDP + IP headers, avoiding fragmentation on
the physical path. The frame's payload cap (1497) is the hard ceiling.

## Point-to-point link

A link with exactly two ends. No broadcast/ARP needed; a single route to the peer
is enough. Multi-hop routing is deliberately out of scope.

## Backpressure

When a consumer is slower than a producer, something must give: buffer forever
(memory blowup), drop, or slow the producer. M2 uses a **bounded queue** and
makes the producer *await*, which propagates the slowdown upstream to the TUN
read. That's backpressure.

## Deadlock (and how the 4-task layout avoids it)

Deadlock: task A holds/wait-for X and needs Y, task B holds Y and needs X. With
one task per direction doing both a blocking send and a blocking receive, exactly
that can happen when both queues fill. Splitting into four tasks — each with one
input and one output queue, never waiting on two queues — makes a dependency
cycle impossible.

## Datagram semantics

Each `read` from the TUN returns exactly one packet, and each UDP datagram
carries exactly one framed packet. We don't assume ordering or delivery; a lost
datagram is a lost packet, which IP already tolerates.

## Root / capabilities

Opening a TUN and changing interface addresses/MTU requires elevated privileges
(`root`, or specific capabilities on Linux). That's why the real-device test is
`#[ignore]` and the default test suite uses a fake device.

## Routes

With the peer set as the interface destination, the kernel adds a host route to
the peer. To reach a network *behind* the peer you add an explicit route
(`route add` / `ip route add`). M2 documents this rather than automating it.

## Fail-closed validation

Every layer validates before forwarding and drops on failure: frame decode, peer
check, IP header check, size check. One bad packet is logged and skipped; the
tunnel keeps running.
