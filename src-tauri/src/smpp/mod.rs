//! The SMPP protocol layer: codec, text, segmentation, receipts.
//!
//! Pure. No sockets, no disk, no Tauri. Everything here can be exercised as a
//! library, which is what makes the anti-goals testable: that the bytes are
//! never hidden, never silently repaired, and never invented.

pub mod constants;
pub mod decode;
pub mod defs;
pub mod encode;
pub mod frame;
pub mod receipt;
pub mod segment;
pub mod summary;
pub mod text;
pub mod tlv;
