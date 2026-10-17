//! the packet pipeline between a tun interface and udp.
//!
//! packets read from the tun are routed to their next hop. packets arriving
//! over udp are delivered locally if the destination is one of ours, otherwise
//! forwarded toward the next hop after a hop-limit decrement. queues are
//! bounded, so a slow side pushes back instead of buffering without limit.

use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

use tokio::net::UdpSocket;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinSet;
use tracing::{debug, warn};

use crate::forward::{self, Decision, DropReason};
use crate::packet;
use crate::peer::PeerSet;
use crate::route::RoutingTable;
use crate::transport;
use crate::tun::{PacketReader, PacketWriter};
use crate::wire::{self, MAX_DATAGRAM_SIZE};

/// one packet on its way to a peer.
type Outbound = (SocketAddr, Vec<u8>);

/// overlay settings.
#[derive(Debug, Clone)]
pub struct OverlayConfig {
    /// this node's own overlay addresses; packets for these are delivered locally.
    pub local: Vec<Ipv4Addr>,
    /// largest packet read from the interface.
    pub mtu: usize,
    /// packets buffered per direction before reads block.
    pub queue_depth: usize,
    /// where each destination prefix is sent.
    pub routes: Arc<RoutingTable>,
    /// peers we accept from, and whether each may be used as a next hop.
    pub peers: Arc<PeerSet>,
}

/// run until shutdown or a task fails.
///
/// `external` is the caller's shutdown signal: when it flips to `true`, or its
/// sender is dropped, everything here stops.
pub async fn run<R, W>(
    reader: R,
    writer: W,
    config: OverlayConfig,
    socket: UdpSocket,
    mut external: watch::Receiver<bool>,
) -> io::Result<()>
where
    R: PacketReader,
    W: PacketWriter,
{
    let socket = Arc::new(socket);

    // bounded queues: outbound carries (next hop, packet), inbound carries packets for the tun.
    let (outbound_tx, outbound_rx) = mpsc::channel::<Outbound>(config.queue_depth);
    let (inbound_tx, inbound_rx) = mpsc::channel::<Vec<u8>>(config.queue_depth);

    // internal stop signal, so any finishing task can stop the others.
    let (stop_tx, stop_rx) = watch::channel(false);

    let mut tasks = JoinSet::new();
    tasks.spawn(tun_to_outbound(
        reader,
        outbound_tx.clone(),
        config.clone(),
        stop_rx.clone(),
    ));
    tasks.spawn(outbound_to_udp(
        outbound_rx,
        socket.clone(),
        stop_rx.clone(),
    ));
    tasks.spawn(udp_to_outbound(
        socket,
        inbound_tx,
        outbound_tx,
        config,
        stop_rx.clone(),
    ));
    tasks.spawn(inbound_to_tun(inbound_rx, writer, stop_rx));

    // forward the caller's shutdown to the internal signal.
    let stop_tx_bridge = stop_tx.clone();
    let bridge = tokio::spawn(async move {
        loop {
            if *external.borrow() {
                break;
            }
            if external.changed().await.is_err() {
                break;
            }
        }
        let _ = stop_tx_bridge.send(true);
    });

    let mut failure = None;
    while let Some(joined) = tasks.join_next().await {
        match joined {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                if failure.is_none() {
                    failure = Some(err);
                }
            }
            Err(err) => warn!(error = %err, "overlay task failed to join"),
        }
        // stop the others once one task finishes.
        let _ = stop_tx.send(true);
    }
    bridge.abort();

    match failure {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

// tun -> outbound queue. locally originated packets are routed but keep their ttl.
async fn tun_to_outbound<R: PacketReader>(
    mut reader: R,
    outbound: mpsc::Sender<Outbound>,
    config: OverlayConfig,
    mut stop: watch::Receiver<bool>,
) -> io::Result<()> {
    let mut buf = vec![0u8; config.mtu];
    loop {
        tokio::select! {
            _ = stop.changed() => {
                debug!("tun reader stopping");
                return Ok(());
            }
            result = reader.recv(&mut buf) => {
                let len = match result {
                    Ok(len) => len,
                    Err(err) => {
                        warn!(error = %err, "failed to read from tun");
                        return Err(err);
                    }
                };
                let packet = &buf[..len];
                if let Err(err) = packet::validate(packet) {
                    warn!(error = %err, len, "dropped outbound packet");
                    continue;
                }
                match forward::decide(packet, &config.local, &config.routes, &config.peers, None) {
                    Decision::Forward { next_hop } => {
                        if outbound.send((next_hop, packet.to_vec())).await.is_err() {
                            return Ok(());
                        }
                    }
                    Decision::Deliver => {
                        debug!("ignored locally originated packet addressed to this node");
                    }
                    Decision::Drop(reason) => {
                        warn!(reason = reason.as_str(), "dropped outbound packet");
                    }
                }
            }
        }
    }
}

// outbound queue -> udp.
async fn outbound_to_udp(
    mut outbound: mpsc::Receiver<Outbound>,
    socket: Arc<UdpSocket>,
    mut stop: watch::Receiver<bool>,
) -> io::Result<()> {
    loop {
        tokio::select! {
            _ = stop.changed() => {
                debug!("udp sender stopping");
                return Ok(());
            }
            item = outbound.recv() => {
                let Some((next_hop, packet)) = item else {
                    return Ok(());
                };
                if let Err(err) = transport::send_message(&socket, next_hop, &packet).await {
                    warn!(%next_hop, error = %err, "failed to send datagram");
                }
            }
        }
    }
}

// udp -> inbound queue (deliver) or outbound queue (forward). only known peers are accepted.
async fn udp_to_outbound(
    socket: Arc<UdpSocket>,
    inbound: mpsc::Sender<Vec<u8>>,
    outbound: mpsc::Sender<Outbound>,
    config: OverlayConfig,
    mut stop: watch::Receiver<bool>,
) -> io::Result<()> {
    let mut buf = vec![0u8; MAX_DATAGRAM_SIZE + 1];
    loop {
        tokio::select! {
            _ = stop.changed() => {
                debug!("udp receiver stopping");
                return Ok(());
            }
            result = transport::recv_datagram(&socket, &mut buf) => {
                let (len, from) = match result {
                    Ok(received) => received,
                    Err(err) => {
                        warn!(error = %err, "failed to receive datagram");
                        return Err(err);
                    }
                };
                if !config.peers.is_known(from) {
                    warn!(%from, "ignored datagram from unknown address");
                    continue;
                }
                let packet = match wire::decode(&buf[..len]) {
                    Ok(payload) => payload,
                    Err(err) => {
                        warn!(%from, error = %err, "dropped malformed datagram");
                        continue;
                    }
                };
                if let Err(err) = packet::validate(packet) {
                    warn!(%from, error = %err, "dropped invalid ip packet");
                    continue;
                }
                match forward::decide(packet, &config.local, &config.routes, &config.peers, Some(from)) {
                    Decision::Deliver => {
                        if inbound.send(packet.to_vec()).await.is_err() {
                            return Ok(());
                        }
                    }
                    Decision::Forward { next_hop } => {
                        let mut forwarded = packet.to_vec();
                        if !packet::decrement_hop_limit(&mut forwarded) {
                            warn!(%from, reason = DropReason::HopLimitExpired.as_str(), "dropped forwarded packet");
                            continue;
                        }
                        // a full queue drops the packet: forwarding must not block receives.
                        if let Err(err) = outbound.try_send((next_hop, forwarded)) {
                            warn!(%from, %next_hop, error = %err, "forward queue full; dropped packet");
                        }
                    }
                    Decision::Drop(reason) => {
                        warn!(%from, reason = reason.as_str(), "dropped received packet");
                    }
                }
            }
        }
    }
}

// inbound queue -> tun.
async fn inbound_to_tun<W: PacketWriter>(
    mut inbound: mpsc::Receiver<Vec<u8>>,
    mut writer: W,
    mut stop: watch::Receiver<bool>,
) -> io::Result<()> {
    loop {
        tokio::select! {
            _ = stop.changed() => {
                debug!("tun writer stopping");
                return Ok(());
            }
            packet = inbound.recv() => {
                let Some(packet) = packet else {
                    return Ok(());
                };
                if let Err(err) = writer.send(&packet).await {
                    warn!(error = %err, "failed to write to tun");
                    return Err(err);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route::Route;
    use std::future::Future;
    use tokio::time::{timeout, Duration};

    // fake tun backed by channels.
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

    struct FakeTun {
        reader: FakeReader,
        writer: FakeWriter,
        inject: mpsc::UnboundedSender<Vec<u8>>,
        collect: mpsc::UnboundedReceiver<Vec<u8>>,
    }

    fn fake_tun() -> FakeTun {
        let (inject, read_rx) = mpsc::unbounded_channel();
        let (write_tx, collect) = mpsc::unbounded_channel();
        FakeTun {
            reader: FakeReader { rx: read_rx },
            writer: FakeWriter { tx: write_tx },
            inject,
            collect,
        }
    }

    // a valid ipv4 packet with the given addresses and ttl.
    fn ipv4_packet(src: &str, dst: &str, ttl: u8, payload: usize) -> Vec<u8> {
        let total = 20 + payload;
        let mut packet = vec![0u8; total];
        packet[0] = 0x45;
        packet[2..4].copy_from_slice(&(total as u16).to_be_bytes());
        packet[8] = ttl;
        packet[9] = 17;
        packet[12..16].copy_from_slice(&src.parse::<Ipv4Addr>().unwrap().octets());
        packet[16..20].copy_from_slice(&dst.parse::<Ipv4Addr>().unwrap().octets());
        packet
    }

    async fn bind_free() -> UdpSocket {
        UdpSocket::bind("127.0.0.1:0").await.expect("bind")
    }

    fn config(local: &str, routes: Vec<Route>, peers: Vec<SocketAddr>) -> OverlayConfig {
        OverlayConfig {
            local: vec![local.parse().unwrap()],
            mtu: 1420,
            queue_depth: 16,
            routes: Arc::new(RoutingTable::new(routes)),
            peers: Arc::new(PeerSet::new(peers)),
        }
    }

    fn host_route(dest: &str, next_hop: SocketAddr) -> Route {
        Route::host(dest.parse().unwrap(), next_hop)
    }

    async fn shutdown(handle: tokio::task::JoinHandle<io::Result<()>>) {
        timeout(Duration::from_secs(2), handle)
            .await
            .expect("task did not stop")
            .expect("join")
            .expect("task returned an error");
    }

    #[tokio::test]
    async fn forwards_ip_packets_both_ways() {
        let a = fake_tun();
        let b = fake_tun();

        let a_socket = bind_free().await;
        let b_socket = bind_free().await;
        let a_addr = a_socket.local_addr().unwrap();
        let b_addr = b_socket.local_addr().unwrap();

        let (a_stop_tx, a_stop_rx) = watch::channel(false);
        let (b_stop_tx, b_stop_rx) = watch::channel(false);

        let a_config = config(
            "10.0.0.1",
            vec![host_route("10.0.0.2", b_addr)],
            vec![b_addr],
        );
        let b_config = config(
            "10.0.0.2",
            vec![host_route("10.0.0.1", a_addr)],
            vec![a_addr],
        );

        let a_task = tokio::spawn(run(a.reader, a.writer, a_config, a_socket, a_stop_rx));
        let b_task = tokio::spawn(run(b.reader, b.writer, b_config, b_socket, b_stop_rx));

        let mut a_collect = a.collect;
        let mut b_collect = b.collect;

        let outbound = ipv4_packet("10.0.0.1", "10.0.0.2", 64, 8);
        a.inject.send(outbound.clone()).unwrap();
        let got = timeout(Duration::from_secs(2), b_collect.recv())
            .await
            .expect("timed out waiting on b")
            .expect("b closed");
        assert_eq!(got, outbound, "locally originated packet keeps its ttl");

        let reverse = ipv4_packet("10.0.0.2", "10.0.0.1", 64, 16);
        b.inject.send(reverse.clone()).unwrap();
        let got = timeout(Duration::from_secs(2), a_collect.recv())
            .await
            .expect("timed out waiting on a")
            .expect("a closed");
        assert_eq!(got, reverse);

        a_stop_tx.send(true).unwrap();
        b_stop_tx.send(true).unwrap();
        shutdown(a_task).await;
        shutdown(b_task).await;
    }

    #[tokio::test]
    async fn drops_malformed_outbound_packets() {
        let a = fake_tun();
        let b = fake_tun();

        let a_socket = bind_free().await;
        let b_socket = bind_free().await;
        let a_addr = a_socket.local_addr().unwrap();
        let b_addr = b_socket.local_addr().unwrap();

        let (a_stop_tx, a_stop_rx) = watch::channel(false);
        let (b_stop_tx, b_stop_rx) = watch::channel(false);

        let a_config = config(
            "10.0.0.1",
            vec![host_route("10.0.0.2", b_addr)],
            vec![b_addr],
        );
        let b_config = config(
            "10.0.0.2",
            vec![host_route("10.0.0.1", a_addr)],
            vec![a_addr],
        );

        let a_task = tokio::spawn(run(a.reader, a.writer, a_config, a_socket, a_stop_rx));
        let b_task = tokio::spawn(run(b.reader, b.writer, b_config, b_socket, b_stop_rx));

        let mut b_collect = b.collect;

        a.inject.send(vec![0x00, 0x01, 0x02]).unwrap();
        assert!(
            timeout(Duration::from_millis(200), b_collect.recv())
                .await
                .is_err(),
            "malformed packet was forwarded"
        );

        let good = ipv4_packet("10.0.0.1", "10.0.0.2", 64, 8);
        a.inject.send(good.clone()).unwrap();
        let got = timeout(Duration::from_secs(2), b_collect.recv())
            .await
            .expect("timed out waiting on b")
            .expect("b closed");
        assert_eq!(got, good);

        a_stop_tx.send(true).unwrap();
        b_stop_tx.send(true).unwrap();
        shutdown(a_task).await;
        shutdown(b_task).await;
    }

    #[tokio::test]
    async fn drops_malformed_inbound_datagrams() {
        let a = fake_tun();

        let a_socket = bind_free().await;
        let a_addr = a_socket.local_addr().unwrap();
        let peer = bind_free().await;
        let peer_addr = peer.local_addr().unwrap();

        let (stop_tx, stop_rx) = watch::channel(false);
        let a_config = config("10.0.0.1", vec![], vec![peer_addr]);
        let a_task = tokio::spawn(run(a.reader, a.writer, a_config, a_socket, stop_rx));

        let mut a_collect = a.collect;

        // too short to be a frame.
        peer.send_to(&[0x01, 0x02], a_addr).await.unwrap();
        // a valid frame carrying a good packet for our address.
        let good = ipv4_packet("10.0.0.2", "10.0.0.1", 64, 8);
        let frame = wire::encode(&good).unwrap();
        peer.send_to(&frame, a_addr).await.unwrap();

        let got = timeout(Duration::from_secs(2), a_collect.recv())
            .await
            .expect("timed out waiting on a")
            .expect("a closed");
        assert_eq!(got, good, "only the valid packet should arrive");

        assert!(
            timeout(Duration::from_millis(200), a_collect.recv())
                .await
                .is_err(),
            "a second packet appeared"
        );

        stop_tx.send(true).unwrap();
        shutdown(a_task).await;
    }

    #[tokio::test]
    async fn ignores_datagrams_from_other_addresses() {
        let a = fake_tun();

        let a_socket = bind_free().await;
        let a_addr = a_socket.local_addr().unwrap();
        let peer = bind_free().await;
        let peer_addr = peer.local_addr().unwrap();
        let stranger = bind_free().await;

        let (stop_tx, stop_rx) = watch::channel(false);
        let a_config = config("10.0.0.1", vec![], vec![peer_addr]);
        let a_task = tokio::spawn(run(a.reader, a.writer, a_config, a_socket, stop_rx));

        let mut a_collect = a.collect;

        // valid frame for our address, but from the wrong source.
        let frame = wire::encode(&ipv4_packet("10.0.0.2", "10.0.0.1", 64, 8)).unwrap();
        stranger.send_to(&frame, a_addr).await.unwrap();
        assert!(
            timeout(Duration::from_millis(200), a_collect.recv())
                .await
                .is_err(),
            "accepted a datagram from a stranger"
        );

        stop_tx.send(true).unwrap();
        shutdown(a_task).await;
    }

    #[tokio::test]
    async fn dropping_the_shutdown_sender_stops_the_overlay() {
        let a = fake_tun();
        let a_socket = bind_free().await;
        let peer = bind_free().await;
        let peer_addr = peer.local_addr().unwrap();

        let (stop_tx, stop_rx) = watch::channel(false);
        let a_config = config("10.0.0.1", vec![], vec![peer_addr]);
        let a_task = tokio::spawn(run(a.reader, a.writer, a_config, a_socket, stop_rx));

        tokio::time::sleep(Duration::from_millis(25)).await;
        drop(stop_tx);

        shutdown(a_task).await;
    }
}
