use std::error::Error;
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use tokio::sync::watch;
use tokio::task::JoinError;
use tracing::{info, warn};

use weave::peer::PeerSet;
use weave::route::{Route, RouteSnapshot, RouteSpec, RoutingTable};
use weave::router::{Neighbor, RouterConfig};
use weave::topology::NodeId;
use weave::{overlay, transport, tun, wire};

/// weave.
///
/// two nodes with `--peer` pointing at each other exchange a message. with
/// `--tun`, overlay addresses and `--route`s they move ip packets, hopping
/// through intermediate peers as needed. with `--neighbor`, peers exchange
/// link state, calculate routes, and fail over to an alternate path when a
/// link dies. no crypto or discovery.
#[derive(Debug, Parser)]
#[command(name = "weave", version, about, long_about)]
struct Args {
    /// local udp addr to bind, e.g. 127.0.0.1:4000
    #[arg(long, value_name = "ADDR:PORT")]
    bind: SocketAddr,

    /// peer endpoint, repeatable, e.g. 127.0.0.1:4001
    #[arg(long = "peer", value_name = "ADDR:PORT")]
    peers: Vec<SocketAddr>,

    /// bring up a tun interface and forward ip packets instead of the dev message
    #[arg(long)]
    tun: bool,

    /// requested tun name; on macos this looks like utunN, omit to let the kernel pick
    #[arg(long, value_name = "NAME", requires = "tun")]
    tun_name: Option<String>,

    /// this node's overlay ipv4 address (required with --tun)
    #[arg(long, value_name = "IP", requires = "tun")]
    address: Option<Ipv4Addr>,

    /// the peer's overlay ipv4 address (required with --tun)
    #[arg(long, value_name = "IP", requires = "tun")]
    peer_address: Option<Ipv4Addr>,

    /// static route, repeatable: `<prefix>=<ADDR:PORT>`, e.g. 10.0.0.3/32=127.0.0.1:4003
    #[arg(long = "route", value_name = "PREFIX=ADDR:PORT", requires = "tun")]
    routes: Vec<RouteSpec>,

    /// topology neighbour, repeatable: `<IP>=<ADDR:PORT>`. enables dynamic
    /// routing: peers exchange link state and routes are calculated.
    #[arg(long = "neighbor", value_name = "IP=ADDR:PORT", requires = "tun")]
    neighbors: Vec<NeighborSpec>,

    /// topology heartbeat interval in milliseconds (default 1000)
    #[arg(long, value_name = "MS", requires = "tun")]
    heartbeat_ms: Option<u64>,

    /// how long a silent neighbour is trusted, in milliseconds (default 3000)
    #[arg(long, value_name = "MS", requires = "tun")]
    link_timeout_ms: Option<u64>,

    /// overlay mtu (default 1420)
    #[arg(long, value_name = "N", requires = "tun")]
    mtu: Option<u16>,

    /// packets buffered per direction (default 1024)
    #[arg(long, value_name = "N", requires = "tun", default_value_t = 1024)]
    queue_depth: usize,
}

/// a topology neighbour: `<overlay-ip>=<udp-addr>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NeighborSpec {
    id: NodeId,
    addr: SocketAddr,
}

impl FromStr for NeighborSpec {
    type Err = io::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (ip, addr) = s
            .split_once('=')
            .ok_or_else(|| missing(&format!("invalid neighbour spec: {s}")))?;
        let id = ip
            .parse::<Ipv4Addr>()
            .map(NodeId)
            .map_err(|_| missing(&format!("invalid neighbour overlay address: {ip}")))?;
        let addr = addr
            .parse::<SocketAddr>()
            .map_err(|_| missing(&format!("invalid neighbour endpoint: {addr}")))?;
        Ok(Self { id, addr })
    }
}

/// default overlay mtu.
const DEFAULT_MTU: u16 = 1420;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .init();

    run(Args::parse()).await
}

async fn run(args: Args) -> Result<(), Box<dyn Error>> {
    if args.tun {
        run_overlay(args).await
    } else {
        run_message(args).await
    }
}

async fn run_message(args: Args) -> Result<(), Box<dyn Error>> {
    let socket = transport::bind(args.bind).await?;
    info!(bind = %args.bind, "weave node listening");

    if let Some(peer) = args.peers.first().copied() {
        let payload = format!("hello from {}", args.bind);
        transport::send_message(&socket, peer, payload.as_bytes()).await?;
        info!(%peer, "sent initial data message");
    } else {
        info!("no --peer configured; running in receive-only mode");
    }

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let mut recv_handle = tokio::spawn(transport::recv_loop(socket, shutdown_rx));

    // whichever happens first: ctrl+c, or the recv task ending on its own
    let early_exit = tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            info!("Ctrl+C received; shutting down");
            None
        }
        result = &mut recv_handle => Some(result),
    };

    // stop the recv task and wait for it to finish
    let _ = shutdown_tx.send(true);
    match early_exit {
        Some(result) => report_receiver(result),
        None => report_receiver(recv_handle.await),
    }

    Ok(())
}

async fn run_overlay(args: Args) -> Result<(), Box<dyn Error>> {
    let address = args
        .address
        .ok_or_else(|| missing("--address is required together with --tun"))?;
    let peer_address = args
        .peer_address
        .ok_or_else(|| missing("--peer-address is required together with --tun"))?;
    let mtu = args.mtu.unwrap_or(DEFAULT_MTU);

    if usize::from(mtu) > wire::MAX_PAYLOAD_LEN {
        return Err(missing(&format!(
            "--mtu {mtu} is larger than the biggest payload of {} bytes",
            wire::MAX_PAYLOAD_LEN
        ))
        .into());
    }

    let routes: Vec<Route> = args
        .routes
        .iter()
        .copied()
        .map(RouteSpec::into_route)
        .collect();
    let neighbors: Vec<Neighbor> = args
        .neighbors
        .iter()
        .map(|spec| Neighbor {
            id: spec.id,
            addr: spec.addr,
        })
        .collect();

    // keep the two-node setup working: a single peer, no routes and no topology
    // means reach that peer's overlay address directly.
    let mut routes = routes;
    if routes.is_empty() && neighbors.is_empty() && args.peers.len() == 1 {
        routes.push(Route::host(peer_address, args.peers[0]));
        info!(peer = %args.peers[0], dest = %peer_address, "added a direct route to the peer");
    }
    if routes.is_empty() && neighbors.is_empty() {
        return Err(missing(
            "--tun needs at least one --route, --neighbor, or a single --peer to route to --peer-address",
        )
        .into());
    }

    // a node accepts from its configured peers, route next hops and neighbours.
    let mut peer_addrs = args.peers.clone();
    for route in &routes {
        if !peer_addrs.contains(&route.next_hop) {
            peer_addrs.push(route.next_hop);
        }
    }
    for neighbor in &neighbors {
        if !peer_addrs.contains(&neighbor.addr) {
            peer_addrs.push(neighbor.addr);
        }
    }
    for route in &routes {
        info!(dest = %route.dest, next_hop = %route.next_hop, source = route.source.as_str(), "route configured");
    }

    let tun_config = tun::TunConfig {
        name: args.tun_name.clone(),
        address,
        destination: peer_address,
        netmask: Ipv4Addr::new(255, 255, 255, 255),
        mtu,
        enable_routing: false,
    };

    let device = tun::TunDevice::open(&tun_config)?;
    info!(?tun_config, "tun interface ready");
    let (reader, writer) = device.split()?;

    let socket = transport::bind(args.bind).await?;
    let local_addr = socket.local_addr()?;
    info!(bind = %args.bind, %address, "weave overlay listening");

    let control = if neighbors.is_empty() {
        None
    } else {
        let mut config = RouterConfig::new(NodeId(address), local_addr, neighbors);
        config.base_routes = routes.clone();
        if let Some(ms) = args.heartbeat_ms {
            config.heartbeat = Duration::from_millis(ms.max(1));
        }
        if let Some(ms) = args.link_timeout_ms {
            config.link_timeout = Duration::from_millis(ms.max(1));
        }
        info!(
            heartbeat_ms = config.heartbeat.as_millis() as u64,
            link_timeout_ms = config.link_timeout.as_millis() as u64,
            "topology routing enabled"
        );
        Some(config)
    };

    let overlay_config = overlay::OverlayConfig {
        local: vec![address],
        mtu: usize::from(mtu),
        queue_depth: args.queue_depth,
        routes: RouteSnapshot::new(RoutingTable::new(routes)),
        peers: Arc::new(PeerSet::new(peer_addrs)),
        control,
    };

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let mut handle = tokio::spawn(overlay::run(
        reader,
        writer,
        overlay_config,
        socket,
        shutdown_rx,
    ));

    let early_exit = tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            info!("Ctrl+C received; shutting down");
            None
        }
        result = &mut handle => Some(result),
    };

    let _ = shutdown_tx.send(true);
    match early_exit {
        Some(result) => report_receiver(result),
        None => report_receiver(handle.await),
    }

    Ok(())
}

fn missing(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.to_owned())
}

fn report_receiver(result: Result<std::io::Result<()>, JoinError>) {
    match result {
        Ok(Ok(())) => info!("receive task stopped cleanly"),
        Ok(Err(err)) => warn!(error = %err, "receive task ended with error"),
        Err(err) => warn!(error = %err, "receive task failed to join"),
    }
}
