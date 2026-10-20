# 01 — Intuition (the big picture)

## What are we actually building?

`weave` wants to be a **peer-to-peer Layer-3 overlay network**. That's a fancy
way of saying:

> Build a *virtual* network on top of the *physical* one, so two machines can
> exchange IP packets even when there's no direct cable between them.

Think of it like a tunnel you dig through the internet. The physical path is
messy (routers, NAT, Wi-Fi, distance). The overlay pretends all of that is a
single flat wire between the peers.

## The one-line mental model

```text
app bytes  ->  frame them  ->  push over UDP  ->  peer reads  ->  unframe  ->  app bytes
```

Everything we've written so far is just the two ends of that arrow:

- **Marshalling** bytes into a predictable shape (`wire.rs`).
- **Moving** those bytes across a socket (`transport.rs`).
- **Driving** the process that does the moving (`main.rs`).

## Why these choices (and their trade-offs)

| Choice | Why | What we give up |
| ------ | --- | --------------- |
| **UDP**, not TCP | We want to control reliability ourselves later (real VPNs can't rely on TCP's head-of-line blocking). | No delivery, ordering, or duplicate protection. Datagrams can vanish. |
| **Fixed 3-byte header** | Dead simple to parse and reason about; enough to self-describe a frame. | No room for flags, checksums, or routing info yet. It will change. |
| **1500-byte cap** | Keeps us under a typical Ethernet MTU so we don't lean on IP fragmentation on a LAN. | Large messages must be split later (not implemented). |
| **No crypto yet** | Get the plumbing right first; crypto is a layer on top. | Traffic is plaintext. Local testing only. |

## The "frame" idea

A raw UDP datagram is just bytes with no structure. If we send `"hello"` the
receiver only sees five bytes — it has no idea if that's a whole message, half a
message, or garbage. So we wrap every payload in a tiny envelope:

```text
+---------+----------------+------------------+
| version |  length (2B BE)|     payload      |
|  1 byte |  how big?      |   the real data  |
+---------+----------------+------------------+
```

The header answers two questions the receiver would otherwise have to guess:

1. **Is this a frame I understand?** → `version`
2. **Where does the payload end?** → `length`

This is the same core idea TCP, IP, Ethernet, and basically every wire protocol
uses: a self-describing header followed by a body. Once you internalize
"header + body", you can read almost any protocol.

## The lifecycle of one process

```text
1. parse CLI args        (bind address, optional peer)
2. open a UDP socket     (bind)
3. optionally shout "hello" at the peer once
4. spawn a background task that receives forever
5. sit and wait: Ctrl+C, or the receive task ending
6. on exit, signal the task to stop and wait for it to finish
```

That's it. Two such processes pointed at each other log each other's hello. The
rest of the docs zoom into each of those moments.

## What "done" looks like (and what isn't)

**Done today:** the UDP frame transport plus the layers built on it in later
milestones (TUN overlay, multi-hop routing, topology-driven failover, endpoint
migration, relay fallback, and Noise IK authentication of control messages).

**Not done (intentionally):** encryption of the *data* plane, peer discovery,
and full NAT traversal. The README "Not done yet" list is the current roadmap.
Every remaining item is a *layer on top* of this transport, not a rewrite of it.
