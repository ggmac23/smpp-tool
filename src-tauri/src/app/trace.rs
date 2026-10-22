//! The PDU trace: a bounded ring of entries plus unbounded per-command
//! counters.
//!
//! When the cap is reached the oldest entries go and `dropped` counts them, so
//! the UI can say so rather than quietly showing a partial history. Counters
//! are never reduced by eviction — they answer "how much traffic was there",
//! which is not the same question as "what is still on screen".

use std::collections::BTreeMap;
use std::collections::VecDeque;

pub const DEFAULT_TRACE_CAP: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    In,
    Out,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceEntry {
    /// Monotonic row ordinal, stable across eviction.
    pub seq: u64,
    pub at: u64,
    pub direction: Direction,
    pub command_name: String,
    pub command_status: u32,
    /// The PDU's own sequence number, not the row ordinal.
    pub sequence_number: u32,
    /// The decoded one-liner. Empty when the PDU would not parse.
    pub summary: String,
    #[serde(skip)]
    pub raw: Vec<u8>,
    /// Set instead of a summary when the PDU would not decode.
    pub decode_error: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct Counter {
    pub sent: u64,
    pub received: u64,
}

#[derive(Debug)]
pub struct TraceStore {
    items: VecDeque<TraceEntry>,
    cap: usize,
    dropped: u64,
    counters: BTreeMap<String, Counter>,
}

impl TraceStore {
    pub fn new(cap: usize) -> Self {
        Self {
            items: VecDeque::new(),
            cap: cap.max(1),
            dropped: 0,
            counters: BTreeMap::new(),
        }
    }

    pub fn add(&mut self, entry: TraceEntry) {
        let counter = self.counters.entry(entry.command_name.clone()).or_default();
        match entry.direction {
            Direction::Out => counter.sent += 1,
            Direction::In => counter.received += 1,
        }

        self.items.push_back(entry);
        self.trim();
    }

    /// Entries from row ordinal `from` onwards, at most `limit` of them.
    pub fn entries(&self, from: Option<u64>, limit: Option<usize>) -> Vec<TraceEntry> {
        self.items
            .iter()
            .filter(|e| from.map(|f| e.seq >= f).unwrap_or(true))
            .take(limit.unwrap_or(usize::MAX))
            .cloned()
            .collect()
    }

    pub fn entry(&self, seq: u64) -> Option<&TraceEntry> {
        self.items.iter().find(|e| e.seq == seq)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Entries discarded to stay inside the cap.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    pub fn counters(&self) -> &BTreeMap<String, Counter> {
        &self.counters
    }

    pub fn set_cap(&mut self, cap: usize) {
        self.cap = cap.max(1);
        self.trim();
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.counters.clear();
        self.dropped = 0;
    }

    fn trim(&mut self) {
        while self.items.len() > self.cap {
            self.items.pop_front();
            self.dropped += 1;
        }
    }
}

impl Default for TraceStore {
    fn default() -> Self {
        Self::new(DEFAULT_TRACE_CAP)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(seq: u64, direction: Direction, command: &str, summary: &str) -> TraceEntry {
        TraceEntry {
            seq,
            at: 1_700_000_000_000,
            direction,
            command_name: command.to_string(),
            command_status: 0,
            sequence_number: 1,
            summary: summary.to_string(),
            raw: vec![0, 0, 0, 16],
            decode_error: None,
        }
    }

    #[test]
    fn keeps_entries_in_insertion_order() {
        let mut s = TraceStore::new(DEFAULT_TRACE_CAP);
        s.add(entry(1, Direction::Out, "submit_sm", "first"));
        s.add(entry(2, Direction::Out, "submit_sm", "second"));
        let summaries: Vec<_> = s.entries(None, None).iter().map(|e| e.summary.clone()).collect();
        assert_eq!(summaries, vec!["first", "second"]);
    }

    #[test]
    fn evicts_the_oldest_entry_past_the_cap_and_counts_it_in_dropped() {
        let mut s = TraceStore::new(2);
        s.add(entry(1, Direction::Out, "submit_sm", "a"));
        s.add(entry(2, Direction::Out, "submit_sm", "b"));
        s.add(entry(3, Direction::Out, "submit_sm", "c"));
        let summaries: Vec<_> = s.entries(None, None).iter().map(|e| e.summary.clone()).collect();
        assert_eq!(summaries, vec!["b", "c"]);
        assert_eq!(s.dropped(), 1);
    }

    #[test]
    fn counts_sent_and_received_per_command_name() {
        let mut s = TraceStore::new(DEFAULT_TRACE_CAP);
        s.add(entry(1, Direction::Out, "submit_sm", ""));
        s.add(entry(2, Direction::Out, "submit_sm", ""));
        s.add(entry(3, Direction::In, "submit_sm_resp", ""));
        s.add(entry(4, Direction::In, "deliver_sm", ""));
        assert_eq!(s.counters()["submit_sm"], Counter { sent: 2, received: 0 });
        assert_eq!(s.counters()["submit_sm_resp"], Counter { sent: 0, received: 1 });
        assert_eq!(s.counters()["deliver_sm"], Counter { sent: 0, received: 1 });
    }

    #[test]
    fn does_not_decrement_counters_when_an_entry_is_evicted() {
        let mut s = TraceStore::new(1);
        s.add(entry(1, Direction::Out, "submit_sm", ""));
        s.add(entry(2, Direction::Out, "submit_sm", ""));
        assert_eq!(s.entries(None, None).len(), 1);
        assert_eq!(s.counters()["submit_sm"].sent, 2);
    }

    #[test]
    fn clears_entries_and_counters() {
        let mut s = TraceStore::new(DEFAULT_TRACE_CAP);
        s.add(entry(1, Direction::Out, "submit_sm", ""));
        s.clear();
        assert!(s.entries(None, None).is_empty());
        assert!(s.counters().is_empty());
        assert_eq!(s.dropped(), 0);
    }

    #[test]
    fn returns_a_page_starting_at_a_row_ordinal() {
        let mut s = TraceStore::new(DEFAULT_TRACE_CAP);
        for i in 1..=10 {
            s.add(entry(i, Direction::Out, "submit_sm", &format!("row-{i}")));
        }
        let page: Vec<_> = s.entries(Some(4), Some(3)).iter().map(|e| e.summary.clone()).collect();
        assert_eq!(page, vec!["row-4", "row-5", "row-6"]);
    }

    #[test]
    fn returns_an_empty_page_past_the_end() {
        let mut s = TraceStore::new(DEFAULT_TRACE_CAP);
        s.add(entry(1, Direction::Out, "submit_sm", ""));
        assert!(s.entries(Some(99), Some(10)).is_empty());
    }

    #[test]
    fn raises_its_cap_without_losing_what_it_holds() {
        let mut s = TraceStore::new(2);
        s.add(entry(1, Direction::Out, "submit_sm", "a"));
        s.add(entry(2, Direction::Out, "submit_sm", "b"));
        s.set_cap(10);
        s.add(entry(3, Direction::Out, "submit_sm", "c"));
        assert_eq!(s.entries(None, None).len(), 3);
    }

    #[test]
    fn trims_to_a_lowered_cap_and_counts_what_it_dropped() {
        let mut s = TraceStore::new(10);
        for i in 1..=5 {
            s.add(entry(i, Direction::Out, "submit_sm", &format!("row-{i}")));
        }
        s.set_cap(2);
        let summaries: Vec<_> = s.entries(None, None).iter().map(|e| e.summary.clone()).collect();
        assert_eq!(summaries, vec!["row-4", "row-5"]);
        assert_eq!(s.dropped(), 3);
    }

    #[test]
    fn holds_a_decode_error_entry_with_its_bytes_and_no_summary() {
        let mut s = TraceStore::new(DEFAULT_TRACE_CAP);
        let mut e = entry(1, Direction::In, "(undecodable)", "");
        e.decode_error = Some("command_length 4 is below the 16-byte header".into());
        e.raw = vec![0xde, 0xad, 0xbe, 0xef];
        s.add(e);
        let got = &s.entries(None, None)[0];
        assert!(got.decode_error.as_ref().unwrap().contains("below the 16-byte header"));
        assert!(got.summary.is_empty());
        assert_eq!(got.raw, vec![0xde, 0xad, 0xbe, 0xef]);
    }
}
