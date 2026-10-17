//! weave - a peer-to-peer layer 3 overlay network.
//!
//! two modes: a udp transport that swaps framed messages, and a tun overlay
//! that moves ip packets between interfaces. wire.rs has the frame format,
//! transport.rs the socket, packet.rs validation, tun.rs the interface,
//! route.rs the routing table, forward.rs the forwarding decision, peer.rs
//! peer availability and overlay.rs the packet pipeline.
//!
//! routing is static for now. no crypto, discovery or automatic failover yet.

pub mod forward;
pub mod overlay;
pub mod packet;
pub mod peer;
pub mod route;
pub mod transport;
pub mod tun;
pub mod wire;
