# M4 — multi-hop routing

How packets hop through intermediate peers, and how to run and test it.

## Where M1–M3 actually stand

| Milestone | Status | Notes |
| --------- | ------ | ----- |
| M1 UDP transport | done | `wire.rs`, `transport.rs`, message mode. |
| M2 TUN overlay | done | `tun.rs`, `overlay.rs`, two-node forwarding. |
| M3 peer lifecycle | **not implemented** | no probing, no suspect/connected/disconnected states. |
| M4 multi-hop routing | this milestone | static routes, destination-aware forwarding, TTL. |

Because M3 is missing, M4 uses a deliberately tiny stand-in, `src/peer.rs`.
`PeerSet` just records which peers are configured and an `available` flag per
peer. `forward::decide` asks `PeerSet::is_available` before choosing a next hop.
When M3 lands, `is_available` should read the live peer state instead. Nothing
else here depends on M3.

## Routing table

`src/route.rs`. Independent of sockets and TUN devices.

- `Ipv4Prefix` — an IPv4 prefix (`10.0.0.3` or `10.0.0.0/24`), with
  `contains(addr)`.
- `Route` — `{ dest: Ipv4Prefix, next_hop: SocketAddr, metric: u32, source }`.
  The next hop is the peer's UDP endpoint. `source` is `Static` for now, so a
  later milestone can add advertised routes without changing the type.
- `RoutingTable::lookup(dest)` — longest-prefix match, ties broken by lowest
  metric. Returns `None` when nothing matches; callers must handle that.

## Forwarding decision

`src/forward.rs` is the pure policy. Given a validated packet, the node's own
addresses, the routing table and the peer set:

1. destination not IPv4/IPv6 → `Drop(NotRoutable)`
2. destination is one of our addresses → `Deliver`
3. no route → `Drop(NoRoute)`
4. transit packet with hop limit ≤ 1 → `Drop(HopLimitExpired)`
5. next hop is the peer we received it from → `Drop(WouldLoop)`
6. next hop not configured/available → `Drop(NextHopUnavailable)`
7. otherwise → `Forward { next_hop }`

Locally originated packets (read from the TUN) skip the hop-limit and loop
checks: a host may send with any TTL and there is no sender to loop back to.

## Pipeline

`src/overlay.rs` runs four tasks joined by two bounded queues:

```text
tun reader ──> outbound queue (next hop, packet) ──> udp sender
udp receiver ──> inbound queue ──> tun writer
             └─> outbound queue (forwarded packets)
```

- packets read from the TUN are validated, routed, and queued toward their next
  hop. Their TTL is left alone.
- packets from a known peer are validated and decided. `Deliver` goes to the
  TUN. `Forward` is cloned, its TTL is decremented (with the IPv4 header
  checksum fixed), and it is pushed to the outbound queue with `try_send`: a
  full queue drops the packet so receiving never blocks.
- unknown source addresses and malformed frames/packets are dropped and logged.
- the inner source and destination addresses are never rewritten.

`packet::decrement_hop_limit` handles IPv4 TTL and IPv6 hop limit and returns
`false` when the limit is exhausted, so forwarding drops rather than mutates.

## Configuring a three-node topology

Overlay addresses `10.0.0.1` (A), `10.0.0.2` (B), `10.0.0.3` (C); UDP endpoints
`127.0.0.1:4001..4003`. B is the middle hop; C has no routes.

```sh
# A: reach B and C through B
cargo run -- --bind 127.0.0.1:4001 --tun \
  --address 10.0.0.1 --peer-address 10.0.0.2 \
  --peer 127.0.0.1:4002 \
  --route 10.0.0.2/32=127.0.0.1:4002 \
  --route 10.0.0.3/32=127.0.0.1:4002

# B: reach C directly, accept from A and C
cargo run -- --bind 127.0.0.1:4002 --tun \
  --address 10.0.0.2 --peer-address 10.0.0.3 \
  --peer 127.0.0.1:4001 --peer 127.0.0.1:4003 \
  --route 10.0.0.3/32=127.0.0.1:4003

# C: no routes, only the local address
cargo run -- --bind 127.0.0.1:4003 --tun \
  --address 10.0.0.3 --peer-address 10.0.0.2 \
  --peer 127.0.0.1:4002
```

A packet sourced at `10.0.0.1` for `10.0.0.3` goes A → B → C. B forwards it, so
its TTL drops by one; the addresses do not change. `--peer` is repeatable and is
the set of peers accepted from. If exactly one `--peer` is given and no
`--route`, a direct host route to `--peer-address` is added automatically, which
keeps the M2 two-node setup working.

Requires root (it opens a TUN interface). The three-node path is also covered by
the automated test below, which needs no privileges.

## Tests

```sh
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

Unit tests cover prefix parsing, longest-prefix lookup, metric tie-breaks, the
forwarding decision (deliver / forward / no route / unavailable / TTL expired /
loop), and the packet destination, hop-limit and checksum helpers.

`tests/routing.rs` runs real overlay pipelines over loopback sockets with a fake
TUN and covers:

- `three_nodes_carry_traffic_from_a_to_c_through_b` — A→C via B, asserting the
  inner addresses are unchanged and the TTL drops by exactly one.
- `packets_without_a_route_are_dropped`
- `an_unavailable_next_hop_drops_the_packet`
- `cyclic_routes_do_not_forward_indefinitely`

## Known limitations

- routes are static; nothing advertises or withdraws them.
- no alternate-path failover when a link goes down.
- no peer authentication or forwarding authorization: accepting a source
  address is just a UDP source check, not proof of identity.
- only IPv4 destinations can be routed; IPv6 destinations are dropped.
- the peer `available` flag is manual (M3 is missing).
- one UDP socket per node; there is no per-peer queue, so a full outbound queue
  drops forwarded packets (tun-originated packets still push back).

## What remains for M5

Topology-driven route updates, link-failure detection, and automatic recovery
over an alternate path.
