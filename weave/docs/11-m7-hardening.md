# M7 — hardening, fault injection and measurement

M7 is not a feature milestone. It audits the code built in M1–M6, fixes concrete
correctness and security defects, and measures behaviour under faults, load and
resource pressure — without inventing results.

The rules this milestone follows:

- every claim below is backed by a test that exists in the repo or by a command
  you can run yourself;
- no fix is claimed unless it is in the tree and covered by a passing test;
- no number is reported that was not produced by `cargo bench` / `cargo test` on
  a stated machine;
- features are not added just to make a milestone look complete. Where something
  is missing it is listed under **Limitations**.

All commands were run on: macOS 15.5, arm64 (Apple Silicon), rustc 1.91.1.

## Implementation audit

| Milestone | State found | Evidence |
| --------- | ----------- | -------- |
| M1 transport | done, committed | `src/transport.rs`, `src/wire.rs` |
| M2 TUN overlay | done, committed | `src/tun.rs`, `src/overlay.rs` |
| M3 peer lifecycle | **added in the follow-up work** | `src/lifecycle.rs`: Connected/Suspect/Disconnected state machine, probes and backoff; wired into `src/router.rs` |
| M4 routing | done, committed | `src/route.rs`, `src/forward.rs` |
| M5 topology | done, committed | `src/topology.rs`, `src/router.rs` |
| M6 migration/relay/auth | present in the working tree, previously uncommitted; docs were missing | `src/connectivity.rs`, `src/endpoints.rs`, `src/relay.rs`; `docs/10-m6-migration-relay.md` added in M7 |
| M6 auth rework | **shared-key HMAC replaced by Noise IK** | `src/noise.rs`; the old `src/auth.rs` is removed |

Baseline static checks before M7 touched anything: `cargo fmt --check` failed (the
tree was unformatted), `cargo clippy -D warnings` failed on one lint, and one
test failed. All three are fixed.

## Findings and fixes

### F1 — a stale advertisement could resurrect a failed link (correctness)

**Symptom.** `tests/topology.rs::a_delayed_advert_cannot_resurrect_a_failed_link`
failed: a node that had declared its own link down accepted an old, reordered
advertisement from the neighbour and marked the link live again.

**Cause.** `Router::handle_advert` counted *any* accepted advertisement as
proof of liveness, including one it had classified as stale.

**Fix.** `handle_advert` now returns a `bool` — `true` only for `Applied::New` /
`Applied::Refreshed`, `false` for forged, `Stale` or `Invalid`
(`src/router.rs:520`). Liveness is recorded only when that flag is set, through a
single `mark_live` helper (`src/router.rs:381`). Regression test:
`src/router.rs::a_stale_advert_does_not_resurrect_a_failed_link`.

### F2 — node-limit bypass in the topology (resource)

**Cause.** `Topology::apply` counted only the advertisement's *origin* against
`Limits::max_nodes`, so an advertisement introducing many *head* nodes could grow
the graph past the limit.

**Fix.** Both newly introduced nodes are counted before insertion and the
advertisement is rejected with `LimitReached` if they would overflow
(`src/topology.rs:424`). Regression test:
`src/topology.rs::head_nodes_count_against_the_node_limit`.

### F3 — malformed IPv4 header length fed to a slice index (robustness)

**Cause.** `packet::decrement_hop_limit` read `ihl` and then indexed the packet
without re-validating it, so a packet with an inconsistent header length could
panic on a slice index.

**Fix.** The function now checks `ihl >= 5` and `header_len <= packet.len()`
before touching the header, returning `false` on a bad packet
(`src/packet.rs:129`). Regression test:
`src/packet.rs::decrementing_rejects_a_malformed_header_len_without_panicking`.

### F4 — relay could be used to impersonate a tracked peer (security)

**Cause.** In `handle_relay` the only sender checks were "known peer" and "is a
relay", so a known peer could wrap traffic claiming `src` was a *different* peer.

**Fix.** The claimed source is now bound: if `src` is a peer we track as a direct
link, the envelope must have arrived from that peer's endpoint; a `src` we track
as relay-only is refused; a `src` we do not track is permitted because it cannot
impersonate a tracked peer (`src/overlay.rs:392`). Regression test:
`tests/connectivity.rs::a_peer_cannot_relay_on_behalf_of_another_peer`.

### F5 — unspecified endpoint candidates accepted (robustness)

**Cause.** `EndpointRegistry::declare` accepted `0.0.0.0` / `::` candidates, so a
node could advertise an unusable endpoint and win an epoch.

**Fix.** `bounded_candidates` strips unspecified addresses and duplicates
(`src/endpoints.rs:267`); `declare` computes the candidate list once and rejects
an empty result with `EndpointReject::NoCandidates`. Regression test:
`src/endpoints.rs::unspecified_candidates_are_filtered_and_rejected`.

### F6 — static-check debt

`cargo fmt --all` applied; the clippy `unwrap_or_else(|| Vec::new())` was changed
to `unwrap_or_default()` in `src/overlay.rs`. `cargo fmt --check` and
`cargo clippy --all-targets --all-features -- -D warnings` now pass.

## Follow-up fixes (same working tree)

After the M7 audit, four more defects/gaps were closed. They are listed here
because they change claims made earlier in this document.

### F7 — control messages were never actually sealed on the send path

**Cause.** The M6 authentication was applied in `handle_body` (receive) but the
sender in `router::send_all` called `control::encode` + `transport::send_control`
directly. A real node therefore sent plaintext while every receiver rejected it;
the tests only passed because they sealed messages by hand.

**Fix.** The whole control path was reworked around Noise IK sessions
(`src/noise.rs`, `src/router.rs`). `Router::send_control` looks up the peer's
session and seals every control body; a message with no session is dropped rather
than sent in the clear. Handshake messages travel in a new version-4 frame
(`wire::HANDSHAKE_VERSION`) and are the only control-plane traffic sent without a
session. Regression test:
`src/router.rs::two_routers_complete_a_handshake_and_exchange_control`.

### F8 — no per-source control-plane rate limiting

**Fix.** `src/rate.rs` implements per-source and global token buckets with
bounded source tracking (default burst 64 / 32 per second per source, 4096 / 1024
global, 1024 tracked sources, 30 s idle eviction). It runs at the very top of
`handle_body`, before any crypto, so a flood cannot force AEAD work. `RouterStats`
exposes `rate_limited`; `--control-rate-limit` sets the per-source rate (0
disables). Tests: `src/rate.rs` unit tests and
`src/router.rs::a_control_flood_is_rate_limited`.

### F9 — M3 was only a liveness stand-in

**Fix.** `src/lifecycle.rs` adds an explicit `PeerState`
(`Connected`/`Suspect`/`Disconnected`) driven by a `ProbeConfig` derived from the
heartbeat and link timeout: a quiet peer goes `Suspect` after half the timeout,
`Disconnected` at the timeout, and probes are retried with exponential backoff.
The router consumes its `Action`s to mark links down/up and to emit `LinkRequest`
probes. Regression test:
`src/router.rs::the_prober_restores_a_link_that_timed_out`.

### F10 — no fuzzing

**Fix.** `tests/fuzz.rs` is a deterministic, seeded mutational fuzzer (no
nightly, no `cargo-fuzz`) over the wire, packet, control, relay, Noise and
topology decoders, ~20 000 iterations each. It asserts only that hostile input
never panics and that records/handshakes never authenticate when mutated. It runs
on stable and in CI.

### F11 — one punch could bounce forever between two neighbours

**Cause.** A `Punch` was echoed whenever its `origin` equalled the sender's
identity, and an echo was itself a `Punch` satisfying the same condition, so
two configured neighbours could reflect one message back and forth indefinitely.

**Fix.** A router tracks the punch tokens it originated and the tokens it has
already echoed. An echo of a token we sent is consumed and not echoed again; a
foreign token is echoed at most once (`src/router.rs`). Regression test:
`src/router.rs::a_punch_is_not_echoed_twice`.

## Security review

Assets: the control plane (topology, endpoints) and the data plane (packets from
peers). Trust boundary: a peer that has completed a Noise IK handshake with a
configured static key is trusted to speak for that key; anything else is not.

| Threat | Mitigation | Residual |
| ------ | ---------- | -------- |
| Forged control message | Noise IK + ChaCha20-Poly1305 records; a record must decrypt under a known session (`src/noise.rs`, `src/router.rs`) | a compromised long-term key is not detected |
| Endpoint hijack / replay | monotonic epochs; older updates rejected (`src/endpoints.rs`); records only accepted after authentication | — |
| Relay impersonation | source binding (`src/overlay.rs:392`, F4) | an untracked `src` is still allowed (it can't impersonate a tracked peer) |
| Resource exhaustion via control flood | per-source and global token buckets (`src/rate.rs`), bounded control body size, bounded candidates, node/link limits (`src/topology.rs`, `src/control.rs`) | — |
| Resource exhaustion via data flood | bounded queues (`queue_depth`); overload backpressures | — |
| Malformed datagram | frame validation (`src/wire.rs`); malformed IPv4 header rejected (F3); deterministic fuzzing (`tests/fuzz.rs`) | — |
| Eavesdropping | control bodies encrypted and authenticated; data packets are **not** encrypted | data-plane confidentiality is out of scope |

Authentication is now **per peer**, not a shared group key: a peer must prove
possession of its static X25519 key, control bodies are confidential, and the
ephemeral exchange gives forward secrecy. The data plane is still plaintext.

## Fault injection and recovery

Deterministic simulation (`tests/topology.rs`) drives a four-node fabric
(1–2–4 and 1–3–4) with a seeded RNG and no real time. Injected faults:

- a link going down and the primary route ageing out
  (`a_failed_link_moves_traffic_to_the_alternate_path`),
- a delayed/queued advertisement arriving after a deadline
  (`a_delayed_advert_cannot_resurrect_a_failed_link`),
- a full fabric partition and heal:
  `a_temporary_partition_heals_after_it_ends` — total loss for longer than the
  link timeout drops every route (`route == None`), and after the loss ends the
  topology **reconverges on its own**.

Real-socket tests cover the same transitions over UDP:
`tests/connectivity.rs::relay_carries_traffic_when_the_direct_peer_is_unreachable`,
and the failover tests in `tests/topology.rs`.

Recovery is *eventual*: there is no leader election or instant convergence, and
this is not claimed.

## Performance

`cargo bench --bench baseline` (`benches/baseline.rs`, `harness = false`, no
benchmarking dependency). Numbers below are one run on the machine named at the
top; treat them as an order-of-magnitude baseline, not a spec.

Pure paths:

```text
wire::encode/decode 64B           20.1 ns/op
wire::decode 64B                   1.9 ns/op
wire::encode/decode 512B          52.2 ns/op
wire::decode 512B                  1.9 ns/op
wire::encode/decode 1400B         37.7 ns/op
wire::decode 1400B                 1.9 ns/op
packet::validate 64/512/1400B      2.2 / 2.2 / 2.2 ns/op
packet::decrement_hop_limit        25.7 / 61.0 / 41.0 ns/op
route::lookup (1000 host routes) 859.9 ns/op
noise::seal (96B control body)  1844.0 ns/op
noise::open (96B control body)   814.1 ns/op
topology::routes (128-node chain) 30347.0 ns/op
```

End-to-end overlay over loopback UDP, direct and through one forwarding hop:

```text
direct  64B   latency median 23.8 us  p95 70.3 us   throughput 211692 pkt/s   108.4 Mbps
direct 512B   latency median 28.7 us  p95 51.1 us   throughput 216546 pkt/s   887.0 Mbps
direct 1400B  latency median 15.9 us  p95 41.8 us   throughput 213530 pkt/s  2391.5 Mbps
2-hop   64B   latency median 24.8 us  p95 51.8 us   throughput  47981 pkt/s    24.6 Mbps
2-hop  512B   latency median 36.3 us  p95 85.0 us   throughput  46051 pkt/s   188.6 Mbps
2-hop 1400B   latency median 30.5 us  p95 67.0 us   throughput  42018 pkt/s   470.6 Mbps
```

The 2-hop numbers are roughly 4–5× lower throughput than direct, which is the
expected cost of a second userspace pipeline (two sockets, two fake-TUN hops) per
packet rather than anything in the forwarding decision itself.

**Caveats.** Loopback on a single machine is not a real network: it excludes
propagation delay, loss and reordering, and includes local scheduler jitter (the
p95 spread). Throughput is bounded here by the fake TUN channel and by loopback,
not by the overlay's design. Forwarding uses bounded queues and may drop under
the throughput flood, so the harness counts delivered packets and computes the
rate over those. The numbers are not compared against any other overlay because
no matched-hardware, matched-MTU baseline was produced.

## Resources and lifecycle

- **Bounded queues.** Data and control use `mpsc::channel(queue_depth)`, so a
  flood is dropped/backpressured rather than buffered without limit
  (`src/overlay.rs`). Test:
  `tests/routing.rs::sustained_overload_on_a_bounded_queue_does_not_wedge`
  (1000 packets into a node whose next hop is a black hole; the node still stops
  cleanly).
- **Bounded state.** A peer that migrates 200 times leaves the accept set at two
  entries (configured + current): `src/router.rs::repeated_endpoint_churn_keeps_state_bounded`.
  Candidate lists, node count and link count are all capped.
- **Clean start/stop.** Starting and stopping the overlay ten times, including
  with a packet in flight, never leaks a task, hangs or errors:
  `tests/routing.rs::repeated_startup_and_shutdown_is_clean`.

## Automated tests

`cargo test` after the follow-up work:

```text
lib (unit)      148 passed
connectivity      3 passed
fuzz              1 passed   (deterministic mutational fuzzing)
routing           6 passed
topology          9 passed
transport         6 passed
tun_open          1 ignored   (needs root; not run in CI)
doc-tests         0
```

Every fix F1–F11 has a dedicated regression test named in its section. The suite
uses random free ports, a fake TUN and short timeouts; the topology simulation is
deterministic and does not sleep on real time, so it is not flaky. CI
(`.github/workflows/weave.yml`) runs fmt, clippy, the full test suite and the
benchmark harness on stable.

## Limitations (not claimed to be solved)

- data-plane packets are unencrypted; only control-plane traffic is confidential;
- authentication is per-peer Noise IK, but a compromised long-term key is not
  detected (no revocation, no rotation);
- NAT traversal is heuristic (endpoint candidates + relay), not a full
  ICE-style negotiation;
- convergence is eventual, not strongly consistent or instantaneous;
- the relay forwards opaque envelopes and is not authenticated as a relay beyond
  being a known peer;
- loopback benchmarks are not a real-network measurement.

## Reproduce

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo bench --bench baseline
```
