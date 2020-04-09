//! RTR: the RPKI to Router Protocol.
//!
//! RPKI, the Resource Public Key Infrastructure, is a distributed database of
//! signed statements by entities that participate in Internet routing. A
//! typical setup to facilitate this information when making routing decisions
//! first collects and validates all statements into something called a
//! _local cache_ and distributes validated and normalized information from
//! the cache to the actual routers or route servers. The standardized
//! protocol for this distribution is the RPKI to Router Protocol or RTR for
//! short.
//!
//! This crate implements both the server and client side of RTR. Both of
//! these are built atop [Tokio]. They are generic over the concrete socket
//! type and can thus be used with different transports. They also are generic
//! over a type that provides or consumes the data. For more details, see the
//! [server] and [client] modules.
//!
//! The create implements both versions 0 and 1 of the protocol. It does not,
//! currently, support router keys, though.
//!
//! You can read more about RPKI in [RFC 6480]. RTR is currently specified in
//! [RFC 8210].
//!
//! [client]: client/index.html
//! [server]: server/index.html
//! [Tokio]: https://crates.io/crates/tokio
//! [RFC 6480]: https://tools.ietf.org/html/rfc6480
//! [RFC 8210]: https://tools.ietf.org/html/rfc8210

pub mod client;
pub mod payload;
pub mod pdu;
pub mod serial;
pub mod server;

