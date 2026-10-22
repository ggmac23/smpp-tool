//! An `Emitter` that keeps what it was given, so the controller's tests can
//! assert on what reached the webview without a window.

use std::sync::Mutex;

use crate::app::events::Emitter;

#[derive(Default)]
pub struct Recorder {
    sent: Mutex<Vec<(String, serde_json::Value)>>,
}

impl Emitter for Recorder {
    fn emit(&self, name: &str, payload: serde_json::Value) {
        self.sent.lock().unwrap().push((name.to_string(), payload));
    }
}

impl Recorder {
    pub fn names(&self) -> Vec<String> {
        self.sent
            .lock()
            .unwrap()
            .iter()
            .map(|(n, _)| n.clone())
            .collect()
    }

    pub fn payloads(&self, name: &str) -> Vec<serde_json::Value> {
        self.sent
            .lock()
            .unwrap()
            .iter()
            .filter(|(n, _)| n == name)
            .map(|(_, p)| p.clone())
            .collect()
    }

    /// Every item of every batch sent under `name`, flattened.
    pub fn batched(&self, name: &str) -> Vec<serde_json::Value> {
        self.payloads(name)
            .into_iter()
            .filter_map(|p| p.as_array().cloned())
            .flatten()
            .collect()
    }
}
