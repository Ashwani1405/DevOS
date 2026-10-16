//! weave - a peer-to-peer layer 3 overlay network.
//!
//! right now it's the udp transport plus a tun overlay on top: two processes
//! can swap framed messages, or forward real ip packets between their tun
//! interfaces. wire.rs has the frame layout, transport.rs the socket side,
//! packet.rs packet validation, tun.rs the interface, overlay.rs the forwarding.
//!
//! not here yet: crypto, discovery, routing, failover. this is for local
//! development, not the real wire protocol.

pub mod overlay;
pub mod packet;
pub mod transport;
pub mod tun;
pub mod wire;
