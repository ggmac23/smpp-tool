//! Decoding a PDU into its fields.
//!
//! A `command_length` that disagrees with the buffer is an error, never
//! something to repair: the hex composer wants that error shown, not fixed.
//! An id the protocol does not define keeps its body whole and parses no TLVs.

use indexmap::IndexMap;

use crate::smpp::constants::{command_name, PDU_HEADER_LENGTH};
use crate::smpp::defs::{fields_for, FieldDef, FieldType};
use crate::smpp::tlv::{decode_tlvs, Tlv};

/// One entry of submit_multi's dest_address list.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DestEntry {
    pub flag: u8,
    pub ton: Option<u8>,
    pub npi: Option<u8>,
    pub addr: Option<String>,
    pub dl_name: Option<String>,
}

/// One entry of submit_multi_resp's unsuccess_sme list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsuccessEntry {
    pub ton: u8,
    pub npi: u8,
    pub addr: String,
    pub error_status: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldValue {
    Num(u32),
    Str(String),
    Bytes(Vec<u8>),
    DestList(Vec<DestEntry>),
    UnsuccessList(Vec<UnsuccessEntry>),
}

#[derive(Debug, Clone)]
pub struct Pdu {
    pub command_id: u32,
    pub command_status: u32,
    pub sequence_number: u32,
    pub fields: IndexMap<String, FieldValue>,
    pub tlvs: Vec<Tlv>,
    /// The exact bytes this PDU was decoded from.
    pub raw: Vec<u8>,
}

impl Pdu {
    pub fn u32_field(&self, name: &str) -> Option<u32> {
        match self.fields.get(name) {
            Some(FieldValue::Num(n)) => Some(*n),
            _ => None,
        }
    }

    pub fn str_field(&self, name: &str) -> Option<String> {
        match self.fields.get(name) {
            Some(FieldValue::Str(s)) => Some(s.clone()),
            _ => None,
        }
    }

    pub fn bytes_field(&self, name: &str) -> Option<Vec<u8>> {
        match self.fields.get(name) {
            Some(FieldValue::Bytes(b)) => Some(b.clone()),
            _ => None,
        }
    }

    pub fn name(&self) -> String {
        command_name(self.command_id)
    }
}

#[derive(Debug, Clone)]
pub struct PduDecodeError {
    pub message: String,
    pub raw: Vec<u8>,
}

impl std::fmt::Display for PduDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for PduDecodeError {}

struct Reader<'a> {
    body: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn remaining(&self) -> usize {
        self.body.len() - self.offset
    }

    fn u8(&mut self, name: &str) -> Result<u32, String> {
        if self.remaining() < 1 {
            return Err(format!("{name}: ran out of bytes reading a u8"));
        }
        let v = self.body[self.offset];
        self.offset += 1;
        Ok(v as u32)
    }

    fn u16(&mut self, name: &str) -> Result<u32, String> {
        if self.remaining() < 2 {
            return Err(format!("{name}: ran out of bytes reading a u16"));
        }
        let v = u16::from_be_bytes([self.body[self.offset], self.body[self.offset + 1]]);
        self.offset += 2;
        Ok(v as u32)
    }

    fn u32(&mut self, name: &str) -> Result<u32, String> {
        if self.remaining() < 4 {
            return Err(format!("{name}: ran out of bytes reading a u32"));
        }
        let v = u32::from_be_bytes([
            self.body[self.offset],
            self.body[self.offset + 1],
            self.body[self.offset + 2],
            self.body[self.offset + 3],
        ]);
        self.offset += 4;
        Ok(v)
    }

    fn cstring(&mut self, name: &str) -> Result<String, String> {
        match self.body[self.offset..].iter().position(|b| *b == 0) {
            None => Err(format!(
                "{name}: unterminated C-string at offset {}",
                self.offset
            )),
            Some(end) => {
                let s: String = self.body[self.offset..self.offset + end]
                    .iter()
                    .map(|b| *b as char)
                    .collect();
                self.offset += end + 1;
                Ok(s)
            }
        }
    }

    fn octets(&mut self, name: &str, length: usize) -> Result<Vec<u8>, String> {
        if length > self.remaining() {
            return Err(format!(
                "{name}: declares {length} byte(s) but only {} remain",
                self.remaining()
            ));
        }
        let v = self.body[self.offset..self.offset + length].to_vec();
        self.offset += length;
        Ok(v)
    }

    fn tail(&self) -> &'a [u8] {
        &self.body[self.offset..]
    }
}

fn count_from(fields: &IndexMap<String, FieldValue>, def: &FieldDef) -> usize {
    def.len_from
        .and_then(|n| match fields.get(n) {
            Some(FieldValue::Num(v)) => Some(*v as usize),
            _ => None,
        })
        .unwrap_or(0)
}

fn read_dest_list(r: &mut Reader, count: usize) -> Result<Vec<DestEntry>, String> {
    let mut out = Vec::with_capacity(count.min(1024));
    for i in 0..count {
        let flag = r.u8(&format!("dest_address[{i}].dest_flag"))? as u8;
        if flag == 2 {
            out.push(DestEntry {
                flag,
                dl_name: Some(r.cstring(&format!("dest_address[{i}].dl_name"))?),
                ..Default::default()
            });
        } else {
            out.push(DestEntry {
                flag,
                ton: Some(r.u8(&format!("dest_address[{i}].dest_addr_ton"))? as u8),
                npi: Some(r.u8(&format!("dest_address[{i}].dest_addr_npi"))? as u8),
                addr: Some(r.cstring(&format!("dest_address[{i}].destination_addr"))?),
                dl_name: None,
            });
        }
    }
    Ok(out)
}

fn read_unsuccess_list(r: &mut Reader, count: usize) -> Result<Vec<UnsuccessEntry>, String> {
    let mut out = Vec::with_capacity(count.min(1024));
    for i in 0..count {
        out.push(UnsuccessEntry {
            ton: r.u8(&format!("unsuccess_sme[{i}].dest_addr_ton"))? as u8,
            npi: r.u8(&format!("unsuccess_sme[{i}].dest_addr_npi"))? as u8,
            addr: r.cstring(&format!("unsuccess_sme[{i}].destination_addr"))?,
            error_status: r.u32(&format!("unsuccess_sme[{i}].error_status"))?,
        });
    }
    Ok(out)
}

/// Decode one complete PDU. `buf` must be exactly the PDU, which is what the
/// framer hands over.
pub fn decode_pdu(buf: &[u8]) -> Result<Pdu, PduDecodeError> {
    let fail = |message: String| PduDecodeError {
        message,
        raw: buf.to_vec(),
    };

    if buf.len() < PDU_HEADER_LENGTH {
        return Err(fail(format!(
            "PDU shorter than the {PDU_HEADER_LENGTH}-byte header: {} byte(s)",
            buf.len()
        )));
    }

    let command_length = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);
    if command_length as usize != buf.len() {
        return Err(fail(format!(
            "command_length {command_length} disagrees with the {} byte(s) present",
            buf.len()
        )));
    }

    let command_id = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]);
    let command_status = u32::from_be_bytes([buf[8], buf[9], buf[10], buf[11]]);
    let sequence_number = u32::from_be_bytes([buf[12], buf[13], buf[14], buf[15]]);
    let body = &buf[PDU_HEADER_LENGTH..];
    let name = command_name(command_id);

    let Some(defs) = fields_for(&name) else {
        // An id the protocol does not define: keep the body intact, parse no TLVs.
        let mut fields = IndexMap::new();
        fields.insert("body".to_string(), FieldValue::Bytes(body.to_vec()));
        return Ok(Pdu {
            command_id,
            command_status,
            sequence_number,
            fields,
            tlvs: Vec::new(),
            raw: buf.to_vec(),
        });
    };

    let mut r = Reader { body, offset: 0 };
    let mut fields: IndexMap<String, FieldValue> = IndexMap::new();

    for def in defs {
        let value = match def.ty {
            FieldType::U8 => FieldValue::Num(r.u8(def.name).map_err(fail)?),
            FieldType::U16 => FieldValue::Num(r.u16(def.name).map_err(fail)?),
            FieldType::U32 => FieldValue::Num(r.u32(def.name).map_err(fail)?),
            FieldType::CString => FieldValue::Str(r.cstring(def.name).map_err(fail)?),
            FieldType::Octets => {
                let n = match def.len_from {
                    Some(_) => count_from(&fields, def),
                    None => r.remaining(),
                };
                FieldValue::Bytes(r.octets(def.name, n).map_err(fail)?)
            }
            FieldType::DestList => {
                let n = count_from(&fields, def);
                FieldValue::DestList(read_dest_list(&mut r, n).map_err(fail)?)
            }
            FieldType::UnsuccessList => {
                let n = count_from(&fields, def);
                FieldValue::UnsuccessList(read_unsuccess_list(&mut r, n).map_err(fail)?)
            }
        };
        fields.insert(def.name.to_string(), value);
    }

    let tail = r.tail();
    let tlvs = if tail.is_empty() {
        Vec::new()
    } else {
        decode_tlvs(tail).map_err(|e| fail(format!("{name}: {e}")))?
    };

    Ok(Pdu {
        command_id,
        command_status,
        sequence_number,
        fields,
        tlvs,
        raw: buf.to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::smpp::constants::command_id;
    use crate::smpp::tlv::{encode_tlvs, Tlv};
    use crate::testing::bytes::{cstr, pdu, submit_sm_body};

    fn id(name: &str) -> u32 {
        command_id(name).unwrap()
    }

    #[test]
    fn decodes_the_sixteen_byte_header() {
        let p = decode_pdu(&pdu(id("enquire_link"), 0, 42, &[])).unwrap();
        assert_eq!(p.command_id, id("enquire_link"));
        assert_eq!(p.command_status, 0);
        assert_eq!(p.sequence_number, 42);
    }

    #[test]
    fn decodes_submit_sm_mandatory_fields_in_wire_order() {
        let p = decode_pdu(&pdu(id("submit_sm"), 0, 1, &submit_sm_body("hello"))).unwrap();
        assert_eq!(p.str_field("service_type"), Some("".to_string()));
        assert_eq!(p.u32_field("source_addr_ton"), Some(1));
        assert_eq!(p.str_field("source_addr"), Some("393331234567".to_string()));
        assert_eq!(
            p.str_field("destination_addr"),
            Some("393337654321".to_string())
        );
        assert_eq!(p.u32_field("registered_delivery"), Some(1));
        assert_eq!(p.u32_field("sm_length"), Some(5));
    }

    #[test]
    fn reads_short_message_using_sm_length() {
        let p = decode_pdu(&pdu(id("submit_sm"), 0, 1, &submit_sm_body("hello"))).unwrap();
        assert_eq!(p.bytes_field("short_message"), Some(b"hello".to_vec()));
    }

    #[test]
    fn decodes_trailing_tlvs_after_the_mandatory_fields() {
        let mut body = submit_sm_body("hi");
        body.extend(encode_tlvs(&[Tlv::new(0x0424, b"payload".to_vec())]));
        let p = decode_pdu(&pdu(id("submit_sm"), 0, 1, &body)).unwrap();
        assert_eq!(p.tlvs, vec![Tlv::new(0x0424, b"payload".to_vec())]);
    }

    #[test]
    fn decodes_submit_sm_resp() {
        let p = decode_pdu(&pdu(id("submit_sm_resp"), 0, 1, &cstr("A7F2"))).unwrap();
        assert_eq!(p.str_field("message_id"), Some("A7F2".to_string()));
    }

    #[test]
    fn keeps_the_body_whole_for_an_unknown_command_id() {
        let p = decode_pdu(&pdu(0x4242, 0, 1, &[1, 2, 3, 4])).unwrap();
        assert_eq!(p.bytes_field("body"), Some(vec![1, 2, 3, 4]));
        assert!(p.tlvs.is_empty());
    }

    #[test]
    fn preserves_the_exact_input_bytes_in_raw() {
        let bytes = pdu(id("submit_sm"), 0, 1, &submit_sm_body("hello"));
        assert_eq!(decode_pdu(&bytes).unwrap().raw, bytes);
    }

    #[test]
    fn rejects_an_unterminated_c_string_and_keeps_the_bytes() {
        let bytes = pdu(id("submit_sm"), 0, 1, b"abc");
        let err = decode_pdu(&bytes).unwrap_err();
        assert!(err.message.contains("C-string"), "{}", err.message);
        assert_eq!(err.raw, bytes);
    }

    #[test]
    fn rejects_an_sm_length_that_runs_past_the_body() {
        let mut body = cstr("");
        body.extend([1u8, 1]);
        body.extend(cstr("111"));
        body.extend([1u8, 1]);
        body.extend(cstr("222"));
        body.extend([0u8, 0, 0]);
        body.extend(cstr(""));
        body.extend(cstr(""));
        body.extend([0u8, 0, 0, 0]);
        body.push(200); // sm_length lies
        body.extend(b"hi");
        assert!(decode_pdu(&pdu(id("submit_sm"), 0, 1, &body)).is_err());
    }

    #[test]
    fn rejects_a_command_length_that_disagrees_with_the_buffer() {
        let bytes = pdu(id("enquire_link"), 0, 1, &[]);
        assert!(decode_pdu(&bytes[..15]).is_err());
    }

    #[test]
    fn decodes_a_submit_multi_destination_list() {
        let mut body = cstr("");
        body.extend([1u8, 1]);
        body.extend(cstr("111"));
        body.push(2); // number_of_dests
        body.push(1);
        body.extend([1u8, 1]);
        body.extend(cstr("393331111111"));
        body.push(2);
        body.extend(cstr("my-list"));
        // submit tail
        body.extend([0u8, 0, 0]);
        body.extend(cstr(""));
        body.extend(cstr(""));
        body.extend([0u8, 0, 0, 0]);
        body.push(2);
        body.extend(b"hi");

        let p = decode_pdu(&pdu(id("submit_multi"), 0, 1, &body)).unwrap();
        assert_eq!(p.u32_field("number_of_dests"), Some(2));
        match p.fields.get("dest_address") {
            Some(FieldValue::DestList(list)) => {
                assert_eq!(list.len(), 2);
                assert_eq!(list[0].addr.as_deref(), Some("393331111111"));
                assert_eq!(list[1].dl_name.as_deref(), Some("my-list"));
            }
            other => panic!("expected a destination list, got {other:?}"),
        }
    }
}
