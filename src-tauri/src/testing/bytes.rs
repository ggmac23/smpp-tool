//! Byte-building helpers, deliberately independent of `smpp::encode`.

pub fn cstr(s: &str) -> Vec<u8> {
    let mut v: Vec<u8> = s.chars().map(|c| c as u8).collect();
    v.push(0);
    v
}

pub fn u32be(n: u32) -> Vec<u8> {
    n.to_be_bytes().to_vec()
}

/// A complete PDU with a correct command_length.
pub fn pdu(command_id: u32, status: u32, seq: u32, body: &[u8]) -> Vec<u8> {
    let mut out = u32be(16 + body.len() as u32);
    out.extend(u32be(command_id));
    out.extend(u32be(status));
    out.extend(u32be(seq));
    out.extend_from_slice(body);
    out
}

/// A PDU whose command_length is whatever you say, correct or not.
pub fn pdu_with_length(
    length: u32,
    command_id: u32,
    status: u32,
    seq: u32,
    body: &[u8],
) -> Vec<u8> {
    let mut out = u32be(length);
    out.extend(u32be(command_id));
    out.extend(u32be(status));
    out.extend(u32be(seq));
    out.extend_from_slice(body);
    out
}

/// A minimal, valid submit_sm body carrying `text` in short_message.
pub fn submit_sm_body(text: &str) -> Vec<u8> {
    let msg: Vec<u8> = text.chars().map(|c| c as u8).collect();
    let mut out = cstr("");
    out.extend([1u8, 1]);
    out.extend(cstr("393331234567"));
    out.extend([1u8, 1]);
    out.extend(cstr("393337654321"));
    out.extend([0u8, 0, 0]);
    out.extend(cstr(""));
    out.extend(cstr(""));
    out.extend([1u8, 0, 0, 0]);
    out.push(msg.len() as u8);
    out.extend(msg);
    out
}
