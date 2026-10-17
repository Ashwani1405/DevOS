# M2 — Intuition

## What changes from M1

M1 could send *our* words over UDP. M2 sends *the kernel's* packets. Instead of
`"hello from 127.0.0.1:4000"`, the payload is a real IP packet that some app on
the machine produced.

The mental model:

```text
app writes bytes
   -> kernel builds an IP packet
      -> it targets an address on our virtual network
         -> the kernel hands the packet to our TUN interface
            -> we ship it over UDP to the peer
               -> the peer's kernel sees it arrive on its TUN
                  -> the peer's app reads bytes
```

We never touch the application. We only move packets the kernel decided to route
to us.

## What is a TUN interface?

A TUN device is a **virtual network interface** that has no hardware behind it.
The kernel treats it like a NIC: you can assign it an IP, bring it up, and add
routes. The difference is what comes out the "cable":

- When the kernel wants to **send** a packet out a TUN, it doesn't touch a wire —
  it hands the raw IP packet to the process that opened the device (a `read`).
- When the process **writes** a packet to the TUN, the kernel acts as if that
  packet just **arrived** on the interface and processes it normally (routing,
  delivery, replies).

So a TUN is a two-way door between "the kernel's IP world" and "our userspace
program". That door is exactly what a VPN needs.

## Why UDP and not TCP underneath

The packets we carry are already IP. If we put them inside TCP, and the outer
TCP has a hiccup, every inner packet stalls behind it — head-of-line blocking.
VPNs carry IP over **UDP** (or raw) so the inner traffic keeps its own timing and
loss behavior. UDP also keeps us free to add crypto/reliability later without
fighting TCP's guarantees.

## Why "point-to-point"

Each utun is created with a local address *and a destination address* (the
peer's overlay IP). The link is a direct line between exactly two endpoints.
That's why a single route to the peer's address is enough to make `ping` work,
and why multi-hop is explicitly out of scope for M2.

## The pipeline, and why it's four tasks

The naive version is one loop: read TUN → send UDP, and read UDP → write TUN.
The catch: if you do both directions in one loop and *block* on a full outbound
queue, you stop draining the inbound queue — and the peer does the same — and
you **deadlock**.

M2 avoids that by splitting into four tasks connected by two queues:

```text
tun reader ──> [outbound] ──> udp sender
tun writer <── [inbound]  <── udp receiver
```

Each queue has one producer and one consumer, and no task ever waits on two
queues at once. There is no cycle, so it cannot deadlock, and a full queue
applies **backpressure** by making the producer wait instead of buffering
forever.

## What we deliberately kept simple

| Choice | Why | Cost |
| ------ | --- | ---- |
| Validate packets, drop the rest | bad input must never crash or corrupt the tunnel | a malformed packet is silently lost |
| Trust only the configured peer | it's a two-node link; anything else is noise | no roaming / NAT rebinding |
| Drop-and-log on bad frames | UDP is lossy anyway | no retransmit |
| MTU default 1420 | leaves headroom under 1500 for outer headers | large packets need fragmentation upstream |
| Root to open the device | the OS requires it for TUN | tests split into root-free + ignored |

## Success looks like

Two nodes, one `ping 10.77.0.2` from the first node, and a reply — with the
packets visible in both nodes' logs. Nothing else in the OS needs to know.
