# M6 — authenticated endpoint migration and relay fallback

M5 gave every node a static UDP endpoint known at configuration time. M6 removes
that assumption: a node's UDP endpoint can move (a laptop changes networks, a NAT
rebinds a port), and when two peers cannot reach each other directly they can
fall back to a third node that forwards their traffic. Migration is guarded by
per-peer authentication (originally a group-shared HMAC; now the Noise IK
sessions described below).

## What M6 adds

| Concern | Module | Idea |
| ------- | ------ | ---- |
| Control-message authentication | `src/noise.rs` | per-peer Noise IK handshake; control bodies travel as encrypted records |
| Endpoint candidates and migration | `src/endpoints.rs` | a peer declares a small ranked list of endpoints; a newer epoch wins |
| Reachability and mode selection | `src/connectivity.rs` | per-peer `Direct` vs `Relay` link with failure/success hysteresis |
| Envelope forwarding | `src/relay.rs` | a relay frame wraps a packet with `src`, `dst`, `ttl` |
| Wiring | `src/router.rs`, `src/overlay.rs` | the router authenticates, migrates endpoints and publishes links; the data plane switches to the relay when a link drops |

New wire versions exist alongside the M1 packet frame and the M5 control frame:

| Version | Direction | Purpose |
| ------- | --------- | ------- |
| `1` | data | raw IP packet (`wire::encode`) |
| `2` | control | control body, encrypted when a session exists (`wire::encode_control`) |
| `3` | relay | relay envelope (`wire::encode_relay`) |
| `4` | handshake | Noise handshake message (`wire::encode_handshake`) |

## Endpoint migration

Each node has a logical identity (`NodeId`, its overlay IP) that never changes,
and a physical UDP endpoint that can. The endpoint registry (`EndpointRegistry`)
holds, per peer:

- the currently active endpoint (the last one confirmed reachable),
- a bounded, ranked list of *candidate* endpoints (the peer's own advertised
  endpoints plus observed sources),
- a monotonic `epoch`, so a delayed `EndpointUpdate` from an older epoch is
  rejected and cannot roll the active endpoint back.

A peer confirms an endpoint by sending from it (a `Punch` or any authenticated
control message). The registry then makes that endpoint active and, if the peer
had moved, drops the previously active address from the accept set — so a peer
that migrates repeatedly does not leak entries.

Candidates are filtered: unspecified addresses (`0.0.0.0`, `::`) and duplicates
are removed, and a declaration with no usable candidate is rejected
(`EndpointReject`). The number of candidates per peer is capped.

## Direct vs relay

`Connectivity` tracks, per peer, whether the current path is `Direct(addr)` or a
`Relay(addr)`, and a mode (`Mode`) describing why. The rules are deliberately
conservative:

- a direct send that succeeds promotes the peer to `Direct`,
- repeated direct failures demote it,
- a relay is only used as a fallback for a peer we know but cannot reach
  directly.

The data plane (`overlay.rs`) consults the published `LinkSnapshot`: a link is
either `Direct(addr)` (send the packet straight there) or `Relay(addr)` (wrap it
in a relay envelope). With no link table (M5 behaviour) it sends to the routing
table's next hop.

## Relay envelopes

`relay::Envelope` is `[src NodeId][dst NodeId][ttl 1B][inner packet]`, encoded as a
version-3 frame. A relay forwards an envelope only when **all** of these hold:

1. this node was configured to serve as a relay (`--serve-relay`),
2. the sender is a known peer (`PeerSet::is_known`),
3. the envelope's claimed `src` does not impersonate a peer we track at a
   *different* endpoint.

The last check is the source binding added in M7. A known peer could otherwise
wrap traffic claiming to originate from another peer we can reach directly. A
`src` we do not track at all is allowed, because it cannot be impersonating a
tracked peer. Forwarded envelopes have their `ttl` decremented; a `ttl` of zero
is dropped (`RelayDrop`). The relay never decodes the inner packet beyond routing
it.

## Authentication

Control messages are protected by a **Noise IK** handshake
(`Noise_IK_25519_ChaChaPoly_BLAKE2s`, via `snow`) between configured peers. Each
node has a long-term X25519 identity (`--identity <hex>`), and every neighbour's
static public key is configured up front (`--peer-key <IP>=<hex>`). The node with
the lexicographically smaller `NodeId` is the initiator, so exactly one side
dials; a handshake is retried on the heartbeat until a session exists.

After the handshake, control bodies are carried as encrypted records: a random
12-byte nonce followed by a ChaCha20-Poly1305 ciphertext+tag. Direction keys are
derived from the handshake hash with distinct labels, so the two sides never
share a key. The custom record layer is used instead of `snow::TransportState`
because the latter's implicit counter nonce desynchronises under UDP loss and
reordering.

A control datagram that does not decrypt under a known session increments
`unauthorized` and is dropped before `control::decode`. A handshake from a static
key that is not in `peer_keys` is rejected. Because the record is authenticated,
a peer can also migrate its UDP endpoint: a record that decrypts under a peer's
session is accepted from a new source address and the binding is updated.

Properties and limits, stated plainly:

- **what it gives:** per-peer authentication (a peer must prove possession of its
  static key), confidentiality of control bodies, forward secrecy from the
  ephemeral X25519 exchange, and replay/reorder tolerance.
- **what it does not give:** data-plane confidentiality (packets are still
  plaintext), NAT traversal beyond endpoint candidates + relay, or trust in a
  *configured* peer that turns malicious after authenticating. A compromised
  long-term key is not detected.

## CLI

```text
--identity <HEX>          this node's long-term private key (enables sessions)
--peer-key <IP=HEX>       a neighbour's static public key, repeatable
--relay <ADDR:PORT>       relay endpoint to fall back to
--serve-relay             forward relay envelopes for neighbours (run as a relay)
```

`--relay` without `--identity` logs a warning and keeps migration disabled: only
configured endpoints are accepted. With `--identity` every neighbour needs a
`--peer-key`; a neighbour without one cannot complete a handshake and is
therefore unreachable.

## Tests

- `tests/connectivity.rs` — relay carries traffic when the direct link is dead;
  a peer cannot relay on behalf of another peer (the M7 source-binding test).
- `src/router.rs` — handshake completes between two routers; a handshake from an
  unknown key is rejected; a forged/tampered record is rejected; stale endpoint
  updates are ignored; repeated endpoint churn stays bounded.
- `src/noise.rs` — handshake round trip, tamper rejection, third-party rejection,
  loss/reorder tolerance.
- `src/endpoints.rs` — unspecified candidates are filtered and rejected.
- `src/relay.rs` — encode/decode round trips, TTL exhaustion.
