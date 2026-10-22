//! Optional parameters: tag, length, value.
//!
//! Any tag is accepted, known or not. Sending an optional parameter the tool
//! has never heard of is a thing a tester needs to do, so an unknown tag
//! round-trips byte for byte.

use crate::smpp::constants::tlv_name;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tlv {
    pub tag: u16,
    pub value: Vec<u8>,
}

impl Tlv {
    pub fn new(tag: u16, value: Vec<u8>) -> Self {
        Self { tag, value }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TlvError {
    #[error("truncated TLV header at offset {offset}: {left} byte(s) left")]
    TruncatedHeader { offset: usize, left: usize },
    #[error("TLV {name} declares {declared} byte(s) but only {left} remain")]
    LengthPastEnd {
        name: String,
        declared: usize,
        left: usize,
    },
}

pub fn encode_tlvs(tlvs: &[Tlv]) -> Vec<u8> {
    let mut out = Vec::new();
    for tlv in tlvs {
        out.extend_from_slice(&tlv.tag.to_be_bytes());
        out.extend_from_slice(&(tlv.value.len() as u16).to_be_bytes());
        out.extend_from_slice(&tlv.value);
    }
    out
}

pub fn decode_tlvs(buf: &[u8]) -> Result<Vec<Tlv>, TlvError> {
    let mut out = Vec::new();
    let mut i = 0usize;

    while i < buf.len() {
        if i + 4 > buf.len() {
            return Err(TlvError::TruncatedHeader {
                offset: i,
                left: buf.len() - i,
            });
        }
        let tag = u16::from_be_bytes([buf[i], buf[i + 1]]);
        let len = u16::from_be_bytes([buf[i + 2], buf[i + 3]]) as usize;
        let start = i + 4;

        if start + len > buf.len() {
            return Err(TlvError::LengthPastEnd {
                name: tlv_name(tag),
                declared: len,
                left: buf.len() - start,
            });
        }

        out.push(Tlv::new(tag, buf[start..start + len].to_vec()));
        i = start + len;
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_a_tlv_as_tag_length_value_big_endian() {
        let out = encode_tlvs(&[Tlv::new(0x0424, b"hi".to_vec())]);
        assert_eq!(out, vec![0x04, 0x24, 0x00, 0x02, 0x68, 0x69]);
    }

    #[test]
    fn round_trips_multiple_tlvs() {
        let tlvs = vec![
            Tlv::new(0x0424, b"hello".to_vec()),
            Tlv::new(0x001e, b"A7F2".to_vec()),
            Tlv::new(0x020c, vec![0x2a]),
        ];
        assert_eq!(decode_tlvs(&encode_tlvs(&tlvs)).unwrap(), tlvs);
    }

    #[test]
    fn decodes_an_unknown_tag_without_losing_its_bytes() {
        let tlv = Tlv::new(0x9999, vec![0xde, 0xad, 0xbe, 0xef]);
        assert_eq!(
            decode_tlvs(&encode_tlvs(std::slice::from_ref(&tlv))).unwrap(),
            vec![tlv]
        );
    }

    #[test]
    fn decodes_a_zero_length_tlv() {
        let decoded = decode_tlvs(&[0x04, 0x26, 0x00, 0x00]).unwrap();
        assert_eq!(decoded, vec![Tlv::new(0x0426, vec![])]);
    }

    #[test]
    fn rejects_a_tlv_whose_declared_length_exceeds_the_buffer() {
        let err = decode_tlvs(&[0x04, 0x24, 0x00, 0x10, 0x68, 0x69]).unwrap_err();
        assert!(err.to_string().contains("message_payload"), "{err}");
        assert!(err.to_string().contains("16"), "{err}");
    }

    #[test]
    fn rejects_a_truncated_tlv_header() {
        assert!(decode_tlvs(&[0x04, 0x24, 0x00]).is_err());
    }

    #[test]
    fn decodes_nothing_from_no_bytes() {
        assert_eq!(decode_tlvs(&[]).unwrap(), vec![]);
    }
}
