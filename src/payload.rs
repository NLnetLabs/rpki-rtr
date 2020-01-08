/// The data being transmitted via RTR.
///
/// The types in here provide a more compact representation than the PDUs.
/// They also implement all the traits to use them as keys in collections to
/// be able to perform difference processing.

use std::net::{Ipv4Addr, Ipv6Addr};


#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Ipv4Prefix {
    pub prefix: Ipv4Addr,
    pub prefix_len: u8,
    pub max_len: u8,
    pub asn: u32
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Ipv6Prefix {
    pub prefix: Ipv6Addr,
    pub prefix_len: u8,
    pub max_len: u8,
    pub asn: u32
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Payload {
    V4(Ipv4Prefix),
    V6(Ipv6Prefix)
}


#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Action {
    Announce,
    Withdraw,
}

impl Action {
    pub fn from_flags(flags: u8) -> Self {
        if flags & 1 == 1 {
            Action::Announce
        }
        else {
            Action::Withdraw
        }
    }

    pub fn into_flags(self) -> u8 {
        match self {
            Action::Announce => 1,
            Action::Withdraw => 0
        }
    }
}

