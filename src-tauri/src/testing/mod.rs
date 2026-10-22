//! Helpers shared by the tests. Byte builders live here so the codec tests
//! never use the encoder to construct their own inputs — a codec that agrees
//! with itself proves nothing.

#[cfg(test)]
pub mod bytes;
pub mod fake_smsc;
