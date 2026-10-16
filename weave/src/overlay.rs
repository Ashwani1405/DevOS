//! the forwarding loop.
//!
//! glues a tun interface to the udp transport in both directions. each
//! direction is a small pipeline of two tasks joined by a *bounded* queue, so a
//! slow side slows the fast side instead of letting memory grow without bound.

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;

use tokio::net::UdpSocket;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinSet;
use tracing::{debug, warn};

use crate::packet;
use crate::transport;
use crate::tun::{PacketReader, PacketWriter};
use crate::wire::{self, MAX_DATAGRAM_SIZE};

/// everything the forwarding loop needs.
#[derive(Debug, Clone)]
pub struct OverlayConfig {
    /// the udp endpoint of the other node.
    pub peer: SocketAddr,
    /// largest ip packet we accept from the interface.
    pub mtu: usize,
    /// how many packets may queue in each direction before we stop reading.
    pub queue_depth: usize,
}

/// run both directions until shutdown, or until a task gives up.
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

    // one bounded queue per direction, this is the whole backpressure story.
    let (outbound_tx, outbound_rx) = mpsc::channel(config.queue_depth);
    let (inbound_tx, inbound_rx) = mpsc::channel(config.queue_depth);

    // our own stop signal, so any finishing task can stop the others.
    let (stop_tx, stop_rx) = watch::channel(false);

    let mut tasks = JoinSet::new();
    tasks.spawn(tun_to_queue(
        reader,
        outbound_tx,
        stop_rx.clone(),
        config.mtu,
    ));
    tasks.spawn(queue_to_udp(
        outbound_rx,
        socket.clone(),
        config.peer,
        stop_rx.clone(),
    ));
    tasks.spawn(udp_to_queue(
        socket,
        inbound_tx,
        config.peer,
        stop_rx.clone(),
    ));
    tasks.spawn(queue_to_tun(inbound_rx, writer, stop_rx));

    // bridge the caller's shutdown into our own stop signal.
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
        // whichever side finished first, wind the rest down.
        let _ = stop_tx.send(true);
    }
    bridge.abort();

    match failure {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

// tun -> outbound queue. validating here keeps junk off the wire.
async fn tun_to_queue<R: PacketReader>(
    mut reader: R,
    outbound: mpsc::Sender<Vec<u8>>,
    mut stop: watch::Receiver<bool>,
    mtu: usize,
) -> io::Result<()> {
    let mut buf = vec![0u8; mtu];
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
                match packet::validate(&buf[..len]) {
                    Ok(()) => {
                        if outbound.send(buf[..len].to_vec()).await.is_err() {
                            return Ok(());
                        }
                    }
                    Err(err) => warn!(error = %err, len, "dropped outbound packet"),
                }
            }
        }
    }
}

// outbound queue -> udp. waiting on the bounded send is what applies backpressure.
async fn queue_to_udp(
    mut outbound: mpsc::Receiver<Vec<u8>>,
    socket: Arc<UdpSocket>,
    peer: SocketAddr,
    mut stop: watch::Receiver<bool>,
) -> io::Result<()> {
    loop {
        tokio::select! {
            _ = stop.changed() => {
                debug!("udp sender stopping");
                return Ok(());
            }
            packet = outbound.recv() => {
                let Some(packet) = packet else {
                    return Ok(());
                };
                if let Err(err) = transport::send_message(&socket, peer, &packet).await {
                    warn!(error = %err, "failed to send datagram");
                }
            }
        }
    }
}

// udp -> inbound queue. only the configured peer is trusted.
async fn udp_to_queue(
    socket: Arc<UdpSocket>,
    inbound: mpsc::Sender<Vec<u8>>,
    peer: SocketAddr,
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
                if from != peer {
                    warn!(%from, "ignored datagram from unexpected address");
                    continue;
                }
                let payload = match wire::decode(&buf[..len]) {
                    Ok(payload) => payload,
                    Err(err) => {
                        warn!(%from, error = %err, "dropped malformed datagram");
                        continue;
                    }
                };
                match packet::validate(payload) {
                    Ok(()) => {
                        if inbound.send(payload.to_vec()).await.is_err() {
                            return Ok(());
                        }
                    }
                    Err(err) => warn!(%from, error = %err, "dropped invalid ip packet"),
                }
            }
        }
    }
}

// inbound queue -> tun.
async fn queue_to_tun<W: PacketWriter>(
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
    use std::future::Future;
    use tokio::time::{timeout, Duration};

    // a tun stand-in backed by channels: no root, no kernel.
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

    // a well-formed ipv4/udp packet of 20 + payload bytes.
    fn ipv4_packet(payload: usize) -> Vec<u8> {
        let total = 20 + payload;
        let mut packet = vec![0u8; total];
        packet[0] = 0x45;
        packet[2..4].copy_from_slice(&(total as u16).to_be_bytes());
        packet[9] = 17;
        packet
    }

    async fn bind_free() -> UdpSocket {
        UdpSocket::bind("127.0.0.1:0").await.expect("bind")
    }

    fn config(peer: SocketAddr) -> OverlayConfig {
        OverlayConfig {
            peer,
            mtu: 1420,
            queue_depth: 16,
        }
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

        let a_task = tokio::spawn(run(a.reader, a.writer, config(b_addr), a_socket, a_stop_rx));
        let b_task = tokio::spawn(run(b.reader, b.writer, config(a_addr), b_socket, b_stop_rx));

        let mut a_collect = a.collect;
        let mut b_collect = b.collect;

        let outbound = ipv4_packet(8);
        a.inject.send(outbound.clone()).unwrap();
        let got = timeout(Duration::from_secs(2), b_collect.recv())
            .await
            .expect("timed out waiting on b")
            .expect("b closed");
        assert_eq!(got, outbound);

        let reverse = ipv4_packet(16);
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

        let a_task = tokio::spawn(run(a.reader, a.writer, config(b_addr), a_socket, a_stop_rx));
        let b_task = tokio::spawn(run(b.reader, b.writer, config(a_addr), b_socket, b_stop_rx));

        let mut b_collect = b.collect;

        // not even a header, should never leave node a
        a.inject.send(vec![0x00, 0x01, 0x02]).unwrap();
        assert!(
            timeout(Duration::from_millis(200), b_collect.recv())
                .await
                .is_err(),
            "malformed packet was forwarded"
        );

        // a real one still gets through, proving the loop survived
        let good = ipv4_packet(8);
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
        let a_task = tokio::spawn(run(
            a.reader,
            a.writer,
            config(peer_addr),
            a_socket,
            stop_rx,
        ));

        let mut a_collect = a.collect;

        // too short to be a frame
        peer.send_to(&[0x01, 0x02], a_addr).await.unwrap();
        // a valid frame carrying a good packet
        let good = ipv4_packet(8);
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
        let a_task = tokio::spawn(run(
            a.reader,
            a.writer,
            config(peer_addr),
            a_socket,
            stop_rx,
        ));

        let mut a_collect = a.collect;

        // a perfectly valid frame, but from the wrong source
        let frame = wire::encode(&ipv4_packet(8)).unwrap();
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
        let a_task = tokio::spawn(run(
            a.reader,
            a.writer,
            config(peer_addr),
            a_socket,
            stop_rx,
        ));

        tokio::time::sleep(Duration::from_millis(25)).await;
        drop(stop_tx);

        shutdown(a_task).await;
    }
}
