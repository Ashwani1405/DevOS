//! the topology control plane: exchange link state, detect failures, and
//! publish routes.
//!
//! this ties the pieces together. it owns a `Topology`, floods advertisements
//! over the same udp transport the data plane uses, tracks which direct
//! neighbours are still alive, and republishes the `RouteSnapshot` whenever the
//! usable graph changes.
//!
//! # what is and is not trusted
//!
//! control messages arrive over udp and are only accepted from configured
//! neighbours (the overlay drops datagrams from unknown source addresses before
//! they get here). `Topology::apply` additionally requires that the origin of a
//! link owns its tail, so an unrelated peer cannot describe a link it does not
//! own. this is *not* authentication: there is no signature and no shared
//! secret, so a peer that can spoof udp source addresses, or a configured but
//! malicious peer forwarding fabricated advertisements, can still lie about the
//! topology. do not treat the topology as authoritative until m6 adds
//! authentication.
//!
//! time is passed into every method, so the core is deterministic and testable
//! without sleeping. `run` is the only async part and just drives the core.

use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::net::UdpSocket;
use tokio::sync::{mpsc, watch};
use tokio::time::MissedTickBehavior;
use tracing::{debug, info, warn};

use crate::control::{self, Message};
use crate::peer::PeerSet;
use crate::route::{Route, RouteSnapshot, RoutingTable};
use crate::topology::{Applied, LinkState, NodeId, Topology};
use crate::transport;

/// one direct neighbour: a stable identity and its current udp endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Neighbor {
    /// the neighbour's overlay identity.
    pub id: NodeId,
    /// the neighbour's udp endpoint.
    pub addr: SocketAddr,
}

/// settings for the topology control plane.
#[derive(Debug, Clone)]
pub struct RouterConfig {
    /// this node's identity.
    pub local: NodeId,
    /// this node's udp endpoint.
    pub local_addr: SocketAddr,
    /// configured direct neighbours.
    pub neighbors: Vec<Neighbor>,
    /// metric applied to every local link.
    pub link_metric: u32,
    /// how often to re-send local advertisements as heartbeats.
    pub heartbeat: Duration,
    /// how long without a control message before a neighbour is considered down.
    pub link_timeout: Duration,
    /// how many hops an originated advertisement may flood.
    pub max_flood_ttl: u8,
    /// static routes kept in every published table.
    pub base_routes: Vec<Route>,
}

impl RouterConfig {
    /// defaults for the timers, keeping the identity and neighbours.
    pub fn new(local: NodeId, local_addr: SocketAddr, neighbors: Vec<Neighbor>) -> Self {
        Self {
            local,
            local_addr,
            neighbors,
            link_metric: 1,
            heartbeat: Duration::from_secs(1),
            link_timeout: Duration::from_secs(3),
            max_flood_ttl: 4,
            base_routes: Vec::new(),
        }
    }
}

/// a message the core wants sent to a neighbour.
pub type Outbound = (SocketAddr, Message);

/// counters for observability.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RouterStats {
    /// advertisements accepted as new information.
    pub applied: u64,
    /// advertisements rejected as stale.
    pub stale: u64,
    /// advertisements rejected as invalid.
    pub invalid: u64,
    /// routing table recalculations.
    pub recalculations: u64,
    /// advertisements re-flooded to other peers.
    pub floods: u64,
}

/// the deterministic core of the control plane.
pub struct Router {
    config: RouterConfig,
    topology: Topology,
    peers: Arc<PeerSet>,
    routes: RouteSnapshot,
    last_seen: HashMap<SocketAddr, Instant>,
    dirty: bool,
    stats: RouterStats,
}

impl Router {
    /// build a router with local links to every configured neighbour.
    pub fn new(config: RouterConfig, peers: Arc<PeerSet>, routes: RouteSnapshot) -> Self {
        let mut topology = Topology::new(config.local, config.local_addr);
        for neighbor in &config.neighbors {
            topology.add_neighbor(neighbor.id, neighbor.addr, config.link_metric);
        }
        Self {
            config,
            topology,
            peers,
            routes,
            last_seen: HashMap::new(),
            dirty: false,
            stats: RouterStats::default(),
        }
    }

    /// the topology this router currently believes in.
    pub fn topology(&self) -> &Topology {
        &self.topology
    }

    /// counters.
    pub fn stats(&self) -> RouterStats {
        self.stats
    }

    /// the routing table this router last published.
    pub fn routes(&self) -> std::sync::Arc<RoutingTable> {
        self.routes.current()
    }

    /// the endpoint for a neighbour identity.
    fn neighbor_addr(&self, id: NodeId) -> Option<SocketAddr> {
        self.config
            .neighbors
            .iter()
            .find(|neighbor| neighbor.id == id)
            .map(|neighbor| neighbor.addr)
    }

    /// the neighbour identity for a udp endpoint.
    fn neighbor_of(&self, addr: SocketAddr) -> Option<NodeId> {
        self.config
            .neighbors
            .iter()
            .find(|neighbor| neighbor.addr == addr)
            .map(|neighbor| neighbor.id)
    }

    /// prime liveness and ask every neighbour for a full update. advertise our
    /// own links so they learn us too.
    pub fn start(&mut self, now: Instant) -> Vec<Outbound> {
        for neighbor in &self.config.neighbors {
            self.last_seen.insert(neighbor.addr, now);
        }
        let mut outbound = Vec::new();
        for neighbor in &self.config.neighbors {
            outbound.push((
                neighbor.addr,
                Message::LinkRequest {
                    origin: self.config.local,
                    origin_addr: self.config.local_addr,
                },
            ));
        }
        self.advertise_all(&mut outbound);
        self.publish_if_dirty();
        outbound
    }

    /// handle a control message from `from`.
    pub fn handle(&mut self, from: SocketAddr, message: Message, now: Instant) -> Vec<Outbound> {
        let Some(peer_id) = self.neighbor_of(from) else {
            debug!(%from, "ignored control message from a non-neighbour");
            return Vec::new();
        };
        self.last_seen.insert(from, now);

        let mut outbound = Vec::new();
        // any control message proves the link is alive: restore it if we had
        // marked it down, and let everyone know.
        if self.topology.local_state(peer_id) == Some(LinkState::Down)
            && self.set_link_state(peer_id, LinkState::Up)
        {
            info!(neighbor = %peer_id, "neighbour link recovered");
            self.advertise_all(&mut outbound);
        }

        match message {
            Message::LinkAdvert(advert) => self.handle_advert(from, advert, now, &mut outbound),
            Message::LinkRequest {
                origin,
                origin_addr,
            } => {
                debug!(%from, %origin, %origin_addr, "replying to a link request");
                self.reply_snapshot(from, &mut outbound);
            }
        }

        self.publish_if_dirty();
        outbound
    }

    /// periodic work: expire stale links, detect failed neighbours, and
    /// heartbeat our own advertisements.
    pub fn tick(&mut self, now: Instant) -> Vec<Outbound> {
        if self.topology.expire(now, self.config.link_timeout) {
            self.dirty = true;
        }

        let mut changed = false;
        let neighbors = self.config.neighbors.clone();
        for neighbor in &neighbors {
            let stale = now
                .saturating_duration_since(*self.last_seen.get(&neighbor.addr).unwrap_or(&now))
                >= self.config.link_timeout;
            if stale
                && self.topology.local_state(neighbor.id) == Some(LinkState::Up)
                && self.set_link_state(neighbor.id, LinkState::Down)
            {
                warn!(neighbor = %neighbor.id, addr = %neighbor.addr, "neighbour link timed out");
                changed = true;
            }
        }
        if changed {
            self.dirty = true;
        }

        self.publish_if_dirty();

        let mut outbound = Vec::new();
        self.advertise_all(&mut outbound);
        outbound
    }

    fn handle_advert(
        &mut self,
        from: SocketAddr,
        advert: crate::topology::LinkAdvert,
        now: Instant,
        outbound: &mut Vec<Outbound>,
    ) {
        match self.topology.apply(&advert, now) {
            Applied::New => {
                self.stats.applied += 1;
                self.dirty = true;
                if let Some(forwarded) = advert.decremented() {
                    let message = Message::LinkAdvert(forwarded);
                    let mut sent = 0;
                    for neighbor in &self.config.neighbors {
                        if neighbor.addr == from || neighbor.id == advert.origin {
                            continue;
                        }
                        outbound.push((neighbor.addr, message.clone()));
                        sent += 1;
                    }
                    self.stats.floods += u64::from(sent > 0);
                }
            }
            Applied::Refreshed => {}
            Applied::Stale => {
                self.stats.stale += 1;
                debug!(%from, origin = %advert.origin, seq = advert.seq, "rejected stale advertisement");
            }
            Applied::Invalid(reason) => {
                self.stats.invalid += 1;
                warn!(%from, reason = reason.as_str(), "rejected invalid advertisement");
            }
        }
    }

    fn reply_snapshot(&self, to: SocketAddr, outbound: &mut Vec<Outbound>) {
        for advert in self.topology.snapshot_adverts() {
            outbound.push((to, Message::LinkAdvert(advert)));
        }
    }

    fn advertise_all(&mut self, outbound: &mut Vec<Outbound>) {
        let ttl = self.config.max_flood_ttl;
        let adverts = self.topology.local_adverts(ttl);
        let messages: Vec<Message> = adverts.into_iter().map(Message::LinkAdvert).collect();
        for neighbor in &self.config.neighbors {
            for message in &messages {
                outbound.push((neighbor.addr, message.clone()));
            }
        }
    }

    fn set_link_state(&mut self, id: NodeId, state: LinkState) -> bool {
        if !self.topology.set_local_state(id, state) {
            return false;
        }
        self.dirty = true;
        if let Some(addr) = self.neighbor_addr(id) {
            self.peers.set_available(addr, state.is_up());
        }
        true
    }

    /// recompute and publish routes if the usable graph changed.
    fn publish_if_dirty(&mut self) {
        if !self.dirty {
            return;
        }
        let mut routes = self.config.base_routes.clone();
        routes.extend(self.topology.routes());
        let count = routes.len();
        self.routes.publish(RoutingTable::new(routes));
        self.dirty = false;
        self.stats.recalculations += 1;
        info!(
            routes = count,
            version = self.topology.version(),
            recalculations = self.stats.recalculations,
            "published routing table"
        );
    }
}

/// run the control plane until shutdown.
///
/// `inbound` carries decoded control messages from the overlay's udp receiver,
/// tagged with the udp source address.
pub async fn run(
    mut router: Router,
    socket: Arc<UdpSocket>,
    mut inbound: mpsc::Receiver<(SocketAddr, Message)>,
    mut stop: watch::Receiver<bool>,
) -> io::Result<()> {
    let started = router.start(Instant::now());
    send_all(&socket, started).await;

    let mut ticker = tokio::time::interval(router.config.heartbeat);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = stop.changed() => {
                debug!("control plane stopping");
                return Ok(());
            }
            _ = ticker.tick() => {
                let outbound = router.tick(Instant::now());
                send_all(&socket, outbound).await;
            }
            item = inbound.recv() => {
                let Some((from, message)) = item else {
                    return Ok(());
                };
                let outbound = router.handle(from, message, Instant::now());
                send_all(&socket, outbound).await;
            }
        }
    }
}

async fn send_all(socket: &UdpSocket, outbound: Vec<Outbound>) {
    for (addr, message) in outbound {
        let body = match control::encode(&message) {
            Ok(body) => body,
            Err(err) => {
                warn!(error = %err, "failed to encode control message");
                continue;
            }
        };
        if let Err(err) = transport::send_control(socket, addr, &body).await {
            debug!(%addr, error = %err, "failed to send control message");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::topology::LinkAdvert;

    fn node(s: &str) -> NodeId {
        NodeId(s.parse().unwrap())
    }

    fn ip(s: &str) -> std::net::Ipv4Addr {
        s.parse().unwrap()
    }

    fn addr(port: u16) -> SocketAddr {
        format!("127.0.0.1:{port}").parse().unwrap()
    }

    fn far_advert(origin: &str, seq: u64, to: &str, state: LinkState) -> LinkAdvert {
        LinkAdvert {
            origin: node(origin),
            origin_addr: addr(4000 + u16::from(origin.as_bytes()[7] - b'0')),
            seq,
            from: node(origin),
            to: node(to),
            metric: 1,
            state,
            ttl: 4,
        }
    }

    fn router(local: &str, local_port: u16, neighbors: &[(&str, u16)]) -> Router {
        let neighbors: Vec<Neighbor> = neighbors
            .iter()
            .map(|(id, port)| Neighbor {
                id: node(id),
                addr: addr(*port),
            })
            .collect();
        let addrs = neighbors.iter().map(|n| n.addr);
        let peers = Arc::new(PeerSet::new(addrs));
        Router::new(
            RouterConfig::new(node(local), addr(local_port), neighbors),
            peers,
            RouteSnapshot::new(RoutingTable::default()),
        )
    }

    #[test]
    fn a_new_advert_is_flooded_to_other_peers() {
        let mut r = router("10.0.0.1", 4001, &[("10.0.0.2", 4002), ("10.0.0.3", 4003)]);
        let now = Instant::now();
        r.start(now);

        // b tells a about b -> d
        let out = r.handle(
            addr(4002),
            Message::LinkAdvert(far_advert("10.0.0.2", 1, "10.0.0.4", LinkState::Up)),
            now,
        );
        assert!(
            out.iter().any(|(to, msg)| *to == addr(4003)
                && matches!(msg, Message::LinkAdvert(a) if a.origin == node("10.0.0.2"))),
            "the new advert should be flooded to the third peer"
        );
        assert_eq!(r.stats().applied, 1);
        assert!(
            r.stats().recalculations >= 1,
            "routes should have been recalculated"
        );
    }

    #[test]
    fn a_stale_advert_is_not_flooded() {
        let mut r = router("10.0.0.1", 4001, &[("10.0.0.2", 4002), ("10.0.0.3", 4003)]);
        let now = Instant::now();
        r.start(now);
        r.handle(
            addr(4002),
            Message::LinkAdvert(far_advert("10.0.0.2", 5, "10.0.0.4", LinkState::Up)),
            now,
        );
        let out = r.handle(
            addr(4002),
            Message::LinkAdvert(far_advert("10.0.0.2", 2, "10.0.0.4", LinkState::Up)),
            now,
        );
        assert!(out.is_empty(), "stale advert was re-flooded");
        assert_eq!(r.stats().stale, 1);
    }

    #[test]
    fn a_peer_timeout_marks_the_link_down_and_recomputes() {
        let mut r = router("10.0.0.1", 4001, &[("10.0.0.2", 4002)]);
        let start = Instant::now();
        r.start(start);
        r.handle(
            addr(4002),
            Message::LinkAdvert(far_advert("10.0.0.2", 1, "10.0.0.4", LinkState::Up)),
            start,
        );
        assert!(r
            .topology()
            .routes()
            .iter()
            .any(|route| route.dest.addr() == ip("10.0.0.4")));

        // no control messages for longer than link_timeout.
        let later = start + r.config.link_timeout + Duration::from_millis(1);
        r.tick(later);
        assert_eq!(
            r.topology().local_state(node("10.0.0.2")),
            Some(LinkState::Down)
        );
        assert!(
            r.topology().routes().is_empty(),
            "routes through the failed peer remain"
        );
        assert!(
            !r.peers.is_available(addr(4002)),
            "failed peer still marked available"
        );
    }

    #[test]
    fn a_recovered_peer_link_comes_back() {
        let mut r = router("10.0.0.1", 4001, &[("10.0.0.2", 4002)]);
        let start = Instant::now();
        r.start(start);
        let later = start + r.config.link_timeout + Duration::from_secs(1);
        r.tick(later);
        assert_eq!(
            r.topology().local_state(node("10.0.0.2")),
            Some(LinkState::Down)
        );

        // a fresh control message arrives from the peer.
        r.handle(
            addr(4002),
            Message::LinkRequest {
                origin: node("10.0.0.2"),
                origin_addr: addr(4002),
            },
            later,
        );
        assert_eq!(
            r.topology().local_state(node("10.0.0.2")),
            Some(LinkState::Up)
        );
        assert!(r.peers.is_available(addr(4002)));
    }

    #[test]
    fn an_exhausted_advert_is_not_flooded() {
        let mut r = router("10.0.0.1", 4001, &[("10.0.0.2", 4002), ("10.0.0.3", 4003)]);
        let now = Instant::now();
        r.start(now);
        let mut advert = far_advert("10.0.0.2", 1, "10.0.0.4", LinkState::Up);
        advert.ttl = 0;
        let out = r.handle(addr(4002), Message::LinkAdvert(advert), now);
        assert!(
            out.iter()
                .all(|(_, msg)| !matches!(msg, Message::LinkAdvert(_))),
            "an advert with no ttl left must not be re-flooded"
        );
        assert_eq!(r.stats().applied, 1, "it is still applied locally");
    }

    #[test]
    fn control_messages_from_non_neighbours_are_ignored() {
        let mut r = router("10.0.0.1", 4001, &[("10.0.0.2", 4002)]);
        let now = Instant::now();
        r.start(now);
        let out = r.handle(
            addr(9999),
            Message::LinkAdvert(far_advert("10.0.0.2", 1, "10.0.0.4", LinkState::Up)),
            now,
        );
        assert!(out.is_empty());
        assert_eq!(
            r.stats().applied,
            0,
            "a stranger must not change the topology"
        );
    }

    #[test]
    fn a_link_request_gets_a_snapshot_reply() {
        let mut r = router("10.0.0.1", 4001, &[("10.0.0.2", 4002)]);
        let now = Instant::now();
        r.start(now);
        let out = r.handle(
            addr(4002),
            Message::LinkRequest {
                origin: node("10.0.0.2"),
                origin_addr: addr(4002),
            },
            now,
        );
        assert!(
            out.iter().all(|(to, _)| *to == addr(4002)),
            "snapshot replies must be unicast to the requester"
        );
        assert!(
            out.iter().any(
                |(_, msg)| matches!(msg, Message::LinkAdvert(a) if a.origin == node("10.0.0.1"))
            ),
            "the snapshot should include our own links"
        );
    }
}
