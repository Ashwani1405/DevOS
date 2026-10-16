//! tun device handling.
//!
//! kept apart from the udp transport on purpose: this module only knows how to
//! open an interface and move single ip packets. the forwarding policy lives in
//! `overlay`.
//!
//! opening needs root on every platform. on macos you get a `utun` interface
//! and the name has to look like `utunN` (or be omitted, letting the kernel
//! pick the next free one).

use std::future::Future;
use std::io;
use std::net::Ipv4Addr;

/// read half of a tun interface. one call yields one ip packet.
pub trait PacketReader: Send + 'static {
    /// read a single packet into `buf`, returning how many bytes it was.
    fn recv<'a>(
        &'a mut self,
        buf: &'a mut [u8],
    ) -> impl Future<Output = io::Result<usize>> + Send + 'a;
}

/// write half of a tun interface. one call injects one ip packet.
pub trait PacketWriter: Send + 'static {
    /// inject a single packet.
    fn send<'a>(&'a mut self, packet: &'a [u8])
        -> impl Future<Output = io::Result<()>> + Send + 'a;
}

/// how to create and configure the interface.
#[derive(Debug, Clone)]
pub struct TunConfig {
    /// requested name. on macos this is `utunN`; `None` lets the kernel choose.
    pub name: Option<String>,
    /// overlay address for this node.
    pub address: Ipv4Addr,
    /// the peer's overlay address (utun is point-to-point).
    pub destination: Ipv4Addr,
    /// netmask; a single host is `255.255.255.255`.
    pub netmask: Ipv4Addr,
    /// interface mtu.
    pub mtu: u16,
    /// let the platform library add routes for us.
    pub enable_routing: bool,
}

/// an opened tun interface, ready to be split into reader and writer.
pub struct TunDevice {
    device: tun::AsyncDevice,
}

impl TunDevice {
    /// create and configure the interface. needs root.
    pub fn open(config: &TunConfig) -> io::Result<Self> {
        let mut tun_config = tun::Configuration::default();
        if let Some(name) = &config.name {
            tun_config.tun_name(name);
        }
        tun_config
            .address(config.address)
            .destination(config.destination)
            .netmask(config.netmask)
            .mtu(config.mtu)
            .up();
        tun_config.platform_config(|platform| {
            platform.enable_routing(config.enable_routing);
        });

        let device = tun::create_as_async(&tun_config).map_err(io::Error::other)?;
        Ok(Self { device })
    }

    /// split into independent read and write halves so the two directions can
    /// run as separate tasks.
    pub fn split(self) -> io::Result<(TunReader, TunWriter)> {
        let (writer, reader) = self.device.split()?;
        Ok((TunReader { reader }, TunWriter { writer }))
    }
}

/// read half of an opened interface.
pub struct TunReader {
    reader: tun::DeviceReader,
}

/// write half of an opened interface.
pub struct TunWriter {
    writer: tun::DeviceWriter,
}

#[allow(clippy::manual_async_fn)] // need the explicit `Send` bound for spawning
impl PacketReader for TunReader {
    fn recv<'a>(
        &'a mut self,
        buf: &'a mut [u8],
    ) -> impl Future<Output = io::Result<usize>> + Send + 'a {
        async move { tokio::io::AsyncReadExt::read(&mut self.reader, buf).await }
    }
}

#[allow(clippy::manual_async_fn)] // need the explicit `Send` bound for spawning
impl PacketWriter for TunWriter {
    fn send<'a>(
        &'a mut self,
        packet: &'a [u8],
    ) -> impl Future<Output = io::Result<()>> + Send + 'a {
        async move { tokio::io::AsyncWriteExt::write_all(&mut self.writer, packet).await }
    }
}
