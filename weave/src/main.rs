use std::error::Error;
use std::io;
use std::net::{Ipv4Addr, SocketAddr};

use clap::Parser;
use tokio::sync::watch;
use tokio::task::JoinError;
use tracing::{info, warn};

use weave::{overlay, transport, tun, wire};

/// weave - a udp transport.
///
/// run two with --peer pointing at each other to exchange a frame. add --tun
/// with overlay addresses to forward real ip packets between tun interfaces
/// instead. no crypto, no routing, no delivery guarantees.
#[derive(Debug, Parser)]
#[command(name = "weave", version, about, long_about)]
struct Args {
    /// local udp addr to bind, e.g. 127.0.0.1:4000
    #[arg(long, value_name = "ADDR:PORT")]
    bind: SocketAddr,

    /// peer endpoint, e.g. 127.0.0.1:4001
    #[arg(long, value_name = "ADDR:PORT")]
    peer: Option<SocketAddr>,

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

    /// overlay mtu (default 1420)
    #[arg(long, value_name = "N", requires = "tun")]
    mtu: Option<u16>,

    /// packets allowed to queue per direction before we stop reading (default 1024)
    #[arg(long, value_name = "N", requires = "tun", default_value_t = 1024)]
    queue_depth: usize,
}

/// default overlay mtu, leaves room for the udp and frame headers.
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

    if let Some(peer) = args.peer {
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
    let peer = args
        .peer
        .ok_or_else(|| missing("--peer is required together with --tun"))?;
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
    info!(bind = %args.bind, %peer, %address, %peer_address, "weave overlay listening");

    let overlay_config = overlay::OverlayConfig {
        peer,
        mtu: usize::from(mtu),
        queue_depth: args.queue_depth,
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
