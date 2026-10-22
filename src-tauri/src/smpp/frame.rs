//! Frames a TCP byte stream into PDUs by `command_length` — never by read
//! boundaries. A chunk may hold a fraction of a PDU, several PDUs, or a
//! boundary falling inside the 16-byte header.

use crate::smpp::constants::PDU_HEADER_LENGTH;

/// Upper bound on a single PDU. A `command_length` above this is rejected
/// before it is ever used as an allocation size.
pub const MAX_PDU_LENGTH: usize = 1_048_576;

/// What the framer is still holding, and what the PDU it belongs to claims it
/// will be. `declared` is None while the header itself is incomplete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pending {
    pub buffered: usize,
    pub declared: Option<u32>,
}

/// A framing failure, carrying both the bytes that caused it and the PDUs
/// already read out of the same chunk. Those are good traffic: losing them
/// alongside the bad one is the "never lose traffic" anti-goal failing.
#[derive(Debug, Clone)]
pub struct FrameError {
    pub message: String,
    pub raw: Vec<u8>,
    pub framed: Vec<Vec<u8>>,
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for FrameError {}

#[derive(Debug, Default)]
pub struct PduFramer {
    buf: Vec<u8>,
}

impl PduFramer {
    pub fn new() -> Self {
        Self { buf: Vec::new() }
    }

    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<Vec<u8>>, FrameError> {
        self.buf.extend_from_slice(chunk);

        let mut out: Vec<Vec<u8>> = Vec::new();
        let mut offset = 0usize;

        loop {
            let left = self.buf.len() - offset;
            if left < PDU_HEADER_LENGTH {
                break;
            }

            let command_length = u32::from_be_bytes([
                self.buf[offset],
                self.buf[offset + 1],
                self.buf[offset + 2],
                self.buf[offset + 3],
            ]);

            // Both bounds are checked before the value is used as a size.
            if (command_length as usize) < PDU_HEADER_LENGTH {
                let raw = self.buf[offset..].to_vec();
                self.buf.drain(..offset);
                return Err(FrameError {
                    message: format!(
                        "command_length {command_length} is below the {PDU_HEADER_LENGTH}-byte header"
                    ),
                    raw,
                    framed: out,
                });
            }
            if command_length as usize > MAX_PDU_LENGTH {
                let raw = self.buf[offset..].to_vec();
                self.buf.drain(..offset);
                return Err(FrameError {
                    message: format!(
                        "command_length {command_length} exceeds the {MAX_PDU_LENGTH}-byte maximum"
                    ),
                    raw,
                    framed: out,
                });
            }

            if left < command_length as usize {
                break;
            }

            out.push(self.buf[offset..offset + command_length as usize].to_vec());
            offset += command_length as usize;
        }

        self.buf.drain(..offset);
        Ok(out)
    }

    pub fn pending(&self) -> Pending {
        Pending {
            buffered: self.buf.len(),
            declared: if self.buf.len() >= PDU_HEADER_LENGTH {
                Some(u32::from_be_bytes([
                    self.buf[0],
                    self.buf[1],
                    self.buf[2],
                    self.buf[3],
                ]))
            } else {
                None
            },
        }
    }

    pub fn reset(&mut self) {
        self.buf.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::smpp::constants::command_id;
    use crate::testing::bytes::{pdu, pdu_with_length, submit_sm_body};

    fn enquire(seq: u32) -> Vec<u8> {
        pdu(command_id("enquire_link").unwrap(), 0, seq, &[])
    }

    #[test]
    fn returns_one_pdu_from_one_chunk() {
        let mut f = PduFramer::new();
        let p = enquire(7);
        assert_eq!(f.push(&p).unwrap(), vec![p]);
    }

    #[test]
    fn returns_nothing_until_a_pdu_is_complete() {
        let mut f = PduFramer::new();
        let p = pdu(
            command_id("submit_sm").unwrap(),
            0,
            1,
            &submit_sm_body("hello"),
        );
        assert!(f.push(&p[..10]).unwrap().is_empty());
    }

    #[test]
    fn reassembles_a_pdu_split_across_three_chunks() {
        let mut f = PduFramer::new();
        let p = pdu(
            command_id("submit_sm").unwrap(),
            0,
            1,
            &submit_sm_body("hello"),
        );
        assert!(f.push(&p[..5]).unwrap().is_empty());
        assert!(f.push(&p[5..20]).unwrap().is_empty());
        assert_eq!(f.push(&p[20..]).unwrap(), vec![p]);
    }

    #[test]
    fn returns_two_pdus_delivered_in_one_chunk() {
        let mut f = PduFramer::new();
        let (a, b) = (enquire(1), enquire(2));
        let mut both = a.clone();
        both.extend(b.clone());
        assert_eq!(f.push(&both).unwrap(), vec![a, b]);
    }

    #[test]
    fn keeps_the_tail_of_a_chunk_that_ends_mid_pdu() {
        let mut f = PduFramer::new();
        let a = enquire(1);
        let b = pdu(
            command_id("submit_sm").unwrap(),
            0,
            2,
            &submit_sm_body("second"),
        );
        let mut both = a.clone();
        both.extend(b.clone());
        let cut = a.len() + 9;
        assert_eq!(f.push(&both[..cut]).unwrap(), vec![a]);
        assert_eq!(f.push(&both[cut..]).unwrap(), vec![b]);
    }

    #[test]
    fn splits_a_pdu_boundary_inside_the_header() {
        let mut f = PduFramer::new();
        let (a, b) = (enquire(1), enquire(2));
        let mut both = a.clone();
        both.extend(b.clone());
        let cut = a.len() + 3;
        assert_eq!(f.push(&both[..cut]).unwrap(), vec![a]);
        assert_eq!(f.push(&both[cut..]).unwrap(), vec![b]);
    }

    #[test]
    fn rejects_a_command_length_below_the_header() {
        let mut f = PduFramer::new();
        let err = f
            .push(&pdu_with_length(
                8,
                command_id("enquire_link").unwrap(),
                0,
                1,
                &[],
            ))
            .unwrap_err();
        assert!(err.message.contains('8'), "{}", err.message);
    }

    #[test]
    fn rejects_a_command_length_above_the_maximum() {
        let mut f = PduFramer::new();
        assert!(f
            .push(&pdu_with_length(MAX_PDU_LENGTH as u32 + 1, 4, 0, 1, &[]))
            .is_err());
    }

    #[test]
    fn rejects_a_lying_command_length_without_allocating() {
        let mut f = PduFramer::new();
        let mut buf = vec![0u8; 32];
        buf[..4].copy_from_slice(&0x7fff_ffffu32.to_be_bytes());
        assert!(f.push(&buf).is_err());
    }

    #[test]
    fn keeps_the_pdus_it_already_framed_when_a_later_one_is_malformed() {
        let mut f = PduFramer::new();
        let (a, b) = (enquire(1), enquire(2));
        let bad = pdu_with_length(4, command_id("enquire_link").unwrap(), 0, 3, &[]);
        let mut chunk = a.clone();
        chunk.extend(b.clone());
        chunk.extend(bad);

        let err = f.push(&chunk).unwrap_err();
        // The two good PDUs arrived before the bad one and must not be lost.
        assert_eq!(err.framed, vec![a, b]);
    }

    #[test]
    fn reports_no_framed_pdus_when_the_very_first_one_is_malformed() {
        let mut f = PduFramer::new();
        let err = f.push(&pdu_with_length(4, 0x15, 0, 1, &[])).unwrap_err();
        assert!(err.framed.is_empty());
    }

    #[test]
    fn reports_what_it_is_holding_and_what_that_pdu_claims() {
        let mut f = PduFramer::new();
        let p = pdu(
            command_id("submit_sm").unwrap(),
            0,
            1,
            &submit_sm_body("hello"),
        );
        f.push(&p[..20]).unwrap();
        assert_eq!(
            f.pending(),
            Pending {
                buffered: 20,
                declared: Some(p.len() as u32)
            }
        );
    }

    #[test]
    fn reports_nothing_pending_when_its_buffer_is_empty() {
        let f = PduFramer::new();
        assert_eq!(
            f.pending(),
            Pending {
                buffered: 0,
                declared: None
            }
        );
    }

    #[test]
    fn reports_no_declared_length_while_the_header_is_incomplete() {
        let mut f = PduFramer::new();
        f.push(&[0, 0, 0]).unwrap();
        assert_eq!(
            f.pending(),
            Pending {
                buffered: 3,
                declared: None
            }
        );
    }

    #[test]
    fn drops_its_buffer_on_reset_and_frames_the_next_pdu_cleanly() {
        let mut f = PduFramer::new();
        let p = pdu(
            command_id("submit_sm").unwrap(),
            0,
            1,
            &submit_sm_body("hello"),
        );
        f.push(&p[..10]).unwrap();
        assert_eq!(f.pending().buffered, 10);

        f.reset();
        assert_eq!(f.pending().buffered, 0);

        let next = enquire(2);
        assert_eq!(f.push(&next).unwrap(), vec![next]);
    }
}
