//! ip packet validation for the overlay.
//!
//! before a packet goes anywhere we check it is well formed: a known ip
//! version, a header that fits, and a declared length that lines up with the
//! bytes we actually got. anything else is dropped, never forwarded.

use std::error::Error;
use std::fmt;

/// smallest possible ipv4 header, with no options.
pub const IPV4_MIN_HEADER_LEN: usize = 20;

/// ipv6 has a fixed size header.
pub const IPV6_HEADER_LEN: usize = 40;

/// why a packet didn't validate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PacketError {
    /// too few bytes to even hold the header.
    TooShort {
        /// bytes we actually got.
        len: usize,
    },
    /// first nibble wasn't 4 or 6.
    UnsupportedVersion(u8),
    /// ipv4 ihl is below the minimum or points past the end of the slice.
    InvalidIpv4HeaderLen {
        /// internet header length in 32-bit words.
        ihl: u8,
        /// bytes we actually got.
        len: usize,
    },
    /// the header's total length doesn't match the bytes present.
    LengthMismatch {
        /// total length taken from the header.
        declared: usize,
        /// bytes actually present.
        actual: usize,
    },
}

/// check that a slice is a well-formed ipv4 or ipv6 packet.
pub fn validate(packet: &[u8]) -> Result<(), PacketError> {
    let Some(&first) = packet.first() else {
        return Err(PacketError::TooShort { len: 0 });
    };

    match first >> 4 {
        4 => validate_ipv4(packet),
        6 => validate_ipv6(packet),
        version => Err(PacketError::UnsupportedVersion(version)),
    }
}

fn validate_ipv4(packet: &[u8]) -> Result<(), PacketError> {
    if packet.len() < IPV4_MIN_HEADER_LEN {
        return Err(PacketError::TooShort { len: packet.len() });
    }

    let ihl = packet[0] & 0x0f;
    let header_len = usize::from(ihl) * 4;
    if ihl < 5 || header_len > packet.len() {
        return Err(PacketError::InvalidIpv4HeaderLen {
            ihl,
            len: packet.len(),
        });
    }

    let declared = usize::from(u16::from_be_bytes([packet[2], packet[3]]));
    if declared != packet.len() {
        return Err(PacketError::LengthMismatch {
            declared,
            actual: packet.len(),
        });
    }

    Ok(())
}

fn validate_ipv6(packet: &[u8]) -> Result<(), PacketError> {
    if packet.len() < IPV6_HEADER_LEN {
        return Err(PacketError::TooShort { len: packet.len() });
    }

    let payload_len = usize::from(u16::from_be_bytes([packet[4], packet[5]]));
    let declared = IPV6_HEADER_LEN + payload_len;
    if declared != packet.len() {
        return Err(PacketError::LengthMismatch {
            declared,
            actual: packet.len(),
        });
    }

    Ok(())
}

impl fmt::Display for PacketError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort { len } => write!(f, "packet too short: {len} bytes"),
            Self::UnsupportedVersion(version) => {
                write!(f, "unsupported ip version: {version}")
            }
            Self::InvalidIpv4HeaderLen { ihl, len } => {
                write!(
                    f,
                    "invalid ipv4 header length: ihl {ihl} with {len} bytes present"
                )
            }
            Self::LengthMismatch { declared, actual } => write!(
                f,
                "packet length mismatch: header says {declared} bytes but {actual} present"
            ),
        }
    }
}

impl Error for PacketError {}

#[cfg(test)]
mod tests {
    use super::*;

    // a valid ipv4 packet of `len` bytes (len >= 20), ihl 5, total length set.
    fn ipv4(len: usize) -> Vec<u8> {
        let mut packet = vec![0u8; len];
        packet[0] = 0x45;
        packet[2..4].copy_from_slice(&(len as u16).to_be_bytes());
        packet[9] = 17; // udp
        packet
    }

    fn ipv6(payload_len: usize) -> Vec<u8> {
        let total = IPV6_HEADER_LEN + payload_len;
        let mut packet = vec![0u8; total];
        packet[0] = 0x60;
        packet[4..6].copy_from_slice(&(payload_len as u16).to_be_bytes());
        packet[6] = 17; // next header: udp
        packet
    }

    #[test]
    fn accepts_a_valid_ipv4_packet() {
        assert_eq!(validate(&ipv4(20)), Ok(()));
        assert_eq!(validate(&ipv4(40)), Ok(()));
    }

    #[test]
    fn accepts_a_valid_ipv4_packet_with_options() {
        let mut packet = ipv4(24);
        packet[0] = 0x46; // ihl 6 -> 24 byte header
        assert_eq!(validate(&packet), Ok(()));
    }

    #[test]
    fn rejects_empty_and_short_packets() {
        assert_eq!(validate(&[]), Err(PacketError::TooShort { len: 0 }));
        assert_eq!(validate(&ipv4(19)), Err(PacketError::TooShort { len: 19 }));
    }

    #[test]
    fn rejects_unknown_version() {
        assert_eq!(
            validate(&[0x50, 0, 0, 0, 0]),
            Err(PacketError::UnsupportedVersion(5))
        );
    }

    #[test]
    fn rejects_bad_ipv4_header_len() {
        let mut too_small = ipv4(20);
        too_small[0] = 0x44; // ihl 4
        assert_eq!(
            validate(&too_small),
            Err(PacketError::InvalidIpv4HeaderLen { ihl: 4, len: 20 })
        );

        let mut past_end = ipv4(20);
        past_end[0] = 0x4f; // ihl 15 -> 60 bytes, more than present
        assert_eq!(
            validate(&past_end),
            Err(PacketError::InvalidIpv4HeaderLen { ihl: 15, len: 20 })
        );
    }

    #[test]
    fn rejects_ipv4_length_mismatch() {
        let mut packet = ipv4(20);
        packet[2..4].copy_from_slice(&100u16.to_be_bytes());
        assert_eq!(
            validate(&packet),
            Err(PacketError::LengthMismatch {
                declared: 100,
                actual: 20
            })
        );
    }

    #[test]
    fn accepts_a_valid_ipv6_packet() {
        assert_eq!(validate(&ipv6(0)), Ok(()));
        assert_eq!(validate(&ipv6(16)), Ok(()));
    }

    #[test]
    fn rejects_short_ipv6_packet() {
        assert_eq!(
            validate(&ipv6(0)[..IPV6_HEADER_LEN - 1]),
            Err(PacketError::TooShort {
                len: IPV6_HEADER_LEN - 1
            })
        );
    }

    #[test]
    fn rejects_ipv6_length_mismatch() {
        let mut packet = ipv6(8);
        packet[4..6].copy_from_slice(&64u16.to_be_bytes());
        assert_eq!(
            validate(&packet),
            Err(PacketError::LengthMismatch {
                declared: IPV6_HEADER_LEN + 64,
                actual: IPV6_HEADER_LEN + 8
            })
        );
    }
}
