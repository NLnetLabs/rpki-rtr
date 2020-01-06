//! RTR PDUs.
//!
//! This module contains types that represent the protocol data units of
//! RPKI-RTR in their wire representation. That is, these types can be
//! used given to read and write operations as buffers.
//! See section 5 of RFC 6810 and RFC 8210. Annoyingly, the format of the
//! `EndOfData` PDU changes between the two versions.

use std::{io, mem, slice};
use std::marker::Unpin;
use std::net::{Ipv4Addr, Ipv6Addr};
use tokio::io::{
    AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt
};
use crate::payload;
use super::serial::Serial;


//------------ Macro for Common Impls ----------------------------------------

macro_rules! common {
    ( $type:ident ) => {
        #[allow(dead_code)]
        impl $type {
            pub async fn read<'a, A: AsyncRead + Unpin>(
                a: &mut A
            ) -> Result<Self, io::Error> {
                let res = Self::default();
                a.read_exact(&mut Self::default().as_mut()).await?;
                Ok(res)
            }

            pub async fn write<A: AsyncWrite + Unpin>(
                &self,
                a: &mut A
            ) -> Result<(), io::Error> {
                a.write_all(self.as_ref()).await
            }
        }

        impl AsRef<[u8]> for $type {
            fn as_ref(&self) -> &[u8] {
                unsafe {
                    slice::from_raw_parts(
                        self as *const Self as *const u8,
                        mem::size_of::<Self>()
                    )
                }
            }
        }

        impl AsMut<[u8]> for $type {
            fn as_mut(&mut self) -> &mut [u8] {
                unsafe {
                    slice::from_raw_parts_mut(
                        self as *mut Self as *mut u8,
                        mem::size_of::<Self>()
                    )
                }
            }
        }
    }
}


//------------ SerialQuery ---------------------------------------------------

#[derive(Default)]
#[repr(packed)]
#[allow(dead_code)]
pub struct SerialNotify {
    header: Header,
    serial: u32,
}

impl SerialNotify {
    pub const PDU: u8 = 0;
    pub const LEN: u32 = 12;

    pub fn new(version: u8, session: u16, serial: Serial) -> Self {
        SerialNotify {
            header: Header::new(version, Self::PDU, session, Self::LEN),
            serial: serial.to_be(),
        }
    }
}

common!(SerialNotify);


//------------ SerialQuery ---------------------------------------------------

pub const SERIAL_QUERY_PDU: u8 = 1;
pub const SERIAL_QUERY_LEN: u32 = 12;

#[allow(dead_code)]  // We currently don’t need this, but might later ...
#[derive(Default)]
#[repr(packed)]
pub struct SerialQuery {
    header: Header,
    payload: SerialQueryPayload,
}

#[allow(dead_code)]
impl SerialQuery {
    pub const PDU: u8 = 1;
    pub const LEN: u32 = 12;

    pub fn new(version: u8, session: u16, serial: Serial) -> Self {
        SerialQuery {
            header: Header::new(version, Self::PDU, session, 12),
            payload: SerialQueryPayload::new(serial),
        }
    }

    pub fn version(&self) -> u8 {
        self.header.version
    }

    pub fn session(&self) -> u16 {
        u16::from_be(self.header.session)
    }

    pub fn serial(&self) -> Serial {
        self.payload.serial()
    }
}

common!(SerialQuery);


//------------ SerialQueryPayload --------------------------------------------

#[derive(Default)]
#[repr(packed)]
pub struct SerialQueryPayload {
    serial: u32
}

impl SerialQueryPayload {
    pub fn new(serial: Serial) -> Self {
        SerialQueryPayload {
            serial: serial.to_be()
        }
    }

    pub fn serial(&self) -> Serial {
        Serial::from_be(self.serial)
    }
}

common!(SerialQueryPayload);


//------------ ResetQuery ----------------------------------------------------

#[derive(Default)]
#[repr(packed)]
pub struct ResetQuery {
    #[allow(dead_code)]
    header: Header
}

impl ResetQuery {
    pub const PDU: u8 = 2;
    pub const LEN: u32 = 8;

    #[allow(dead_code)]
    pub fn new(version: u8) -> Self {
        ResetQuery {
            header: Header::new(version, 2, 0, 8)
        }
    }
}

common!(ResetQuery);


//------------ CacheResponse -------------------------------------------------

#[derive(Default)]
#[repr(packed)]
pub struct CacheResponse {
    #[allow(dead_code)]
    header: Header
}

impl CacheResponse {
    pub fn new(version: u8, session: u16) -> Self {
        CacheResponse {
            header: Header::new(version, 3, session, 8)
        }
    }
}

common!(CacheResponse);


//------------ Ipv4Prefix ----------------------------------------------------

#[derive(Default)]
#[repr(packed)]
#[allow(dead_code)]
pub struct Ipv4Prefix {
    header: Header,
    flags: u8,
    prefix_len: u8,
    max_len: u8,
    zero: u8,
    prefix: u32,
    asn: u32
}

#[allow(dead_code)]
impl Ipv4Prefix {
    pub fn new(
        version: u8,
        flags: u8,
        prefix_len: u8,
        max_len: u8,
        prefix: Ipv4Addr,
        asn: u32
    ) -> Self {
        Ipv4Prefix {
            header: Header::new(version, 4, 0, 20),
            flags,
            prefix_len,
            max_len,
            zero: 0,
            prefix: u32::from(prefix).to_be(),
            asn: asn.to_be()
        }
    }

    pub fn version(&self) -> u8 {
        self.header.version
    }

    pub fn flags(&self) -> u8 {
        self.flags
    }

    pub fn prefix_len(&self) -> u8 {
        self.prefix_len
    }

    pub fn max_len(&self) -> u8 {
        self.max_len
    }

    pub fn prefix(&self) -> Ipv4Addr {
        u32::from_be(self.prefix).into()
    }

    pub fn asn(&self) -> u32 {
        u32::from_be(self.asn)
    }
}

common!(Ipv4Prefix);


//------------ Ipv6Prefix ----------------------------------------------------

#[derive(Default)]
#[repr(packed)]
#[allow(dead_code)] 
pub struct Ipv6Prefix {
    header: Header,
    flags: u8,
    prefix_len: u8,
    max_len: u8,
    zero: u8,
    prefix: u128,
    asn: u32,
}

#[allow(dead_code)] 
impl Ipv6Prefix {
    pub fn new(
        version: u8,
        flags: u8,
        prefix_len: u8,
        max_len: u8,
        prefix: Ipv6Addr,
        asn: u32
    ) -> Self {
        Ipv6Prefix {
            header: Header::new(version, 6, 0, 32),
            flags,
            prefix_len,
            max_len,
            zero: 0,
            prefix: u128::from(prefix).to_be(),
            asn: asn.to_be()
        }
    }

    pub fn version(&self) -> u8 {
        self.header.version
    }

    pub fn flags(&self) -> u8 {
        self.flags
    }

    pub fn prefix_len(&self) -> u8 {
        self.prefix_len
    }

    pub fn max_len(&self) -> u8 {
        self.max_len
    }

    pub fn prefix(&self) -> Ipv6Addr {
        u128::from_be(self.prefix).into()
    }

    pub fn asn(&self) -> u32 {
        u32::from_be(self.asn)
    }
}

common!(Ipv6Prefix);


//------------ Payload -------------------------------------------------------

pub enum Payload {
    V4(Ipv4Prefix),
    V6(Ipv6Prefix),
}

impl Payload {
    pub fn new(version: u8, flags: u8, payload: payload::Payload) -> Self {
        match payload {
            payload::Payload::V4(prefix) => {
                Payload::V4(
                    Ipv4Prefix::new(
                        version,
                        flags,
                        prefix.prefix_len,
                        prefix.max_len,
                        prefix.prefix,
                        prefix.asn
                    )
                )
            }
            payload::Payload::V6(prefix) => {
                Payload::V6(
                    Ipv6Prefix::new(
                        version,
                        flags,
                        prefix.prefix_len,
                        prefix.max_len,
                        prefix.prefix,
                        prefix.asn
                    )
                )
            }
        }
    }

    pub async fn write<A: AsyncWrite + Unpin>(
        &self,
        a: &mut A
    ) -> Result<(), io::Error> {
        a.write_all(self.as_ref()).await
    }
}

impl AsRef<[u8]> for Payload {
    fn as_ref(&self) -> &[u8] {
        match *self {
            Payload::V4(ref prefix) => prefix.as_ref(),
            Payload::V6(ref prefix) => prefix.as_ref(),
        }
    }
}

impl AsMut<[u8]> for Payload {
    fn as_mut(&mut self) -> &mut [u8] {
        match *self {
            Payload::V4(ref mut prefix) => prefix.as_mut(),
            Payload::V6(ref mut prefix) => prefix.as_mut(),
        }
    }
}


//------------ EndOfData -----------------------------------------------------

/// Generic End-of-Data PDU.
///
/// This PDU differs between version 0 and 1 of RTR. Consequently, this
/// generic version is an enum that can be both, depending on the version
/// requested.
pub enum EndOfData {
    V0(EndOfDataV0),
    V1(EndOfDataV1),
}

impl EndOfData {
    pub fn new(
        version: u8,
        session: u16,
        serial: Serial,
        refresh: u32,
        retry: u32,
        expire: u32
    ) -> Self {
        if version == 0 {
            EndOfData::V0(EndOfDataV0::new(session, serial))
        }
        else {
            EndOfData::V1(EndOfDataV1::new(
                version, session, serial, refresh, retry, expire
            ))
        }
    }

    pub async fn write<A: AsyncWrite + Unpin>(
        &self, a: &mut A
    ) -> Result<(), io::Error> {
        a.write_all(self.as_ref()).await
    }
}

impl AsRef<[u8]> for EndOfData {
    fn as_ref(&self) -> &[u8] {
        match *self {
            EndOfData::V0(ref inner) => inner.as_ref(),
            EndOfData::V1(ref inner) => inner.as_ref(),
        }
    }
}

impl AsMut<[u8]> for EndOfData {
    fn as_mut(&mut self) -> &mut [u8] {
        match *self {
            EndOfData::V0(ref mut inner) => inner.as_mut(),
            EndOfData::V1(ref mut inner) => inner.as_mut(),
        }
    }
}


//------------ EndOfDataV0 ---------------------------------------------------

#[derive(Default)]
#[repr(packed)]
#[allow(dead_code)]
pub struct EndOfDataV0 {
    header: Header,
    serial: u32
}

#[allow(dead_code)]
impl EndOfDataV0 {
    pub fn new(session: u16, serial: Serial) -> Self {
        EndOfDataV0 {
            header: Header::new(0, 7, session, 12),
            serial: serial.to_be()
        }
    }

    pub fn version(&self) -> u8 {
        self.header.version
    }

    pub fn session(&self) -> u16 {
        u16::from_be(self.header.session)
    }

    pub fn serial(&self) -> Serial {
        Serial::from_be(self.serial)
    }
}

common!(EndOfDataV0);
    

//------------ EndOfDataV1 ---------------------------------------------------

#[derive(Default)]
#[repr(packed)]
#[allow(dead_code)] 
pub struct EndOfDataV1 {
    header: Header,
    serial: u32,
    refresh: u32,
    retry: u32,
    expire: u32,
}

#[allow(dead_code)] 
impl EndOfDataV1 {
    pub fn new(
        version: u8,
        session: u16,
        serial: Serial,
        refresh: u32,
        retry: u32,
        expire: u32
    ) -> Self {
        EndOfDataV1 {
            header: Header::new(version, 7, session, 24),
            serial: serial.to_be(),
            refresh: refresh.to_be(),
            retry: retry.to_be(),
            expire: expire.to_be(),
        }
    }

    pub fn version(&self) -> u8 {
        self.header.version
    }

    pub fn session(&self) -> u16 {
        u16::from_be(self.header.session)
    }

    pub fn serial(&self) -> Serial {
        Serial::from_be(self.serial)
    }

    pub fn refresh(&self) -> u32 {
        u32::from_be(self.refresh)
    }

    pub fn retry(&self) -> u32 {
        u32::from_be(self.retry)
    }

    pub fn expire(&self) -> u32 {
        u32::from_be(self.expire)
    }
}

common!(EndOfDataV1);


//------------ CacheReset ----------------------------------------------------

#[derive(Default)]
#[repr(packed)]
#[allow(dead_code)] 
pub struct CacheReset {
    header: Header
}

#[allow(dead_code)] 
impl CacheReset {
    pub fn new(version: u8) -> Self {
        CacheReset {
            header: Header::new(version, 8, 0, 8)
        }
    }

    pub fn version(&self) -> u8 {
        self.header.version
    }
}

common!(CacheReset);


//------------ Error ---------------------------------------------------------

#[derive(Default)]
#[repr(packed)]
#[allow(dead_code)]
pub struct Error<P: Sized, T: Sized> {
    header: Header,
    pdu_len: u32,
    pdu: P,
    text_len: u32,
    text: T
}

impl<P, T> Error<P, T> 
where
    P: Sized + 'static + Send + Sync,
    T: Sized + 'static + Send + Sync,
{
    pub fn new(
        version: u8,
        error_code: u16,
        pdu: P,
        text: T
    ) -> Self {
        Error {
            header: Header::new(
                version, 10, error_code,
                16 + mem::size_of::<P>() as u32 + mem::size_of::<T>() as u32
            ),
            pdu_len: (mem::size_of::<P>() as u32).to_be(),
            pdu,
            text_len: (mem::size_of::<T>() as u32).to_be(),
            text
        }
    }

    pub fn boxed(self) -> BoxedError {
        BoxedError(Box::new(self))
    }
}

impl<P: Sized, T: Sized> Error<P, T> {
    pub async fn read<A: AsyncRead + Unpin>(
        a: &mut A
    ) -> Result<Self, io::Error>
    where P: Default, T: Default {
        let mut res = Self::default();
        a.read_exact(res.as_mut()).await?;
        Ok(res)
    }

    pub async fn write<A: AsyncWrite + Unpin>(
        &self, a: &mut A
    ) -> Result<(), io::Error> {
        a.write_all(self.as_ref()).await
    }
}

impl<P: Sized, T: Sized> AsRef<[u8]> for Error<P, T> {
    fn as_ref(&self) -> &[u8] {
        unsafe {
            slice::from_raw_parts(
                self as *const Self as *const u8,
                mem::size_of::<Self>()
            )
        }
    }
}

impl<P: Sized, T: Sized> AsMut<[u8]> for Error<P, T> {
    fn as_mut(&mut self) -> &mut [u8] {
        unsafe {
            slice::from_raw_parts_mut(
                self as *mut Self as *mut u8,
                mem::size_of::<Self>()
            )
        }
    }
}


//------------ BoxedError ----------------------------------------------------

pub struct BoxedError(Box<dyn AsRef<[u8]> + Sync + Send>);

impl BoxedError {
    pub async fn write<A: AsyncWrite + Unpin>(
        &self, a: &mut A
    ) -> Result<(), io::Error> {
        a.write_all(self.as_ref()).await
    }
}

impl AsRef<[u8]> for BoxedError {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref().as_ref()
    }
}


//------------ Header --------------------------------------------------------

#[derive(Clone, Copy, Default)]
#[repr(packed)]
pub struct Header {
    version: u8,
    pdu: u8,
    session: u16,
    length: u32,
}

impl Header {
    pub fn new(version: u8, pdu: u8, session: u16, length: u32) -> Self {
        Header {
            version,
            pdu,
            session: session.to_be(),
            length: length.to_be(),
        }
    }

    pub fn version(self) -> u8 {
        self.version
    }

    pub fn pdu(self) -> u8 {
        self.pdu
    }

    pub fn session(self) -> u16 {
        u16::from_be(self.session)
    }

    pub fn length(self) -> u32 {
        u32::from_be(self.length)
    }
}

common!(Header);

