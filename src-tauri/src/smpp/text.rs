//! Text codecs and the live size counter.
//!
//! Over SMPP, `data_coding 0` is conventionally sent one septet per octet,
//! because sm_length counts octets — that is what SMSCs expect, so it is the
//! default. Packed 7-bit is the GSM air-interface layout; a tester still needs
//! to be able to send it, so it is offered, never assumed.

use crate::smpp::constants::DataCoding;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SegmentationMode {
    #[default]
    None,
    Udh,
    Sar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Gsm7Packing {
    #[default]
    Unpacked,
    Packed,
}

pub const GSM_ESCAPE: u8 = 0x1b;

/// GSM 03.38 default alphabet, index = septet value.
const GSM_BASIC: [char; 128] = [
    '@', '£', '$', '¥', 'è', 'é', 'ù', 'ì', 'ò', 'Ç', '\n', 'Ø', 'ø', '\r', 'Å', 'å', 'Δ', '_',
    'Φ', 'Γ', 'Λ', 'Ω', 'Π', 'Ψ', 'Σ', 'Θ', 'Ξ', '\u{1b}', 'Æ', 'æ', 'ß', 'É', ' ', '!', '"', '#',
    '¤', '%', '&', '\'', '(', ')', '*', '+', ',', '-', '.', '/', '0', '1', '2', '3', '4', '5', '6',
    '7', '8', '9', ':', ';', '<', '=', '>', '?', '¡', 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I',
    'J', 'K', 'L', 'M', 'N', 'O', 'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z', 'Ä', 'Ö',
    'Ñ', 'Ü', '§', '¿', 'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o',
    'p', 'q', 'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z', 'ä', 'ö', 'ñ', 'ü', 'à',
];

/// GSM 03.38 extension table: ESC followed by these septet values.
const GSM_EXT: &[(char, u8)] = &[
    ('\u{c}', 0x0a),
    ('^', 0x14),
    ('{', 0x28),
    ('}', 0x29),
    ('\\', 0x2f),
    ('[', 0x3c),
    ('~', 0x3d),
    (']', 0x3e),
    ('|', 0x40),
    ('€', 0x65),
];

fn gsm_basic_septet(ch: char) -> Option<u8> {
    GSM_BASIC.iter().position(|c| *c == ch).and_then(|i| {
        // The escape marker is not a character in its own right.
        if i as u8 == GSM_ESCAPE {
            None
        } else {
            Some(i as u8)
        }
    })
}

fn gsm_ext_septet(ch: char) -> Option<u8> {
    GSM_EXT.iter().find(|(c, _)| *c == ch).map(|(_, v)| *v)
}

fn gsm_ext_char(septet: u8) -> Option<char> {
    GSM_EXT.iter().find(|(_, v)| *v == septet).map(|(c, _)| *c)
}

/// The septet sequence for `text`, counting an escape-table character as two.
pub fn septets_for(text: &str) -> Vec<u8> {
    to_septets(text, &mut |_| {})
}

fn to_septets(text: &str, on_unmappable: &mut dyn FnMut(char)) -> Vec<u8> {
    let mut out = Vec::new();
    for ch in text.chars() {
        if let Some(v) = gsm_basic_septet(ch) {
            out.push(v);
        } else if let Some(v) = gsm_ext_septet(ch) {
            out.push(GSM_ESCAPE);
            out.push(v);
        } else {
            on_unmappable(ch);
            out.push(gsm_basic_septet('?').expect("'?' is in the basic alphabet"));
        }
    }
    out
}

fn pack_septets(septets: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for s in septets {
        acc |= ((*s as u32) & 0x7f) << bits;
        bits += 7;
        while bits >= 8 {
            out.push((acc & 0xff) as u8);
            acc >>= 8;
            bits -= 8;
        }
    }
    if bits > 0 {
        out.push((acc & 0xff) as u8);
    }
    out
}

fn unpack_septets(buf: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for byte in buf {
        acc |= (*byte as u32) << bits;
        bits += 8;
        while bits >= 7 {
            out.push((acc & 0x7f) as u8);
            acc >>= 7;
            bits -= 7;
        }
    }
    // A trailing all-zero septet is padding, not an "@".
    if bits == 0 {
        if let Some(0) = out.last() {
            out.pop();
        }
    }
    out
}

fn septets_to_text(septets: &[u8]) -> String {
    let mut s = String::new();
    let mut i = 0;
    while i < septets.len() {
        if septets[i] == GSM_ESCAPE {
            if let Some(ch) = septets.get(i + 1).and_then(|v| gsm_ext_char(*v)) {
                s.push(ch);
                i += 2;
                continue;
            }
            // A lone escape carries no character.
            i += 1;
            continue;
        }
        if let Some(ch) = GSM_BASIC.get(septets[i] as usize) {
            s.push(*ch);
        }
        i += 1;
    }
    s
}

fn encoding_for(data_coding: u8) -> Option<&'static encoding_rs::Encoding> {
    match data_coding {
        DataCoding::IA5_ASCII | DataCoding::LATIN_1 => Some(encoding_rs::WINDOWS_1252),
        DataCoding::CYRILLIC => Some(encoding_rs::ISO_8859_5),
        DataCoding::LATIN_HEBREW => Some(encoding_rs::ISO_8859_8),
        DataCoding::JIS => Some(encoding_rs::SHIFT_JIS),
        _ => None,
    }
}

fn is_octet(data_coding: u8) -> bool {
    data_coding == DataCoding::OCTET_UNSPECIFIED_2 || data_coding == DataCoding::OCTET_UNSPECIFIED_4
}

#[derive(Debug, Clone, Default)]
pub struct Encoded {
    pub bytes: Vec<u8>,
    /// Each distinct character the coding cannot represent, in first-seen order.
    pub unmappable: Vec<String>,
}

pub fn encode_text(text: &str, data_coding: u8, packing: Gsm7Packing) -> Encoded {
    let mut unmappable: Vec<String> = Vec::new();
    let mut note = |ch: char| {
        let s = ch.to_string();
        if !unmappable.contains(&s) {
            unmappable.push(s);
        }
    };

    if data_coding == DataCoding::GSM_DEFAULT {
        let septets = to_septets(text, &mut note);
        let bytes = match packing {
            Gsm7Packing::Packed => pack_septets(&septets),
            Gsm7Packing::Unpacked => septets,
        };
        return Encoded { bytes, unmappable };
    }

    if data_coding == DataCoding::UCS2 {
        let mut bytes = Vec::new();
        for unit in text.encode_utf16() {
            bytes.extend(unit.to_be_bytes());
        }
        return Encoded { bytes, unmappable };
    }

    if is_octet(data_coding) {
        let mut bytes = Vec::new();
        for ch in text.chars() {
            if (ch as u32) > 0xff {
                note(ch);
            }
            bytes.push((ch as u32 & 0xff) as u8);
        }
        return Encoded { bytes, unmappable };
    }

    if let Some(encoding) = encoding_for(data_coding) {
        // Encode a character at a time so the ones that will not fit can be
        // named rather than silently replaced.
        let mut bytes = Vec::new();
        for ch in text.chars() {
            let mut buf = [0u8; 4];
            let s = ch.encode_utf8(&mut buf);
            let (out, _, had_errors) = encoding.encode(s);
            if had_errors {
                note(ch);
                bytes.push(b'?');
            } else {
                bytes.extend_from_slice(&out);
            }
        }
        return Encoded { bytes, unmappable };
    }

    // A data_coding with no table: treat the text as octets and say so.
    encode_text(text, DataCoding::OCTET_UNSPECIFIED_4, packing)
}

pub fn decode_text(bytes: &[u8], data_coding: u8, packing: Gsm7Packing) -> String {
    if data_coding == DataCoding::GSM_DEFAULT {
        let septets = match packing {
            Gsm7Packing::Packed => unpack_septets(bytes),
            Gsm7Packing::Unpacked => bytes.to_vec(),
        };
        return septets_to_text(&septets);
    }

    if data_coding == DataCoding::UCS2 {
        let units: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16_lossy(&units);
    }

    if is_octet(data_coding) {
        return bytes.iter().map(|b| *b as char).collect();
    }

    if let Some(encoding) = encoding_for(data_coding) {
        let (text, _, _) = encoding.decode(bytes);
        return text.into_owned();
    }

    bytes.iter().map(|b| *b as char).collect()
}

/* ---------------------------------------------------------------------------
 * Splitting. The live counter and the segmenter must agree about where a
 * boundary falls, so both go through these.
 * ------------------------------------------------------------------------ */

pub const GSM7_SINGLE: usize = 160;
pub const GSM7_CONCAT: usize = 153;
pub const UCS2_SINGLE_UNITS: usize = 70;
pub const UCS2_CONCAT_UNITS: usize = 67;
pub const OCTET_SINGLE: usize = 140;
pub const OCTET_CONCAT: usize = 134;

/// Split septets into runs of at most `limit`, never between an ESC and its pair.
pub fn split_septets(septets: &[u8], limit: usize) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < septets.len() {
        let mut end = (i + limit).min(septets.len());
        // An ESC as the last septet of a run would be orphaned: push it on.
        if end < septets.len() && septets[end - 1] == GSM_ESCAPE {
            end -= 1;
        }
        out.push(septets[i..end].to_vec());
        i = end;
    }
    out
}

/// Split text into chunks of at most `limit_units` UTF-16 code units, never
/// inside a surrogate pair.
/// Splits text so that each piece encodes to at most `limit` octets under
/// `data_coding`, cutting only on character boundaries.
///
/// Codings outside GSM 03.38 and UCS2 are not one byte per character —
/// Shift-JIS takes two for most of its range — so counting parts from the
/// byte total while cutting by character made the counter and the segmenter
/// disagree, and produced bodies past what a single PDU carries. Both now
/// come through here.
pub fn split_by_encoded_bytes(
    text: &str,
    data_coding: u8,
    packing: Gsm7Packing,
    limit: usize,
) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }

    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_len = 0usize;

    for c in text.chars() {
        let width = encode_text(&c.to_string(), data_coding, packing)
            .bytes
            .len();
        // A single character wider than the limit cannot be split any
        // further: it goes out on its own rather than being dropped.
        if current_len + width > limit && !current.is_empty() {
            out.push(std::mem::take(&mut current));
            current_len = 0;
        }
        current.push(c);
        current_len += width;
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

pub fn split_by_code_units(text: &str, limit_units: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut chunk = String::new();
    let mut units = 0usize;
    for ch in text.chars() {
        let cost = ch.len_utf16();
        if units + cost > limit_units {
            out.push(std::mem::take(&mut chunk));
            units = 0;
        }
        chunk.push(ch);
        units += cost;
    }
    if !chunk.is_empty() || out.is_empty() {
        out.push(chunk);
    }
    out
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Measurement {
    /// Characters the user typed, counting an astral character as one.
    pub chars: usize,
    /// Bits the encoded body occupies.
    pub bits: usize,
    /// Octets the encoded body occupies.
    pub bytes: usize,
    /// PDUs this body will take under the chosen segmentation mode.
    pub parts: usize,
    pub unmappable: Vec<String>,
}

pub fn measure(
    text: &str,
    data_coding: u8,
    mode: SegmentationMode,
    packing: Gsm7Packing,
) -> Measurement {
    let encoded = encode_text(text, data_coding, packing);
    let chars = text.chars().count();
    let concatenating = mode != SegmentationMode::None;

    let (bits, parts) = if data_coding == DataCoding::GSM_DEFAULT {
        let septets = septets_for(text);
        let parts = if !concatenating || septets.len() <= GSM7_SINGLE {
            1
        } else {
            split_septets(&septets, GSM7_CONCAT).len()
        };
        (septets.len() * 7, parts)
    } else if data_coding == DataCoding::UCS2 {
        let units = encoded.bytes.len() / 2;
        let parts = if !concatenating || units <= UCS2_SINGLE_UNITS {
            1
        } else {
            split_by_code_units(text, UCS2_CONCAT_UNITS).len()
        };
        (encoded.bytes.len() * 8, parts)
    } else {
        let parts = if !concatenating || encoded.bytes.len() <= OCTET_SINGLE {
            1
        } else {
            // The same splitter the segmenter uses, so the two cannot differ.
            split_by_encoded_bytes(text, data_coding, packing, OCTET_CONCAT).len()
        };
        (encoded.bytes.len() * 8, parts)
    };

    Measurement {
        chars,
        bits,
        bytes: encoded.bytes.len(),
        parts,
        unmappable: encoded.unmappable,
    }
}

/// The reference tool's sample messages. `gsm1` exercises the escape table on
/// purpose, so the counter is tested by the preset itself.
pub struct Samples {
    pub gsm1: &'static str,
    pub ucs2_1: &'static str,
    pub gsm3: &'static str,
    pub ucs2_3: &'static str,
}

pub const SAMPLES: Samples = Samples {
    gsm1: "GSM 03.38 sample: price €10 {braces} [brackets] ^caret~ |pipe| @ £ $ ¥ § ¿ ¡ ÄÖÑÜäöñüà",
    ucs2_1: "UCS2 sample: Привет мир — שלום עולם — こんにちは — ciao 🙂",
    gsm3: "This is a three-part GSM 03.38 message used to exercise concatenation. It runs past one hundred and sixty septets so the client has to split it, and it keeps going past three hundred and six so that a third segment is needed as well. It stays inside the default alphabet throughout, apart from the euro sign € which costs two septets and therefore proves that an escape pair is never cut in half at a segment boundary. Keep typing.",
    ucs2_3: "UCS2 three-part sample. Это сообщение намеренно длинное, чтобы клиент разбил его на три части. これは三つに分割される長いメッセージです 🙂 и ещё немного текста, чтобы третья часть точно понадобилась.",
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::smpp::constants::DataCoding;

    fn a(n: usize) -> String {
        "a".repeat(n)
    }

    #[test]
    fn packs_gsm7_septets() {
        let r = encode_text(&a(160), DataCoding::GSM_DEFAULT, Gsm7Packing::Packed);
        assert_eq!(r.bytes.len(), 140);
    }

    #[test]
    fn sends_one_septet_per_octet_when_unpacked() {
        let r = encode_text(&a(160), DataCoding::GSM_DEFAULT, Gsm7Packing::Unpacked);
        assert_eq!(r.bytes.len(), 160);
    }

    #[test]
    fn defaults_to_unpacked_which_is_what_smscs_expect() {
        assert_eq!(
            encode_text(&a(160), DataCoding::GSM_DEFAULT, Gsm7Packing::default())
                .bytes
                .len(),
            160
        );
    }

    #[test]
    fn round_trips_packed_gsm7() {
        let text = "Hello @£$¥ world";
        let r = encode_text(text, DataCoding::GSM_DEFAULT, Gsm7Packing::Packed);
        assert_eq!(
            decode_text(&r.bytes, DataCoding::GSM_DEFAULT, Gsm7Packing::Packed),
            text
        );
    }

    #[test]
    fn round_trips_unpacked_gsm7() {
        let text = "Hello Ä Ö Ñ Ü à";
        let r = encode_text(text, DataCoding::GSM_DEFAULT, Gsm7Packing::Unpacked);
        assert_eq!(
            decode_text(&r.bytes, DataCoding::GSM_DEFAULT, Gsm7Packing::Unpacked),
            text
        );
    }

    #[test]
    fn counts_an_escape_table_character_as_two_septets() {
        assert_eq!(septets_for("€").len(), 2);
        let m = measure(
            "€",
            DataCoding::GSM_DEFAULT,
            SegmentationMode::Udh,
            Gsm7Packing::Unpacked,
        );
        assert_eq!(m.chars, 1);
        assert_eq!(m.bits, 14);
    }

    #[test]
    fn encodes_every_escape_table_character() {
        for ch in "^{}\\[~]|€".chars() {
            let r = encode_text(
                &ch.to_string(),
                DataCoding::GSM_DEFAULT,
                Gsm7Packing::Unpacked,
            );
            assert!(r.unmappable.is_empty(), "{ch} reported unmappable");
            assert_eq!(r.bytes.len(), 2, "{ch} did not cost two septets");
            assert_eq!(r.bytes[0], 0x1b, "{ch} did not start with ESC");
        }
    }

    #[test]
    fn encodes_ucs2_big_endian() {
        let r = encode_text("é", DataCoding::UCS2, Gsm7Packing::Unpacked);
        assert_eq!(r.bytes, vec![0x00, 0xe9]);
    }

    #[test]
    fn round_trips_ucs2_including_an_emoji() {
        let text = "ciao 🙂";
        let r = encode_text(text, DataCoding::UCS2, Gsm7Packing::Unpacked);
        assert_eq!(
            decode_text(&r.bytes, DataCoding::UCS2, Gsm7Packing::Unpacked),
            text
        );
    }

    #[test]
    fn passes_octet_data_through_unchanged() {
        let raw = "\u{0}\u{1}\u{fe}\u{ff}";
        let r = encode_text(raw, DataCoding::OCTET_UNSPECIFIED_4, Gsm7Packing::Unpacked);
        assert_eq!(r.bytes, vec![0x00, 0x01, 0xfe, 0xff]);
    }

    #[test]
    fn encodes_latin_1() {
        let r = encode_text("é", DataCoding::LATIN_1, Gsm7Packing::Unpacked);
        assert_eq!(r.bytes, vec![0xe9]);
    }

    #[test]
    fn encodes_cyrillic() {
        let r = encode_text("Привет", DataCoding::CYRILLIC, Gsm7Packing::Unpacked);
        assert!(r.unmappable.is_empty());
        assert_eq!(
            decode_text(&r.bytes, DataCoding::CYRILLIC, Gsm7Packing::Unpacked),
            "Привет"
        );
    }

    #[test]
    fn encodes_latin_hebrew() {
        let r = encode_text("שלום", DataCoding::LATIN_HEBREW, Gsm7Packing::Unpacked);
        assert!(r.unmappable.is_empty());
        assert_eq!(
            decode_text(&r.bytes, DataCoding::LATIN_HEBREW, Gsm7Packing::Unpacked),
            "שלום"
        );
    }

    #[test]
    fn encodes_jis() {
        let r = encode_text("こんにちは", DataCoding::JIS, Gsm7Packing::Unpacked);
        assert!(r.unmappable.is_empty());
        assert_eq!(
            decode_text(&r.bytes, DataCoding::JIS, Gsm7Packing::Unpacked),
            "こんにちは"
        );
    }

    fn parts(text: &str, dc: u8) -> usize {
        measure(text, dc, SegmentationMode::Udh, Gsm7Packing::Unpacked).parts
    }

    #[test]
    fn reports_parts_one_for_160_gsm7_chars_and_two_for_161() {
        assert_eq!(parts(&a(160), DataCoding::GSM_DEFAULT), 1);
        assert_eq!(parts(&a(161), DataCoding::GSM_DEFAULT), 2);
    }

    #[test]
    fn reports_parts_one_for_70_ucs2_chars_and_two_for_71() {
        assert_eq!(parts(&a(70), DataCoding::UCS2), 1);
        assert_eq!(parts(&a(71), DataCoding::UCS2), 2);
    }

    #[test]
    fn reports_one_part_in_none_mode_however_long_the_text_is() {
        let m = measure(
            &a(1000),
            DataCoding::GSM_DEFAULT,
            SegmentationMode::None,
            Gsm7Packing::Unpacked,
        );
        assert_eq!(m.parts, 1);
    }

    #[test]
    fn reports_chars_bits_and_bytes_for_unpacked_gsm7() {
        let m = measure(
            "hello",
            DataCoding::GSM_DEFAULT,
            SegmentationMode::Udh,
            Gsm7Packing::Unpacked,
        );
        assert_eq!((m.chars, m.bits, m.bytes), (5, 35, 5));
    }

    #[test]
    fn reports_the_packed_byte_count_when_packing_is_packed() {
        let m = measure(
            &a(160),
            DataCoding::GSM_DEFAULT,
            SegmentationMode::Udh,
            Gsm7Packing::Packed,
        );
        assert_eq!(m.bytes, 140);
    }

    #[test]
    fn never_splits_an_escape_pair_across_parts() {
        let text = format!("{}€{}", a(152), "b".repeat(100));
        let runs = split_septets(&septets_for(&text), GSM7_CONCAT);
        assert_eq!(runs[0].len(), 152);
        assert_eq!(runs[1][0], GSM_ESCAPE);
    }

    #[test]
    fn counts_an_emoji_as_one_character_but_two_ucs2_code_units() {
        let m = measure(
            "🙂",
            DataCoding::UCS2,
            SegmentationMode::Udh,
            Gsm7Packing::Unpacked,
        );
        assert_eq!(m.chars, 1);
        assert_eq!(m.bytes, 4);
    }

    #[test]
    fn reports_emoji_as_unmappable_under_gsm7_and_does_not_drop_them() {
        let r = encode_text("hi 🙂", DataCoding::GSM_DEFAULT, Gsm7Packing::Unpacked);
        assert_eq!(r.unmappable, vec!["🙂".to_string()]);
        assert!(!r.bytes.is_empty());
    }

    #[test]
    fn substitutes_a_question_mark_for_an_unmappable_character() {
        let r = encode_text("hi 🙂", DataCoding::GSM_DEFAULT, Gsm7Packing::Unpacked);
        assert_eq!(
            decode_text(&r.bytes, DataCoding::GSM_DEFAULT, Gsm7Packing::Unpacked),
            "hi ?"
        );
    }

    #[test]
    fn reports_each_distinct_unmappable_character_once() {
        let r = encode_text("🙂🙂🙃", DataCoding::GSM_DEFAULT, Gsm7Packing::Unpacked);
        assert_eq!(r.unmappable, vec!["🙂".to_string(), "🙃".to_string()]);
    }

    #[test]
    fn reports_unmappable_characters_from_measure_too() {
        let m = measure(
            "hi 🙂",
            DataCoding::GSM_DEFAULT,
            SegmentationMode::Udh,
            Gsm7Packing::Unpacked,
        );
        assert_eq!(m.unmappable, vec!["🙂".to_string()]);
    }

    #[test]
    fn reports_a_cyrillic_character_as_unmappable_under_latin_1() {
        let r = encode_text("Привет", DataCoding::LATIN_1, Gsm7Packing::Unpacked);
        assert!(!r.unmappable.is_empty());
    }

    #[test]
    fn offers_the_four_presets_and_they_measure_as_their_names_claim() {
        assert_eq!(parts(SAMPLES.gsm1, DataCoding::GSM_DEFAULT), 1);
        assert_eq!(parts(SAMPLES.gsm3, DataCoding::GSM_DEFAULT), 3);
        assert_eq!(parts(SAMPLES.ucs2_1, DataCoding::UCS2), 1);
        assert_eq!(parts(SAMPLES.ucs2_3, DataCoding::UCS2), 3);
    }

    #[test]
    fn exercises_the_gsm_escape_table_in_the_gsm1_preset() {
        assert!(septets_for(SAMPLES.gsm1).len() > SAMPLES.gsm1.chars().count());
    }

    #[test]
    fn maps_every_preset_character_in_its_own_coding() {
        for (text, dc) in [
            (SAMPLES.gsm1, DataCoding::GSM_DEFAULT),
            (SAMPLES.gsm3, DataCoding::GSM_DEFAULT),
            (SAMPLES.ucs2_1, DataCoding::UCS2),
            (SAMPLES.ucs2_3, DataCoding::UCS2),
        ] {
            let r = encode_text(text, dc, Gsm7Packing::Unpacked);
            assert!(
                r.unmappable.is_empty(),
                "unmappable in a preset: {:?}",
                r.unmappable
            );
        }
    }
}
