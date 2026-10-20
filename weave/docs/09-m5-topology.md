# M5 — self-healing and route convergence

How peers exchange link state, how routes are calculated from it, and how the
overlay fails over to an alternate path when a link dies.

## Where M1–M4 actually stand

| Milestone | Status | Notes |
| --------- | ------ | ----- |
| M1 UDP transport | done | `wire.rs`, `transport.rs`, message mode. |
| M2 TUN overlay | done | `tun.rs`, `overlay.rs`, two-node forwarding. |
| M3 peer lifecycle | added after M5 | `src/lifecycle.rs` implements the probe state machine; the control plane now consumes its actions. |
| M4 multi-hop routing | done | static routes, destination-aware forwarding, TTL. |
| M5 topology + convergence | this milestone | link-state exchange, dijkstra, failure detection, failover. |

M5 was written before M3 existed, so the control plane derived liveness from the
periodic link advertisements it already sends: a peer that goes quiet for
`link_timeout` was treated as down. That signal now lives in
`src/lifecycle.rs`: a `Connected`/`Suspect`/`Disconnected` state machine with
probes and exponential backoff, which the router drives and consumes. The
heartbeat/timeout semantics are unchanged.

## Modules

```text
src/topology.rs  the graph: nodes, directed links, validation, expiry, dijkstra
src/control.rs   wire encoding for the link-state messages
src/router.rs    the control plane: flood, detect, recompute, publish
src/route.rs     + RouteSnapshot, a swappable handle to the routing table
src/wire.rs      + control frames (version 2) alongside packet frames (version 1)
src/overlay.rs   + a control channel from the udp receiver to the router
```

Topology state is deliberately separate from the forwarding table. `topology.rs`
describes the network and is pure; `route.rs` holds the derived table and
`forward.rs` uses it. Route calculation never touches sockets, and packet
forwarding never touches the graph.

## Topology model

- `NodeId(Ipv4Addr)` — a node's stable identity: its overlay address.
- `LinkAdvert` — one **directed** link, originated by the node at its tail:
  `{ origin, origin_addr, seq, from, to, metric, state, ttl }`.
  - `origin_addr` is the origin's udp endpoint, so every node learns how to
    reach the origin even for flooded (transitive) advertisements.
  - `state` is `Up` or `Down`.
  - `seq` is the origin's per-link revision, incremented on every advertisement
    the origin sends (see "freshness" below).
  - `ttl` bounds how many more hops the advertisement may flood.
- The graph is directed: a path `a → b → d` needs `a`'s advert for `a→b` and
  `b`'s advert for `b→d`. Each node originates its own outgoing links, so the
  view is consistent without anyone describing links it does not own.
- `Topology` stores local links (authoritative, self-originated) separately from
  remote links (learned), plus a `NodeId → SocketAddr` endpoint map. It keeps
  `Instant`s for freshness but never reads the clock itself: callers pass `now`.

## Topology exchange

- Every `heartbeat` (default 1s) a node re-originates an advertisement for each
  local link and sends it to every neighbour. Each advertisement gets a new
  `seq`.
- A node that receives a *new* advertisement (`seq` greater than any it has for
  that link) re-floods it to its other neighbours with `ttl - 1`, until `ttl`
  hits 0.
- Duplicates and delayed messages do not re-flood.
- A fresh node starts by sending a `LinkRequest` to each neighbour; the
  neighbour replies with a unicast snapshot of everything it knows (ttl 0), so a
  joiner converges quickly.

### Validation

`Topology::apply` rejects:

- a `Down`-marked origin we did not expect, e.g. our own advertisement looping
  back (`SelfOriginated`);
- an advertisement whose origin is not the tail of the link it describes
  (`OriginMismatch`) — a peer may only describe its own outgoing links;
- zero metrics (`ZeroMetric`) and unspecified addresses (`UnspecifiedNode`);
- anything past the node or link caps (`LimitReached`).

Control messages are decoded by `control.rs`, which length-checks and bounds
every message and rejects unknown kinds, bad state bytes and IPv6 endpoints.
The overlay drops datagrams from any udp source that is not a configured peer
before they reach the router, and the router ignores control messages from
addresses that are not its neighbours.

### Resource limits

- `Limits { max_nodes: 1024, max_links: 8192 }` caps stored state.
- `MAX_CONTROL_BYTES = 256` caps a single control message; the frame cap is the
  existing 1497-byte payload limit.
- Flooding is bounded by `max_flood_ttl` (default 4) and by the heartbeat rate.

## Freshness and versioning

This is the heart of the convergence model.

- `seq` is a per-link revision, bumped on **every** advertisement the origin
  sends — each heartbeat is a new revision. It starts from wall-clock
  milliseconds so a restarted node originates numbers above what its peers last
  saw.
- On receive:
  - `seq < stored` → **stale**, rejected. A delayed message cannot overwrite
    newer state, and cannot refresh freshness.
  - `seq == stored` → **duplicate**. If it conflicts with the stored state it is
    rejected; if it matches and the link had expired, it restores the link (a
    returning peer); while the link is already fresh it changes nothing.
  - `seq > stored` → accepted. Freshness resets. If the state or metric changed
    (or the link had expired), the usable graph changed, so routes are
    recomputed and the advert is re-flooded; otherwise it was just a heartbeat
    and nothing is recomputed.
- Freshness: a remote link with no accepted advertisement within `link_timeout`
  is **expired** — kept in the table (so its sequence number still blocks older
  updates) but treated as `Down` for routing. Local links are marked `Down`
  when their neighbour times out, and `Up` again when any control message
  arrives from it.

Time is passed into `apply`/`expire`, so all of this is deterministic in tests.

## Route calculation

`Topology::routes` builds the directed adjacency of usable links (local `Up`
links and non-expired remote `Up` links) and runs **Dijkstra** from the local
node. For each reachable node it records the first hop and emits a dynamic host
route `NodeId.0/32 → endpoint(first hop)` with the path cost as metric.

- Unreachable destinations simply have no route; `RoutingTable::lookup` returns
  `None`, and `forward::decide` drops with `NoRoute`. Unreachability is explicit.
- Equal-cost ties break on the lower next-hop `NodeId`, independent of heap pop
  order.
- Route calculation is independent of forwarding.

### Publication

`RouteSnapshot` wraps `Arc<RwLock<Arc<RoutingTable>>>`. The router publishes a
whole new immutable table with one swap; forwarding tasks call `.current()` and
get a consistent snapshot. Base (static) routes from `--route` are kept in every
published table alongside the dynamic ones.

Recomputation only happens when the usable graph actually changed (a `New`
apply, an expiry, or a local link change) — unrelated heartbeats do not trigger
it. `RouterStats::recalculations` counts publishes.

## Failure detection and recovery

`Router::tick` (every heartbeat) and `Router::handle` (every message) together:

1. expire remote links older than `link_timeout`;
2. mark a silent neighbour's local link `Down`, set `PeerSet::set_available`
   false, and flood a `Down` advertisement;
3. recompute and publish if the graph changed;
4. forward subsequent packets through the next hop the new table picks.

Recovery is the reverse: any control message from a neighbour restores the local
link and re-floods it; a returning neighbour's advertisement restores its remote
links. Routes come back only once the link is usable again.

Not every failure is recoverable: if the remaining network has no path to a
destination, there is no route and traffic is dropped (`NoRoute`), by design.

## Convergence model and limitations

- Eventual, not strong, consistency. Different nodes can briefly disagree about
  the topology while advertisements are in flight; routes converge within about
  one heartbeat plus one flood per hop.
- No guarantee of convergence order; a transient loop of routes is possible but
  bounded. TTL and the "re-flood only new info" rule prevent full-mesh floods.
- A delayed duplicate that arrives after its link has expired but before a newer
  revision is seen can re-extend freshness. This is the fundamental ambiguity of
  using the same message for heartbeats and state; it is bounded by the flood
  TTL and the heartbeat interval.
- A `Down` link always carries a higher `seq` than the `Up` it replaced, so a
  delayed `Up` cannot resurrect a withdrawn route.
- Restart safety relies on wall-clock sequence numbers; clock skew between nodes
  could let an old revision look new. There is no authenticated ordering.

### Security

Control messages are **not** authenticated. Only neighbours' udp source
addresses are trusted, and a node may only describe links whose tail it owns.
That stops an unrelated observer from injecting arbitrary links, but a
configured-but-malicious peer, or anyone able to spoof a neighbour's source
address, can still lie about the topology. Do not treat the topology as
authoritative until M6 adds authentication. There is no custom cryptography
here.

## Observability

- Structured `tracing` logs: `published routing table` on each recompute,
  `neighbour link timed out`, `neighbour link recovered`, and rejections.
- `Router::topology().link_views()` — every link's advertised and effective
  state, metric, freshness and sequence.
- `Router::routes()` — the current routing table and its next hops.
- `Router::stats()` — `applied`, `stale`, `invalid`, `recalculations`, `floods`.
- `Router::topology().version()` bumps on every usable-graph change, so a
  controller can watch convergence.

Recovery time is the interval between the last message from a peer and the
timeout that reroutes around it; convergence time is the interval between a
graph change and the published table. Both are directly measurable from the sim
tests below by comparing simulated timestamps.

## Running a topology by hand

Four nodes with two paths: `A—B—D` and `A—C—D`. Overlay addresses `10.0.0.1..4`,
udp endpoints `127.0.0.1:4001..4004`. Each node lists its neighbours with
`--neighbor <overlay-ip>=<addr:port>`; routes are calculated, not configured.

```sh
# A: neighbours B and C
cargo run -- --bind 127.0.0.1:4001 --tun --address 10.0.0.1 --peer-address 10.0.0.2 \
  --neighbor 10.0.0.2=127.0.0.1:4002 --neighbor 10.0.0.3=127.0.0.1:4003

# B: neighbours A and D
cargo run -- --bind 127.0.0.1:4002 --tun --address 10.0.0.2 --peer-address 10.0.0.1 \
  --neighbor 10.0.0.1=127.0.0.1:4001 --neighbor 10.0.0.4=127.0.0.1:4004

# C: neighbours A and D
cargo run -- --bind 127.0.0.1:4003 --tun --address 10.0.0.3 --peer-address 10.0.0.1 \
  --neighbor 10.0.0.1=127.0.0.1:4001 --neighbor 10.0.0.4=127.0.0.1:4004

# D: neighbours B and C
cargo run -- --bind 127.0.0.1:4004 --tun --address 10.0.0.4 --peer-address 10.0.0.2 \
  --neighbor 10.0.0.2=127.0.0.1:4002 --neighbor 10.0.0.3=127.0.0.1:4003
```

`--heartbeat-ms` and `--link-timeout-ms` tune convergence versus overhead.
Static `--route`s still work and are kept alongside the calculated ones.

To watch recovery: ping `10.0.0.4` from `10.0.0.1`, kill B, and the pings
resume over C within roughly `link_timeout` plus a heartbeat. Requires root
(it opens TUN interfaces).

## Tests

```sh
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

Unit tests cover: graph building, longest-path next-hop selection, unreachable
destinations, stale/duplicate/conflicting updates, delayed updates not restoring
stale routes, expiry, recovery, resource limits, flood-ttl bounding, dijkstra
through learned links, and the control codec (round trip, bad length, unknown
kind, bad state, oversized, IPv6 endpoint).

`tests/topology.rs` adds:

- a **deterministic simulation** (no sleeps, injected time) of the two-path
  topology: it converges to B, fails B over to C after `link_timeout`, and
  recovers B; plus a line topology where a failure leaves the destination
  genuinely unreachable.
- a **real-socket** test: four overlay pipelines over loopback udp, a packet
  forwarded A→B→D, B shut down, A's route converges to C, and a second packet
  is delivered A→C→D.

Recovery and convergence are therefore measured reproducibly rather than
asserted.

## Known limitations

- no authentication of advertisements (see above);
- no guarantee of instantaneous or strongly consistent convergence;
- a delayed duplicate can briefly re-extend an expired link's freshness;
- sequence numbers assume roughly synchronised clocks across restarts;
- endpoint migration, NAT traversal and relay fallback are out of scope (M6);
- IPv4 overlay addressing and IPv4 udp endpoints only;
- the control plane shares the data socket, so there is no separate control
  queue and no prioritisation.

## What remains for M6

Endpoint migration, NAT traversal and relay fallback, plus authenticated
topology advertisements.
