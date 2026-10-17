//! m5 integration tests: topology exchange, route calculation, failure
//! detection and recovery.
//!
//! two levels of test live here:
//!
//! 1. a deterministic in-memory simulation. four nodes run the real `Router`
//!    core, but messages are shuttled between them directly and time is passed
//!    in, so there is no sleeping and every run is reproducible. it covers
//!    convergence, a link failure, and recovery.
//! 2. a real-socket test. four overlay pipelines run over loopback udp with the
//!    control plane enabled; we forward a packet across two possible paths, kill
//!    the middle node on the active path, and check traffic moves to the other.

use std::collections::HashMap;
use std::future::Future;
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant as StdInstant};

use tokio::net::UdpSocket;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::{timeout, Instant};

use weave::control::Message;
use weave::overlay::{self, OverlayConfig};
use weave::peer::PeerSet;
use weave::route::{RouteSnapshot, RoutingTable};
use weave::router::{Neighbor, Router, RouterConfig};
use weave::topology::NodeId;
use weave::tun::{PacketReader, PacketWriter};

fn ip(s: &str) -> Ipv4Addr {
    s.parse().unwrap()
}

fn node(s: &str) -> NodeId {
    NodeId(ip(s))
}

fn addr(port: u16) -> SocketAddr {
    format!("127.0.0.1:{port}").parse().unwrap()
}

/// `(overlay-ip, udp-port, neighbours)` for one simulated node.
type NodeSpec = (&'static str, u16, &'static [(&'static str, u16)]);

fn neighbors(spec: &[(&str, u16)]) -> Vec<Neighbor> {
    spec.iter()
        .map(|(id, port)| Neighbor {
            id: node(id),
            addr: addr(*port),
        })
        .collect()
}

fn build_router(id: &str, port: u16, spec: &[(&str, u16)]) -> Router {
    let neighbors = neighbors(spec);
    let peers = Arc::new(PeerSet::new(neighbors.iter().map(|n| n.addr)));
    let mut config = RouterConfig::new(node(id), addr(port), neighbors);
    config.link_timeout = Duration::from_millis(500);
    Router::new(config, peers, RouteSnapshot::new(RoutingTable::default()))
}

// ---------------------------------------------------------------------------
// deterministic simulation
// ---------------------------------------------------------------------------

struct SimNode {
    id: NodeId,
    addr: SocketAddr,
    router: Router,
    active: bool,
}

struct Sim {
    nodes: Vec<SimNode>,
    by_addr: HashMap<SocketAddr, usize>,
    floods: usize,
}

impl Sim {
    /// the standard four-node two-path topology:
    /// a-b-d and a-c-d, plus the reverse links.
    fn two_paths() -> Self {
        let spec: [NodeSpec; 4] = [
            ("10.0.0.1", 4101, &[("10.0.0.2", 4102), ("10.0.0.3", 4103)]),
            ("10.0.0.2", 4102, &[("10.0.0.1", 4101), ("10.0.0.4", 4104)]),
            ("10.0.0.3", 4103, &[("10.0.0.1", 4101), ("10.0.0.4", 4104)]),
            ("10.0.0.4", 4104, &[("10.0.0.2", 4102), ("10.0.0.3", 4103)]),
        ];
        let mut nodes = Vec::new();
        let mut by_addr = HashMap::new();
        for (id, port, neigh) in spec {
            let router = build_router(id, port, neigh);
            by_addr.insert(addr(port), nodes.len());
            nodes.push(SimNode {
                id: node(id),
                addr: addr(port),
                router,
                active: true,
            });
        }
        Self {
            nodes,
            by_addr,
            floods: 0,
        }
    }

    fn set_active(&mut self, id: &str, active: bool) {
        let id = node(id);
        for node in &mut self.nodes {
            if node.id == id {
                node.active = active;
            }
        }
    }

    fn start(&mut self, now: StdInstant) {
        let mut queue = Vec::new();
        for node in self.nodes.iter_mut().filter(|n| n.active) {
            for (to, message) in node.router.start(now) {
                queue.push((node.addr, to, message));
            }
        }
        self.deliver(queue, now);
    }

    /// one heartbeat interval across every active node, then flood everything.
    fn round(&mut self, now: StdInstant) {
        let mut queue = Vec::new();
        for node in self.nodes.iter_mut().filter(|n| n.active) {
            for (to, message) in node.router.tick(now) {
                queue.push((node.addr, to, message));
            }
        }
        self.deliver(queue, now);
    }

    fn deliver(&mut self, mut queue: Vec<(SocketAddr, SocketAddr, Message)>, now: StdInstant) {
        // a hard budget turns any accidental flood loop into a test failure
        // instead of a hang.
        let mut budget = 100_000;
        while let Some((from, to, message)) = queue.pop() {
            budget -= 1;
            assert!(budget > 0, "flood did not terminate");
            let Some(&index) = self.by_addr.get(&to) else {
                continue;
            };
            if !self.nodes[index].active {
                continue;
            }
            let out = self.nodes[index].router.handle(from, message, now);
            for (next, message) in out {
                self.floods += 1;
                queue.push((self.nodes[index].addr, next, message));
            }
        }
    }

    fn converge(&mut self, now: StdInstant, rounds: usize) {
        for _ in 0..rounds {
            self.round(now);
        }
    }

    fn route_to(&self, id: &str, dest: &str) -> Option<SocketAddr> {
        let id = node(id);
        let dest = ip(dest);
        self.nodes
            .iter()
            .find(|node| node.id == id)
            .and_then(|node| {
                node.router
                    .routes()
                    .lookup(dest)
                    .map(|route| route.next_hop)
            })
    }
}

#[test]
fn multi_node_topology_converges_then_reroutes_and_recovers() {
    let t0 = StdInstant::now();
    let mut sim = Sim::two_paths();
    sim.start(t0);
    sim.converge(t0, 2);

    // both paths cost 2; the tie breaks on the lower next-hop id, so a goes
    // through b.
    assert_eq!(
        sim.route_to("10.0.0.1", "10.0.0.4"),
        Some(addr(4102)),
        "a should reach d through b"
    );

    // b goes silent; after the link timeout a must drop it and fall back to c.
    sim.set_active("10.0.0.2", false);
    let t1 = t0 + Duration::from_millis(600);
    sim.converge(t1, 2);
    assert_eq!(
        sim.route_to("10.0.0.1", "10.0.0.4"),
        Some(addr(4103)),
        "a should have failed over to c"
    );
    assert_eq!(
        sim.route_to("10.0.0.1", "10.0.0.2"),
        None,
        "the failed neighbour must be unreachable"
    );

    // b comes back; a restores the preferred path.
    sim.set_active("10.0.0.2", true);
    sim.converge(t1 + Duration::from_millis(100), 2);
    assert_eq!(
        sim.route_to("10.0.0.1", "10.0.0.4"),
        Some(addr(4102)),
        "a should recover the path through b"
    );
}

#[test]
fn a_failed_link_with_no_alternate_leaves_the_destination_unreachable() {
    let t0 = StdInstant::now();
    // a line: a - b - d, no alternate path.
    let mut nodes = Vec::new();
    let mut by_addr = HashMap::new();
    let spec: [NodeSpec; 3] = [
        ("10.0.0.1", 4201, &[("10.0.0.2", 4202)]),
        ("10.0.0.2", 4202, &[("10.0.0.1", 4201), ("10.0.0.4", 4204)]),
        ("10.0.0.4", 4204, &[("10.0.0.2", 4202)]),
    ];
    for (id, port, neigh) in spec {
        let router = build_router(id, port, neigh);
        by_addr.insert(addr(port), nodes.len());
        nodes.push(SimNode {
            id: node(id),
            addr: addr(port),
            router,
            active: true,
        });
    }
    let mut sim = Sim {
        nodes,
        by_addr,
        floods: 0,
    };
    sim.start(t0);
    sim.converge(t0, 2);
    assert_eq!(sim.route_to("10.0.0.1", "10.0.0.4"), Some(addr(4202)));

    sim.set_active("10.0.0.2", false);
    sim.converge(t0 + Duration::from_millis(600), 2);
    assert_eq!(
        sim.route_to("10.0.0.1", "10.0.0.4"),
        None,
        "with no alternate path d is unreachable, not silently misrouted"
    );
}

// ---------------------------------------------------------------------------
// real sockets
// ---------------------------------------------------------------------------

const TIMEOUT: Duration = Duration::from_secs(5);

struct FakeReader {
    rx: mpsc::UnboundedReceiver<Vec<u8>>,
}

#[allow(clippy::manual_async_fn)]
impl PacketReader for FakeReader {
    fn recv<'a>(
        &'a mut self,
        buf: &'a mut [u8],
    ) -> impl Future<Output = io::Result<usize>> + Send + 'a {
        async move {
            match self.rx.recv().await {
                Some(packet) => {
                    let len = packet.len().min(buf.len());
                    buf[..len].copy_from_slice(&packet[..len]);
                    Ok(len)
                }
                None => Err(io::Error::new(io::ErrorKind::UnexpectedEof, "tun closed")),
            }
        }
    }
}

struct FakeWriter {
    tx: mpsc::UnboundedSender<Vec<u8>>,
}

#[allow(clippy::manual_async_fn)]
impl PacketWriter for FakeWriter {
    fn send<'a>(
        &'a mut self,
        packet: &'a [u8],
    ) -> impl Future<Output = io::Result<()>> + Send + 'a {
        async move {
            self.tx
                .send(packet.to_vec())
                .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "tun closed"))
        }
    }
}

fn tun() -> (
    FakeReader,
    FakeWriter,
    mpsc::UnboundedSender<Vec<u8>>,
    mpsc::UnboundedReceiver<Vec<u8>>,
) {
    let (inject, rx) = mpsc::unbounded_channel();
    let (tx, collect) = mpsc::unbounded_channel();
    (FakeReader { rx }, FakeWriter { tx }, inject, collect)
}

struct Node {
    stop: watch::Sender<bool>,
    handle: JoinHandle<io::Result<()>>,
}

impl Node {
    async fn shutdown(self) {
        let _ = self.stop.send(true);
        let _ = timeout(TIMEOUT, self.handle).await;
    }
}

async fn bind() -> (UdpSocket, SocketAddr) {
    let socket = UdpSocket::bind("127.0.0.1:0").await.expect("bind");
    let addr = socket.local_addr().expect("local_addr");
    (socket, addr)
}

fn spawn_node(
    local: &str,
    reader: FakeReader,
    writer: FakeWriter,
    socket: UdpSocket,
    neigh: Vec<(NodeId, SocketAddr)>,
) -> (Node, RouteSnapshot) {
    let local_addr = socket.local_addr().unwrap();
    let peers = Arc::new(PeerSet::new(neigh.iter().map(|(_, addr)| *addr)));
    let mut config = RouterConfig::new(NodeId(ip(local)), local_addr, map_neighbors(&neigh));
    config.heartbeat = Duration::from_millis(50);
    config.link_timeout = Duration::from_millis(400);
    let routes = RouteSnapshot::new(RoutingTable::default());
    let overlay_config = OverlayConfig {
        local: vec![ip(local)],
        mtu: 1420,
        queue_depth: 64,
        routes: routes.clone(),
        peers,
        control: Some(config),
    };
    let (stop, stop_rx) = watch::channel(false);
    let handle = tokio::spawn(overlay::run(
        reader,
        writer,
        overlay_config,
        socket,
        stop_rx,
    ));
    (Node { stop, handle }, routes)
}

fn map_neighbors(neigh: &[(NodeId, SocketAddr)]) -> Vec<Neighbor> {
    neigh
        .iter()
        .map(|(id, addr)| Neighbor {
            id: *id,
            addr: *addr,
        })
        .collect()
}

fn ipv4_packet(src: &str, dst: &str, ttl: u8, payload: usize) -> Vec<u8> {
    let total = 20 + payload;
    let mut packet = vec![0u8; total];
    packet[0] = 0x45;
    packet[2..4].copy_from_slice(&(total as u16).to_be_bytes());
    packet[8] = ttl;
    packet[9] = 17;
    packet[12..16].copy_from_slice(&ip(src).octets());
    packet[16..20].copy_from_slice(&ip(dst).octets());
    packet
}

async fn wait_for_route(routes: &RouteSnapshot, dest: &str, next_hop: SocketAddr) {
    let dest = ip(dest);
    let deadline = Instant::now() + TIMEOUT;
    loop {
        if routes
            .current()
            .lookup(dest)
            .is_some_and(|route| route.next_hop == next_hop)
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "route to {dest} via {next_hop} never appeared"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn traffic_moves_to_the_alternate_path_when_a_link_fails() {
    let (a_reader, a_writer, a_inject, _a_collect) = tun();
    let (b_reader, b_writer, _b_inject, _b_collect) = tun();
    let (c_reader, c_writer, _c_inject, _c_collect) = tun();
    let (d_reader, d_writer, _d_inject, mut d_collect) = tun();

    let (a_socket, a_udp) = bind().await;
    let (b_socket, b_udp) = bind().await;
    let (c_socket, c_udp) = bind().await;
    let (d_socket, d_udp) = bind().await;

    let (a, a_routes) = spawn_node(
        "10.0.0.1",
        a_reader,
        a_writer,
        a_socket,
        vec![(node("10.0.0.2"), b_udp), (node("10.0.0.3"), c_udp)],
    );
    let (b, _b_routes) = spawn_node(
        "10.0.0.2",
        b_reader,
        b_writer,
        b_socket,
        vec![(node("10.0.0.1"), a_udp), (node("10.0.0.4"), d_udp)],
    );
    let (c, _c_routes) = spawn_node(
        "10.0.0.3",
        c_reader,
        c_writer,
        c_socket,
        vec![(node("10.0.0.1"), a_udp), (node("10.0.0.4"), d_udp)],
    );
    let (d, d_routes) = spawn_node(
        "10.0.0.4",
        d_reader,
        d_writer,
        d_socket,
        vec![(node("10.0.0.2"), b_udp), (node("10.0.0.3"), c_udp)],
    );

    // let the topology converge: a picks b as the next hop to d and d reaches a.
    wait_for_route(&a_routes, "10.0.0.4", b_udp).await;
    wait_for_route(&d_routes, "10.0.0.1", b_udp).await;

    // traffic a -> d works over b.
    let first = ipv4_packet("10.0.0.1", "10.0.0.4", 64, 24);
    a_inject.send(first.clone()).unwrap();
    let got = timeout(TIMEOUT, d_collect.recv())
        .await
        .expect("d timed out on the primary path")
        .expect("d closed");
    assert_eq!(&got[16..20], &first[16..20]);

    // b fails. a must detect it and reroute through c.
    b.shutdown().await;
    wait_for_route(&a_routes, "10.0.0.4", c_udp).await;

    // traffic flows again over the remaining path.
    let second = ipv4_packet("10.0.0.1", "10.0.0.4", 64, 24);
    a_inject.send(second.clone()).unwrap();
    let got = timeout(TIMEOUT, d_collect.recv())
        .await
        .expect("d timed out on the alternate path")
        .expect("d closed");
    assert_eq!(&got[16..20], &second[16..20]);

    a.shutdown().await;
    c.shutdown().await;
    d.shutdown().await;
}
