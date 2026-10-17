//! the routing table: destination prefix -> next hop.
//!
//! this only holds the mapping; it knows nothing about sockets or tun devices.
//! lookup picks the longest matching prefix, breaking ties on the lowest metric,
//! and callers must handle a missing route themselves.
//!
//! routes are static for now. a later milestone can advertise and withdraw them,
//! which is why each one carries a source.

use std::error::Error;
use std::fmt;
use std::net::{Ipv4Addr, SocketAddr};
use std::str::FromStr;

/// an ipv4 prefix, e.g. `10.0.0.0/24` or a single host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ipv4Prefix {
    addr: Ipv4Addr,
    len: u8,
}

impl Ipv4Prefix {
    /// build a prefix. host bits past `len` are masked off.
    pub fn new(addr: Ipv4Addr, len: u8) -> Result<Self, RouteError> {
        if len > 32 {
            return Err(RouteError::PrefixTooLong(len));
        }
        Ok(Self {
            addr: mask(addr, len),
            len,
        })
    }

    /// a single host, `/32`.
    pub fn host(addr: Ipv4Addr) -> Self {
        Self { addr, len: 32 }
    }

    /// the network address.
    pub fn addr(self) -> Ipv4Addr {
        self.addr
    }

    /// prefix length in bits.
    pub fn prefix_len(self) -> u8 {
        self.len
    }

    /// does `addr` fall inside this prefix?
    pub fn contains(self, addr: Ipv4Addr) -> bool {
        mask(addr, self.len) == self.addr
    }
}

impl fmt::Display for Ipv4Prefix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.addr, self.len)
    }
}

impl FromStr for Ipv4Prefix {
    type Err = RouteError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (addr, len) = match s.split_once('/') {
            Some((addr, len)) => (addr, Some(len)),
            None => (s, None),
        };
        let addr = addr
            .parse::<Ipv4Addr>()
            .map_err(|_| RouteError::BadPrefix(s.to_owned()))?;
        let len = match len {
            Some(len) => len
                .parse::<u8>()
                .map_err(|_| RouteError::BadPrefix(s.to_owned()))?,
            None => 32,
        };
        Ipv4Prefix::new(addr, len).map_err(|_| RouteError::BadPrefix(s.to_owned()))
    }
}

/// where a route came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteSource {
    /// configured by hand.
    Static,
}

impl RouteSource {
    /// short label for logs.
    pub fn as_str(self) -> &'static str {
        match self {
            RouteSource::Static => "static",
        }
    }
}

/// one entry: send traffic for `dest` to `next_hop`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    /// destination prefix.
    pub dest: Ipv4Prefix,
    /// udp endpoint of the peer that is the next hop.
    pub next_hop: SocketAddr,
    /// cost of the route; lower wins on a tie.
    pub metric: u32,
    /// how the route got here.
    pub source: RouteSource,
}

impl Route {
    /// a static host route to `addr` via `next_hop`.
    pub fn host(addr: Ipv4Addr, next_hop: SocketAddr) -> Self {
        Self {
            dest: Ipv4Prefix::host(addr),
            next_hop,
            metric: 1,
            source: RouteSource::Static,
        }
    }
}

/// `"<prefix>=<next_hop>"`, the cli form of a static route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteSpec {
    /// destination prefix.
    pub dest: Ipv4Prefix,
    /// udp endpoint of the next hop.
    pub next_hop: SocketAddr,
}

impl RouteSpec {
    /// turn the spec into a static route.
    pub fn into_route(self) -> Route {
        Route {
            dest: self.dest,
            next_hop: self.next_hop,
            metric: 1,
            source: RouteSource::Static,
        }
    }
}

impl FromStr for RouteSpec {
    type Err = RouteError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (dest, next_hop) = s
            .split_once('=')
            .ok_or_else(|| RouteError::BadRouteSpec(s.to_owned()))?;
        let dest = dest
            .parse::<Ipv4Prefix>()
            .map_err(|_| RouteError::BadRouteSpec(s.to_owned()))?;
        let next_hop = next_hop
            .parse::<SocketAddr>()
            .map_err(|_| RouteError::BadRouteSpec(s.to_owned()))?;
        Ok(Self { dest, next_hop })
    }
}

/// a set of routes with longest-prefix lookup.
#[derive(Debug, Clone, Default)]
pub struct RoutingTable {
    routes: Vec<Route>,
}

impl RoutingTable {
    /// build a table from routes.
    pub fn new(routes: Vec<Route>) -> Self {
        Self { routes }
    }

    /// every route, in insertion order.
    pub fn routes(&self) -> &[Route] {
        &self.routes
    }

    /// number of routes.
    pub fn len(&self) -> usize {
        self.routes.len()
    }

    /// true when there are no routes.
    pub fn is_empty(&self) -> bool {
        self.routes.is_empty()
    }

    /// the best route for `dest`, or `None` when nothing matches.
    pub fn lookup(&self, dest: Ipv4Addr) -> Option<&Route> {
        self.routes
            .iter()
            .filter(|route| route.dest.contains(dest))
            .min_by(|a, b| {
                b.dest
                    .prefix_len()
                    .cmp(&a.dest.prefix_len())
                    .then(a.metric.cmp(&b.metric))
            })
    }
}

/// what can go wrong building a prefix or parsing a spec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteError {
    /// a prefix length beyond 32.
    PrefixTooLong(u8),
    /// not a `<addr>` or `<addr>/<len>`.
    BadPrefix(String),
    /// not a `<prefix>=<addr:port>` spec.
    BadRouteSpec(String),
}

impl fmt::Display for RouteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PrefixTooLong(len) => write!(f, "prefix length {len} is more than 32"),
            Self::BadPrefix(prefix) => write!(f, "invalid ipv4 prefix: {prefix}"),
            Self::BadRouteSpec(spec) => write!(f, "invalid route spec: {spec}"),
        }
    }
}

impl Error for RouteError {}

/// zero the host bits of `addr` above `len`.
fn mask(addr: Ipv4Addr, len: u8) -> Ipv4Addr {
    if len == 0 {
        return Ipv4Addr::UNSPECIFIED;
    }
    let value = u32::from(addr) & (u32::MAX << (32 - u32::from(len)));
    Ipv4Addr::from(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hop(port: u16) -> SocketAddr {
        format!("127.0.0.1:{port}").parse().unwrap()
    }

    fn ip(s: &str) -> Ipv4Addr {
        s.parse().unwrap()
    }

    #[test]
    fn parses_prefixes() {
        let host: Ipv4Prefix = "10.0.0.3".parse().unwrap();
        assert_eq!(host, Ipv4Prefix::host(ip("10.0.0.3")));
        assert_eq!(host.prefix_len(), 32);

        let net: Ipv4Prefix = "10.0.0.0/24".parse().unwrap();
        assert_eq!(net.prefix_len(), 24);
        assert!(net.contains(ip("10.0.0.200")));
        assert!(!net.contains(ip("10.0.1.1")));
    }

    #[test]
    fn masks_host_bits() {
        let prefix = Ipv4Prefix::new(ip("10.0.0.7"), 24).unwrap();
        assert_eq!(prefix.addr(), ip("10.0.0.0"));
    }

    #[test]
    fn rejects_long_and_bad_prefixes() {
        assert_eq!(
            Ipv4Prefix::new(ip("10.0.0.1"), 33),
            Err(RouteError::PrefixTooLong(33))
        );
        assert!("10.0.0.0/99".parse::<Ipv4Prefix>().is_err());
        assert!("not-an-ip".parse::<Ipv4Prefix>().is_err());
    }

    #[test]
    fn lookup_picks_the_longest_prefix() {
        let table = RoutingTable::new(vec![
            Route {
                dest: "0.0.0.0/0".parse().unwrap(),
                next_hop: hop(1),
                metric: 1,
                source: RouteSource::Static,
            },
            Route {
                dest: "10.0.0.0/24".parse().unwrap(),
                next_hop: hop(2),
                metric: 1,
                source: RouteSource::Static,
            },
        ]);

        assert_eq!(table.lookup(ip("10.0.0.5")).unwrap().next_hop, hop(2));
        assert_eq!(table.lookup(ip("8.8.8.8")).unwrap().next_hop, hop(1));
    }

    #[test]
    fn lookup_breaks_ties_by_metric() {
        let table = RoutingTable::new(vec![
            Route {
                dest: "10.0.0.0/24".parse().unwrap(),
                next_hop: hop(1),
                metric: 20,
                source: RouteSource::Static,
            },
            Route {
                dest: "10.0.0.0/24".parse().unwrap(),
                next_hop: hop(2),
                metric: 5,
                source: RouteSource::Static,
            },
        ]);

        assert_eq!(table.lookup(ip("10.0.0.9")).unwrap().next_hop, hop(2));
    }

    #[test]
    fn lookup_without_a_match_is_none() {
        let table = RoutingTable::new(vec![Route::host(ip("10.0.0.3"), hop(3))]);
        assert!(table.lookup(ip("10.0.0.4")).is_none());
        assert!(RoutingTable::default().lookup(ip("10.0.0.3")).is_none());
    }

    #[test]
    fn parses_route_specs() {
        let spec: RouteSpec = "10.0.0.3/32=127.0.0.1:4003".parse().unwrap();
        let route = spec.into_route();
        assert_eq!(route.dest, Ipv4Prefix::host(ip("10.0.0.3")));
        assert_eq!(route.next_hop, hop(4003));
        assert_eq!(route.source, RouteSource::Static);

        assert!("10.0.0.3".parse::<RouteSpec>().is_err());
        assert!("10.0.0.3/32=nope".parse::<RouteSpec>().is_err());
    }
}
