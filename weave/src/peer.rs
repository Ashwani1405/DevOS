//! minimal peer availability.
//!
//! the m3 lifecycle monitor (probing, suspect/connected/disconnected) is not
//! implemented yet. routing only needs to know which peers are configured and
//! whether each is currently usable, so that is all this does for now. when m3
//! lands, `is_available` should read the live peer state instead of a flag.
//!
//! this is deliberately not the m3 state machine; treat it as a stand-in.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};

/// a configured peer and whether it can currently be used as a next hop.
#[derive(Debug)]
struct PeerLink {
    available: AtomicBool,
}

/// the peers a node knows about.
#[derive(Debug, Default)]
pub struct PeerSet {
    peers: HashMap<SocketAddr, PeerLink>,
}

impl PeerSet {
    /// build a set from peer addresses, all marked available.
    pub fn new(addrs: impl IntoIterator<Item = SocketAddr>) -> Self {
        let peers = addrs
            .into_iter()
            .map(|addr| {
                (
                    addr,
                    PeerLink {
                        available: AtomicBool::new(true),
                    },
                )
            })
            .collect();
        Self { peers }
    }

    /// is this a configured peer? unknown addresses are ignored on receive.
    pub fn is_known(&self, addr: SocketAddr) -> bool {
        self.peers.contains_key(&addr)
    }

    /// can this peer be used as a next hop right now? unknown peers are not.
    pub fn is_available(&self, addr: SocketAddr) -> bool {
        self.peers
            .get(&addr)
            .is_some_and(|peer| peer.available.load(Ordering::Relaxed))
    }

    /// mark a peer usable or not. returns false if it wasn't known.
    pub fn set_available(&self, addr: SocketAddr, available: bool) -> bool {
        match self.peers.get(&addr) {
            Some(peer) => {
                peer.available.store(available, Ordering::Relaxed);
                true
            }
            None => false,
        }
    }

    /// number of configured peers.
    pub fn len(&self) -> usize {
        self.peers.len()
    }

    /// true when no peers are configured.
    pub fn is_empty(&self) -> bool {
        self.peers.is_empty()
    }

    /// every configured peer address.
    pub fn addrs(&self) -> impl Iterator<Item = SocketAddr> + '_ {
        self.peers.keys().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(port: u16) -> SocketAddr {
        format!("127.0.0.1:{port}").parse().unwrap()
    }

    #[test]
    fn tracks_known_and_available_peers() {
        let peers = PeerSet::new([addr(1), addr(2)]);
        assert_eq!(peers.len(), 2);
        assert!(peers.is_known(addr(1)));
        assert!(!peers.is_known(addr(3)));
        assert!(peers.is_available(addr(1)));
        assert!(
            !peers.is_available(addr(3)),
            "unknown peers are unavailable"
        );
    }

    #[test]
    fn availability_can_be_flipped() {
        let peers = PeerSet::new([addr(1)]);
        assert!(peers.set_available(addr(1), false));
        assert!(!peers.is_available(addr(1)));
        assert!(peers.set_available(addr(1), true));
        assert!(peers.is_available(addr(1)));
        assert!(!peers.set_available(addr(9), false), "unknown peer");
    }
}
