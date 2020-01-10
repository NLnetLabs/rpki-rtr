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

macro_rules! concrete {
    ( $type:ident ) => {
        common!($type);

        #[allow(dead_code)]
        impl $type {
            pub fn version(&self) -> u8 {
                self.header.version()
            }

            pub fn session(&self) -> u16 {
                self.header.session()
            }

            pub async fn read<Sock: AsyncRead + Unpin>(
                sock: &mut Sock 
            ) -> Result<Self, io::Error> {
                let mut res = Self::default();
                sock.read_exact(res.header.as_mut()).await?;
                if res.header.pdu() != Self::PDU {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        concat!(
                            "PDU type mismatch when expecting ",
                            stringify!($type)
                        )
                    ))
                }
                if res.header.length() as usize != res.as_ref().len() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        concat!(
                            "invalid length for ",
                            stringify!($type)
                        )
                    ))
                }
                sock.read_exact(&mut res.as_mut()[Header::LEN..]).await?;
                Ok(res)
            }

            pub async fn try_read<Sock: AsyncRead + Unpin>(
                sock: &mut Sock 
            ) -> Result<Result<Self, Header>, io::Error> {
                let mut res = Self::default();
                sock.read_exact(res.header.as_mut()).await?;
                if res.header.pdu() == ERROR_PDU {
                    // Since we should drop the session after an error, we
                    // can safely ignore all the rest of the error for now.
                    return Ok(Err(res.header))
                }
                if res.header.pdu() != Self::PDU {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        concat!(
                            "PDU type mismatch when expecting ",
                            stringify!($type)
                        )
                    ))
                }
                if res.header.length() as usize != res.as_ref().len() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        concat!(
                            "invalid length for ",
                            stringify!($type)
                        )
                    ))
                }
                sock.read_exact(&mut res.as_mut()[Header::LEN..]).await?;
                Ok(Ok(res))
            }

            pub async fn read_payload<Sock: AsyncRead + Unpin>(
                header: Header, sock: &mut Sock
            ) -> Result<Self, io::Error> {
                if header.length() as usize != mem::size_of::<Self>() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        concat!(
                            "invalid length for ",
                            stringify!($type),
                            " PDU"
                        )
                    ))
                }
                let mut res = Self::default();
                sock.read_exact(&mut res.as_mut()[Header::LEN..]).await?;
                res.header = header;
                Ok(res)
            }
        }
    }
}


//------------ SerialNotify --------------------------------------------------

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

concrete!(SerialNotify);


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
    pub fn serial(&self) -> Serial {
        self.payload.serial()
    }
}

concrete!(SerialQuery);


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

    pub async fn read<Sock: AsyncRead + Unpin>(
        sock: &mut Sock 
    ) -> Result<Self, io::Error> {
        let mut res = Self::default();
        sock.read_exact(res.as_mut()).await?;
        Ok(res)
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

concrete!(ResetQuery);


//------------ CacheResponse -------------------------------------------------

#[derive(Default)]
#[repr(packed)]
pub struct CacheResponse {
    #[allow(dead_code)]
    header: Header
}

impl CacheResponse {
    pub const PDU: u8 = 3;

    pub fn new(version: u8, session: u16) -> Self {
        CacheResponse {
            header: Header::new(version, 3, session, 8)
        }
    }
}

concrete!(CacheResponse);


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
    pub const PDU: u8 = 4;

    pub fn new(
        version: u8,
        flags: u8,
        prefix_len: u8,
        max_len: u8,
        prefix: Ipv4Addr,
        asn: u32
    ) -> Self {
        Ipv4Prefix {
            header: Header::new(version, Self::PDU, 0, 20),
            flags,
            prefix_len,
            max_len,
            zero: 0,
            prefix: u32::from(prefix).to_be(),
            asn: asn.to_be()
        }
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

concrete!(Ipv4Prefix);


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
    pub const PDU: u8 = 6;

    pub fn new(
        version: u8,
        flags: u8,
        prefix_len: u8,
        max_len: u8,
        prefix: Ipv6Addr,
        asn: u32
    ) -> Self {
        Ipv6Prefix {
            header: Header::new(version, Self::PDU, 0, 32),
            flags,
            prefix_len,
            max_len,
            zero: 0,
            prefix: u128::from(prefix).to_be(),
            asn: asn.to_be()
        }
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

concrete!(Ipv6Prefix);


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

    pub async fn read<Sock: AsyncRead + Unpin>(
        sock: &mut Sock
    ) -> Result<Result<Self, EndOfData>, io::Error> {
        let header = Header::read(sock).await?;
        match header.pdu {
            Ipv4Prefix::PDU => {
                Ipv4Prefix::read_payload(header, sock).await.map(|res| {
                    Ok(Payload::V4(res))
                })
            }
            Ipv6Prefix::PDU => {
                Ipv6Prefix::read_payload(header, sock).await.map(|res| {
                    Ok(Payload::V6(res))
                })
            }
            // XXX Ignore RouterKey
            EndOfData::PDU => {
                EndOfData::read_payload(header, sock).await.map(Err)
            }
            _ => {
                Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "unexpected PDU in payload sequence"
                ))
            }
        }
    }

    pub fn version(&self) -> u8 {
        match *self {
            Payload::V4(ref data) => data.version(),
            Payload::V6(ref data) => data.version(),
        }
    }

    pub fn flags(&self) -> u8 {
        match *self {
            Payload::V4(ref data) => data.flags(),
            Payload::V6(ref data) => data.flags(),
        }
    }

    pub async fn write<A: AsyncWrite + Unpin>(
        &self,
        a: &mut A
    ) -> Result<(), io::Error> {
        a.write_all(self.as_ref()).await
    }

    pub fn into_payload(self) -> (payload::Action, payload::Payload) {
        (
            payload::Action::from_flags(self.flags()),
            match self {
                Payload::V4(data) => {
                    payload::Payload::V4(payload::Ipv4Prefix {
                        prefix: data.prefix(),
                        prefix_len: data.prefix_len(),
                        max_len: data.max_len(),
                        asn: data.asn(),
                    })
                }
                Payload::V6(data) => {
                    payload::Payload::V6(payload::Ipv6Prefix {
                        prefix: data.prefix(),
                        prefix_len: data.prefix_len(),
                        max_len: data.max_len(),
                        asn: data.asn(),
                    })
                }
            }
        )
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
    pub const PDU: u8 = 7;

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

    pub async fn read_payload<Sock: AsyncRead + Unpin>(
        header: Header, sock: &mut Sock
    ) -> Result<Self, io::Error> {
        match header.version() {
            0 => {
                EndOfDataV0::read_payload(header, sock)
                    .await.map(EndOfData::V0)
            }
            1 => {
                EndOfDataV1::read_payload(header, sock)
                    .await.map(EndOfData::V1)
            }
            _ => {
                Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid version in end of data PDU"
                ))
            }
        }
    }

    pub fn version(&self) -> u8 {
        match *self {
            EndOfData::V0(_) => 0,
            EndOfData::V1(_) => 1,
        }
    }

    pub fn session(&self) -> u16 {
        match *self {
            EndOfData::V0(ref data) => data.session(),
            EndOfData::V1(ref data) => data.session(),
        }
    }

    pub fn serial(&self) -> Serial {
        match *self {
            EndOfData::V0(ref data) => data.serial(),
            EndOfData::V1(ref data) => data.serial(),
        }
    }

    pub fn timing(&self) -> Option<Timing> {
        match *self {
            EndOfData::V0(_) => None,
            EndOfData::V1(ref data) => {
                Some(Timing {
                    refresh: data.refresh(),
                    retry: data.retry(),
                    expire: data.expire()
                })
            }
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
    pub const PDU: u8 = 7;

    pub fn new(session: u16, serial: Serial) -> Self {
        EndOfDataV0 {
            header: Header::new(0, Self::PDU, session, 12),
            serial: serial.to_be()
        }
    }

    pub fn serial(&self) -> Serial {
        Serial::from_be(self.serial)
    }
}

concrete!(EndOfDataV0);
    

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
    pub const PDU: u8 = 7;

    pub fn new(
        version: u8,
        session: u16,
        serial: Serial,
        refresh: u32,
        retry: u32,
        expire: u32
    ) -> Self {
        EndOfDataV1 {
            header: Header::new(version, Self::PDU, session, 24),
            serial: serial.to_be(),
            refresh: refresh.to_be(),
            retry: retry.to_be(),
            expire: expire.to_be(),
        }
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

concrete!(EndOfDataV1);


//------------ CacheReset ----------------------------------------------------

#[derive(Default)]
#[repr(packed)]
#[allow(dead_code)] 
pub struct CacheReset {
    header: Header
}

#[allow(dead_code)] 
impl CacheReset {
    pub const PDU: u8 = 7;

    pub fn new(version: u8) -> Self {
        CacheReset {
            header: Header::new(version, Self::PDU, 0, 8)
        }
    }
}

concrete!(CacheReset);


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

pub const ERROR_PDU: u8 = 10;


impl<P, T> Error<P, T> 
where
    P: Sized + 'static + Send + Sync,
    T: Sized + 'static + Send + Sync,
{
    pub const PDU: u8 = ERROR_PDU;

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
    pub const LEN: usize = mem::size_of::<Self>();

    pub fn new(version: u8, pdu: u8, session: u16, length: u32) -> Self {
        Header {
            version,
            pdu,
            session: session.to_be(),
            length: length.to_be(),
        }
    }

    pub async fn read<Sock: AsyncRead + Unpin>(
        sock: &mut Sock 
    ) -> Result<Self, io::Error> {
        let mut res = Self::default();
        sock.read_exact(res.as_mut()).await?;
        Ok(res)
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


//------------ Timing --------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct Timing {
    pub refresh: u32,
    pub retry: u32,
    pub expire: u32
}

impl Default for Timing {
    fn default() -> Self {
        Timing {
            refresh: 3600,
            retry: 600,
            expire: 7200
        }
    }
}

