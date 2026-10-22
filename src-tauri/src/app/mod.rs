//! Everything with side effects: the socket, the session, the trace, the burst
//! engine and the profile store. The protocol layer in `smpp` stays pure.

pub mod dlr;
pub mod transport;
