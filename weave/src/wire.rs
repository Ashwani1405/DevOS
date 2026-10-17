//! frame layout for the udp transport. not the real weave wire protocol.
//!
//!   [ version 1B ][ len 2B BE ][ payload 0..N ]
//!
//! len has to match the payload bytes or the frame is dropped. datagram cap is
//! MAX_DATAGRAM_SIZE, so the body caps at MAX_PAYLOAD_LEN.
//!
//! the version byte says what the body means:
//! - `VERSION` (1): the body is a raw ip packet, ready to forward.
//! - `CONTROL_VERSION` (2): the body is a topology control message, handled by
//!   the control plane (`control.rs`) instead of being forwarded.

use std::error::Error;
use std::fmt;

/// bump this when the layout changes
pub const VERSION: u8 = 1;

/// version byte for control-plane frames. the body is a `control::Message`.
pub const CONTROL_VERSION: u8 = 2;

/// a parsed frame, tagged by what the body means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Frame<'a> {
    /// a raw ip packet to forward or deliver.
    Packet(&'a [u8]),
    /// a topology control message.
    Control(&'a [u8]),
}

/// version + 2 byte length
pub const HEADER_LEN: usize = 3;

/// keep under 1500 so we don't lean on IP fragmentation on a LAN
pub const MAX_DATAGRAM_SIZE: usize = 1500;

/// bytes left for the body
pub const MAX_PAYLOAD_LEN: usize = MAX_DATAGRAM_SIZE - HEADER_LEN;

/// pack payload into [version][len][payload].
/// errors if the payload is longer than MAX_PAYLOAD_LEN.
pub fn encode(payload: &[u8]) -> Result<Vec<u8>, WireError> {
    encode_versioned(VERSION, payload)
}

/// pack a topology control message into a control frame.
pub fn encode_control(payload: &[u8]) -> Result<Vec<u8>, WireError> {
    encode_versioned(CONTROL_VERSION, payload)
}

fn encode_versioned(version: u8, payload: &[u8]) -> Result<Vec<u8>, WireError> {
    if payload.len() > MAX_PAYLOAD_LEN {
        return Err(WireError::PayloadTooLarge {
            len: payload.len(),
            max: MAX_PAYLOAD_LEN,
        });
    }

    let payload_len = payload.len() as u16;
    let mut bytes = Vec::with_capacity(HEADER_LEN + payload.len());
    bytes.push(version);
    bytes.extend_from_slice(&payload_len.to_be_bytes());
    bytes.extend_from_slice(payload);
    Ok(bytes)
}

/// parse and validate a datagram, returning the payload. anything malformed
/// comes back as an error. only accepts ip-packet frames (version 1).
pub fn decode(datagram: &[u8]) -> Result<&[u8], WireError> {
    let (version, body) = parse(datagram)?;
    if version != VERSION {
        return Err(WireError::UnsupportedVersion(version));
    }
    Ok(body)
}

/// parse and validate a datagram, returning the tagged frame. accepts both
/// packet and control frames.
pub fn decode_frame(datagram: &[u8]) -> Result<Frame<'_>, WireError> {
    let (version, body) = parse(datagram)?;
    match version {
        VERSION => Ok(Frame::Packet(body)),
        CONTROL_VERSION => Ok(Frame::Control(body)),
        other => Err(WireError::UnsupportedVersion(other)),
    }
}

/// validate the common header and return the version plus body.
fn parse(datagram: &[u8]) -> Result<(u8, &[u8]), WireError> {
    if datagram.len() > MAX_DATAGRAM_SIZE {
        return Err(WireError::DatagramTooLarge {
            len: datagram.len(),
            max: MAX_DATAGRAM_SIZE,
        });
    }
    if datagram.len() < HEADER_LEN {
        return Err(WireError::TooShort {
            len: datagram.len(),
        });
    }

    let version = datagram[0];
    let declared = u16::from_be_bytes([datagram[1], datagram[2]]) as usize;
    let body = &datagram[HEADER_LEN..];
    if declared != body.len() {
        return Err(WireError::LengthMismatch {
            declared,
            actual: body.len(),
        });
    }

    Ok((version, body))
}

/// everything encode/decode can fail with
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WireError {
    /// shorter than the header
    TooShort {
        /// bytes we actually got
        len: usize,
    },
    /// bigger than MAX_DATAGRAM_SIZE
    DatagramTooLarge {
        /// bytes we actually got
        len: usize,
        /// the cap
        max: usize,
    },
    /// payload didn't fit while encoding
    PayloadTooLarge {
        /// payload bytes we were asked to encode
        len: usize,
        /// the cap
        max: usize,
    },
    /// version byte wasn't VERSION
    UnsupportedVersion(u8),
    /// header len didn't line up with the bytes that followed
    LengthMismatch {
        /// len from the header
        declared: usize,
        /// bytes actually present
        actual: usize,
    },
}

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort { len } => {
                write!(f, "datagram too short: {len} bytes (minimum {HEADER_LEN})")
            }
            Self::DatagramTooLarge { len, max } => {
                write!(f, "datagram too large: {len} bytes (maximum {max})")
            }
            Self::PayloadTooLarge { len, max } => {
                write!(f, "payload too large: {len} bytes (maximum {max})")
            }
            Self::UnsupportedVersion(version) => {
                write!(
                    f,
                    "unsupported protocol version: {version} (expected {VERSION})"
                )
            }
            Self::LengthMismatch { declared, actual } => write!(
                f,
                "payload length mismatch: header declares {declared} bytes but {actual} present"
            ),
        }
    }
}

impl Error for WireError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_payload() {
        let payload = b"payload";
        let bytes = encode(payload).expect("encode");
        assert_eq!(bytes.len(), HEADER_LEN + payload.len());
        assert_eq!(decode(&bytes).expect("decode"), payload);
    }

    #[test]
    fn encodes_empty_payload() {
        let bytes = encode(b"").expect("encode");
        assert_eq!(bytes, vec![VERSION, 0, 0]);
        assert_eq!(decode(&bytes).expect("decode"), &[] as &[u8]);
    }

    #[test]
    fn encode_rejects_payload_over_max() {
        let payload = vec![0u8; MAX_PAYLOAD_LEN + 1];
        assert_eq!(
            encode(&payload),
            Err(WireError::PayloadTooLarge {
                len: MAX_PAYLOAD_LEN + 1,
                max: MAX_PAYLOAD_LEN,
            })
        );
    }

    #[test]
    fn decode_rejects_short_datagram() {
        for datagram in [&[][..], &[VERSION][..], &[VERSION, 0x00][..]] {
            assert_eq!(
                decode(datagram),
                Err(WireError::TooShort {
                    len: datagram.len()
                })
            );
        }
    }

    #[test]
    fn decode_rejects_bad_version() {
        assert_eq!(
            decode(&[0x09, 0x00, 0x00]),
            Err(WireError::UnsupportedVersion(0x09))
        );
    }

    #[test]
    fn decode_rejects_length_mismatch() {
        assert_eq!(
            decode(&[VERSION, 0x00, 0x05, b'h', b'i']),
            Err(WireError::LengthMismatch {
                declared: 5,
                actual: 2,
            })
        );
    }

    #[test]
    fn decode_rejects_oversized_datagram() {
        let oversized = vec![0u8; MAX_DATAGRAM_SIZE + 1];
        assert_eq!(
            decode(&oversized),
            Err(WireError::DatagramTooLarge {
                len: MAX_DATAGRAM_SIZE + 1,
                max: MAX_DATAGRAM_SIZE,
            })
        );
    }

    #[test]
    fn control_frames_round_trip() {
        let body = b"control body";
        let bytes = encode_control(body).expect("encode");
        assert_eq!(bytes[0], CONTROL_VERSION);
        assert_eq!(decode_frame(&bytes).expect("decode"), Frame::Control(body));
        // the packet-only decoder still refuses a control frame.
        assert_eq!(
            decode(&bytes),
            Err(WireError::UnsupportedVersion(CONTROL_VERSION))
        );
    }

    #[test]
    fn decode_frame_tags_packet_frames() {
        let bytes = encode(b"packet").expect("encode");
        assert_eq!(
            decode_frame(&bytes).expect("decode"),
            Frame::Packet(b"packet")
        );
    }

    #[test]
    fn decode_frame_rejects_unknown_version() {
        assert_eq!(
            decode_frame(&[0x07, 0x00, 0x00]),
            Err(WireError::UnsupportedVersion(0x07))
        );
    }

    #[test]
    fn decode_rejects_truncated_payload() {
        // header says 200 payload bytes, none actually follow
        assert_eq!(
            decode(&[VERSION, 0x00, 200]),
            Err(WireError::LengthMismatch {
                declared: 200,
                actual: 0,
            })
        );
    }
}
