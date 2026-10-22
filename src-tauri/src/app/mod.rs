//! Everything with side effects: the socket, the session, the trace, the burst
//! engine and the profile store. The protocol layer in `smpp` stays pure.

pub mod burst;
pub mod compose;
pub mod dlr;
pub mod events;
pub mod export;
pub mod loadtest;
pub mod profiles;
pub mod recipients;
pub mod session;
pub mod trace;
pub mod transport;
pub mod wire;
