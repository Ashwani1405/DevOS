//! weave - a peer-to-peer layer 3 overlay network.
//!
//! two modes: a udp transport that swaps framed messages, and a tun overlay
//! that moves ip packets between interfaces. wire.rs has the frame format,
//! transport.rs the socket, packet.rs validation, tun.rs the interface,
//! route.rs the routing table, forward.rs the forwarding decision, peer.rs
//! peer availability and overlay.rs the packet pipeline.
//!
//! topology.rs models the network as a graph and calculates routes with
//! dijkstra; control.rs encodes the link-state messages peers exchange;
//! router.rs is the control plane that floods updates, detects failed links and
//! republishes the routing table. routing is static unless a topology is
//! configured. no crypto, discovery or endpoint migration yet.

pub mod control;
pub mod forward;
pub mod overlay;
pub mod packet;
pub mod peer;
pub mod route;
pub mod router;
pub mod topology;
pub mod transport;
pub mod tun;
pub mod wire;
