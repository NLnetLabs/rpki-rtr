use std::io;
use std::marker::Unpin;
use futures::future::Future;
use tokio::time::{Duration, timeout};
use tokio::io::{AsyncRead, AsyncWrite};
use crate::payload::{Action, Payload};
use crate::pdu;
use crate::serial::Serial;


//------------ VrpTarget -----------------------------------------------------

pub trait VrpTarget {
    type Input: VrpInput;

    fn start(&mut self, reset: bool) -> Self::Input;
}

pub trait VrpInput {
    fn push(&mut self, action: Action, payload: Payload);
    fn done(self, timing: pdu::Timing) -> Result<(), io::Error>;
}


//------------ Client --------------------------------------------------------

pub struct Client<Sock, Target> {
    sock: Sock,
    store: Target,
    state: Option<(u16, Serial)>,
    version: Option<u8>,
    timing: pdu::Timing,
}

impl<Sock, Target> Client<Sock, Target> {
    pub fn new(
        sock: Sock,
        store: Target,
        state: Option<(u16, Serial)>
    ) -> Self {
        Client {
            sock, store, state,
            version: None,
            timing: pdu::Timing::default(),
        }
    }
}

impl<Sock, Target> Client<Sock, Target>
where
    Sock: AsyncRead + AsyncWrite + Unpin,
    Target: VrpTarget
{
    pub async fn run(&mut self) -> Result<(), io::Error> {
        match self._run().await {
            Ok(()) => Ok(()),
            Err(err) => {
                if err.kind() == io::ErrorKind::UnexpectedEof {
                    Ok(())
                }
                else {
                    Err(err)
                }
            }
        }
    }

    pub async fn _run(&mut self) -> Result<(), io::Error> {
        // End of loop via an io::Error.
        loop {
            match self.state {
                Some((session, serial)) => {
                    if !self.serial(session, serial).await? {
                        // Do it again! Do it again!
                        continue
                    }
                }
                None => {
                    self.reset().await?;
                }
            }

            match timeout(
                Duration::from_secs(u64::from(self.timing.refresh)),
                pdu::SerialNotify::read(&mut self.sock)
            ).await {
                Ok(Err(err)) => return Err(err),
                _ => { }
            }
        }
    }

    /// Perform a serial query.
    ///
    /// Returns `Ok(true)` if the query succeeded and the client should now
    /// wait for a while. Returns `Ok(false)` if the server reported a
    /// restart and we need to proceed with a reset query. Returns an error in
    /// any other case.
    async fn serial(
        &mut self, session: u16, serial: Serial
    ) -> Result<bool, io::Error> {
        pdu::SerialQuery::new(
            self.version.unwrap_or(1), session, serial
        ).write(&mut self.sock).await?;
        let start = match self.try_io(FirstReply::read).await? {
            FirstReply::Response(start) => start,
            FirstReply::Reset(_) => {
                self.state = None;
                return Ok(false)
            }
        };
        self.check_version(start.version())?;

        println!("Start serial update.");

        let mut target = self.store.start(false);
        loop {
            match pdu::Payload::read(&mut self.sock).await? {
                Ok(payload) => {
                    self.check_version(payload.version())?;
                    let (action, payload) = payload.into_payload();
                    target.push(action, payload);
                }
                Err(end) => {
                    self.check_version(end.version())?;
                    self.state = Some((end.session(), end.serial()));
                    if let Some(timing) = end.timing() {
                        self.timing = timing
                    }
                    break;
                }
            }
        }
        target.done(self.timing)?;
        Ok(true)
    }

    async fn reset(&mut self) -> Result<(), io::Error> {
        pdu::ResetQuery::new(
            self.version.unwrap_or(1)
        ).write(&mut self.sock).await?;
        let start = self.try_io(|sock| {
            pdu::CacheResponse::read(sock)
        }).await?;
        self.check_version(start.version())?;
        let mut target = self.store.start(true);
        loop {
            match pdu::Payload::read(&mut self.sock).await? {
                Ok(payload) => {
                    self.check_version(payload.version())?;
                    let (action, payload) = payload.into_payload();
                    target.push(action, payload);
                }
                Err(end) => {
                    self.check_version(end.version())?;
                    self.state = Some((end.session(), end.serial()));
                    if let Some(timing) = end.timing() {
                        self.timing = timing
                    }
                    break;
                }
            }
        }
        target.done(self.timing)?;
        Ok(())
    }
}


impl<Sock, Target> Client<Sock, Target> {
    async fn try_io<'a, F, Fut, T>(&'a mut self, op: F) -> Result<T, io::Error>
    where
        F: FnOnce(&'a mut Sock) -> Fut,
        Fut: Future<Output = Result<T, io::Error>> + 'a
    {
        match timeout(Duration::from_secs(1), op(&mut self.sock)).await {
            Ok(res) => res,
            Err(_) => {
                Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "server response timed out"
                ))
            }
        }
    }

    fn check_version(&mut self, version: u8) -> Result<(), io::Error> {
        if let Some(stored_version) = self.version {
            if version != stored_version {
                Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "version has changed"
                ))
            }
            else {
                Ok(())
            }
        }
        else {
            self.version = Some(version);
            Ok(())
        }
    }
}


//------------ FirstReply ----------------------------------------------------

enum FirstReply {
    Response(pdu::CacheResponse),
    Reset(pdu::CacheReset),
}

impl FirstReply {
    async fn read<Sock: AsyncRead + Unpin>(
        sock: &mut Sock
    ) -> Result<Self, io::Error> {
        let header = pdu::Header::read(sock).await?;
        match header.pdu() {
            pdu::CacheResponse::PDU => {
                pdu::CacheResponse::read_payload(
                    header, sock
                ).await.map(FirstReply::Response)
            }
            pdu::CacheReset::PDU => {
                pdu::CacheReset::read_payload(
                    header, sock
                ).await.map(FirstReply::Reset)
            }
            pdu::ERROR_PDU => {
                Err(io::Error::new(
                    io::ErrorKind::Other,
                    format!("server reported error {}", header.session())
                ))
            }
            pdu => {
                Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unexpected PDU {}", pdu)
                ))
            }
        }
    }
}

