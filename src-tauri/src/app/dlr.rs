//! Matching delivery receipts to the submits that caused them.
//!
//! Aggregators get this wrong constantly: the receipt id comes back in a
//! different radix, or with padding, or only inside the receipt text. Every
//! step is switchable so a profile can be pinned to what one SMSC actually
//! does. Nothing is dropped and nothing is guessed — a receipt that matches no
//! registration goes to `unmatched`, where the UI shows it.

use std::collections::HashMap;

use crate::smpp::receipt::Receipt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CorrelationConfig {
    /// Prefer the receipted_message_id TLV over the receipt text.
    pub prefer_tlv: bool,
    /// Fall back to parsing `id:` out of the receipt text.
    pub parse_text: bool,
    /// Try the hex reading of a decimal id and vice versa.
    pub normalise_radix: bool,
    /// Try the id without leading zeros.
    pub strip_leading_zeros: bool,
}

impl Default for CorrelationConfig {
    fn default() -> Self {
        Self {
            prefer_tlv: true,
            parse_text: true,
            normalise_radix: true,
            strip_leading_zeros: true,
        }
    }
}

fn is_hex(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_hexdigit())
}

fn is_dec(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

/// Every form of `id` worth trying. The id as given always comes first, so a
/// verbatim match is found without any guessing.
pub fn normalise_id(id: &str, cfg: &CorrelationConfig) -> Vec<String> {
    let mut out: Vec<String> = vec![id.to_string()];
    let add = |s: String, out: &mut Vec<String>| {
        if !s.is_empty() && !out.contains(&s) {
            out.push(s);
        }
    };

    let mut seeds = vec![id.to_string()];
    if cfg.strip_leading_zeros {
        let stripped = id.trim_start_matches('0');
        let stripped = if stripped.is_empty() { "0" } else { stripped };
        add(stripped.to_string(), &mut out);
        seeds.push(stripped.to_string());
    }

    if cfg.normalise_radix {
        for seed in &seeds {
            if is_hex(seed) {
                add(seed.to_lowercase(), &mut out);
                add(seed.to_uppercase(), &mut out);
                if let Ok(n) = u128::from_str_radix(seed, 16) {
                    add(n.to_string(), &mut out);
                }
            }
            if is_dec(seed) {
                if let Ok(n) = seed.parse::<u128>() {
                    add(format!("{n:x}"), &mut out);
                    add(format!("{n:X}"), &mut out);
                }
            }
        }
    }

    out
}

#[derive(Debug)]
struct Registration {
    token: String,
    matched: bool,
}

#[derive(Debug)]
pub struct DlrCorrelator {
    cfg: CorrelationConfig,
    /// Candidate key to the registrations that answer to it, in order.
    index: HashMap<String, Vec<usize>>,
    registrations: Vec<Registration>,
    unmatched: Vec<Receipt>,
}

impl DlrCorrelator {
    pub fn new(cfg: CorrelationConfig) -> Self {
        Self {
            cfg,
            index: HashMap::new(),
            registrations: Vec::new(),
            unmatched: Vec::new(),
        }
    }

    pub fn register(&mut self, message_id: &str, token: &str) {
        let at = self.registrations.len();
        self.registrations.push(Registration {
            token: token.to_string(),
            matched: false,
        });
        for key in normalise_id(message_id, &self.cfg) {
            self.index.entry(key).or_default().push(at);
        }
    }

    pub fn match_receipt(&mut self, receipt: &Receipt) -> Option<String> {
        let Some(id) = receipt.message_id.as_ref().filter(|s| !s.is_empty()) else {
            self.unmatched.push(receipt.clone());
            return None;
        };

        for candidate in normalise_id(id, &self.cfg) {
            let Some(bucket) = self.index.get(&candidate) else {
                continue;
            };
            // Earliest unmatched registration wins, so a collision cannot steal
            // another row's receipt.
            if let Some(at) = bucket
                .iter()
                .copied()
                .find(|i| !self.registrations[*i].matched)
            {
                self.registrations[at].matched = true;
                return Some(self.registrations[at].token.clone());
            }
        }

        self.unmatched.push(receipt.clone());
        None
    }

    /// Receipts that matched no submit. Visible, never discarded.
    pub fn unmatched(&self) -> &[Receipt] {
        &self.unmatched
    }

    /// Registrations still waiting for a receipt.
    pub fn pending(&self) -> usize {
        self.registrations.iter().filter(|r| !r.matched).count()
    }

    pub fn reset(&mut self) {
        self.index.clear();
        self.registrations.clear();
        self.unmatched.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::smpp::receipt::{Receipt, ReceiptSource};

    const ALL: CorrelationConfig = CorrelationConfig {
        prefer_tlv: true,
        parse_text: true,
        normalise_radix: true,
        strip_leading_zeros: true,
    };

    const OFF: CorrelationConfig = CorrelationConfig {
        prefer_tlv: true,
        parse_text: true,
        normalise_radix: false,
        strip_leading_zeros: false,
    };

    fn receipt(message_id: Option<&str>) -> Receipt {
        Receipt {
            message_id: message_id.map(|s| s.to_string()),
            source: ReceiptSource::Text,
            state: Some("DELIVRD".into()),
            error_code: Some("000".into()),
            submit_date: None,
            done_date: None,
        }
    }

    #[test]
    fn includes_the_id_as_given() {
        assert!(normalise_id("ABC", &ALL).contains(&"ABC".to_string()));
    }

    #[test]
    fn includes_the_decimal_reading_of_a_hex_id() {
        assert!(normalise_id("123456", &ALL).contains(&"1193046".to_string()));
    }

    #[test]
    fn includes_the_hex_reading_of_a_decimal_id() {
        assert!(normalise_id("1193046", &ALL).contains(&"123456".to_string()));
    }

    #[test]
    fn includes_the_form_without_leading_zeros() {
        assert!(normalise_id("00ABC", &ALL).contains(&"ABC".to_string()));
    }

    #[test]
    fn returns_only_the_id_itself_when_both_switches_are_off() {
        assert_eq!(normalise_id("00123456", &OFF), vec!["00123456".to_string()]);
    }

    #[test]
    fn matches_an_id_returned_verbatim() {
        let mut c = DlrCorrelator::new(ALL);
        c.register("A7F2", "row-1");
        assert_eq!(
            c.match_receipt(&receipt(Some("A7F2"))),
            Some("row-1".into())
        );
    }

    #[test]
    fn matches_a_decimal_receipt_id_against_a_hex_submit_id() {
        let mut c = DlrCorrelator::new(ALL);
        c.register("123456", "row-1");
        assert_eq!(
            c.match_receipt(&receipt(Some("1193046"))),
            Some("row-1".into())
        );
    }

    #[test]
    fn matches_a_hex_receipt_id_against_a_decimal_submit_id() {
        let mut c = DlrCorrelator::new(ALL);
        c.register("1193046", "row-1");
        assert_eq!(
            c.match_receipt(&receipt(Some("123456"))),
            Some("row-1".into())
        );
    }

    #[test]
    fn matches_an_id_that_gained_leading_zeros() {
        let mut c = DlrCorrelator::new(ALL);
        c.register("ABC", "row-1");
        assert_eq!(
            c.match_receipt(&receipt(Some("00ABC"))),
            Some("row-1".into())
        );
    }

    #[test]
    fn does_not_match_when_normalisation_is_switched_off() {
        let mut c = DlrCorrelator::new(OFF);
        c.register("123456", "row-1");
        assert_eq!(c.match_receipt(&receipt(Some("1193046"))), None);
    }

    #[test]
    fn puts_an_unmatchable_receipt_in_unmatched() {
        let mut c = DlrCorrelator::new(ALL);
        assert_eq!(c.match_receipt(&receipt(Some("NOPE"))), None);
        assert_eq!(c.unmatched().len(), 1);
    }

    #[test]
    fn puts_a_receipt_with_no_message_id_in_unmatched() {
        let mut c = DlrCorrelator::new(ALL);
        assert_eq!(c.match_receipt(&receipt(None)), None);
        assert_eq!(c.unmatched().len(), 1);
    }

    #[test]
    fn never_returns_a_token_it_was_not_given() {
        let mut c = DlrCorrelator::new(ALL);
        c.register("A", "row-1");
        assert_eq!(c.match_receipt(&receipt(Some("B"))), None);
    }

    #[test]
    fn does_not_let_a_second_receipt_steal_the_first_rows_match() {
        let mut c = DlrCorrelator::new(ALL);
        c.register("A7F2", "row-1");
        assert_eq!(
            c.match_receipt(&receipt(Some("A7F2"))),
            Some("row-1".into())
        );
        assert_eq!(c.match_receipt(&receipt(Some("A7F2"))), None);
        assert_eq!(c.unmatched().len(), 1);
    }

    #[test]
    fn gives_colliding_registrations_to_the_earliest_unmatched_row() {
        let mut c = DlrCorrelator::new(ALL);
        c.register("A7F2", "row-1");
        c.register("A7F2", "row-2");
        assert_eq!(
            c.match_receipt(&receipt(Some("A7F2"))),
            Some("row-1".into())
        );
        assert_eq!(
            c.match_receipt(&receipt(Some("A7F2"))),
            Some("row-2".into())
        );
    }

    #[test]
    fn keeps_every_candidate_form_pointing_at_the_same_row() {
        let mut c = DlrCorrelator::new(ALL);
        c.register("123456", "row-1");
        assert_eq!(
            c.match_receipt(&receipt(Some("00123456"))),
            Some("row-1".into())
        );
    }

    #[test]
    fn reports_how_many_registrations_are_still_waiting() {
        let mut c = DlrCorrelator::new(ALL);
        c.register("A", "row-1");
        c.register("B", "row-2");
        assert_eq!(c.pending(), 2);
        c.match_receipt(&receipt(Some("A")));
        assert_eq!(c.pending(), 1);
    }
}
