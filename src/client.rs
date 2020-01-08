use std::io;
use std::marker::Unpin;
use log::debug;
use futures::future::Future;
use tokio::time::{Duration, timeout};
use tokio::io::{AsyncRead, AsyncWrite};
use crate::payload::{Action, Payload};
use crate::pdu;
use crate::serial::Serial;


//------------ VrpStore ------------------------------------------------------

pub trait VrpStore {
    fn start(&mut self, reset: bool);
    fn push(&mut self, action: Action, payload: Payload);
    fn done(&mut self);
}


//------------ Client --------------------------------------------------------

pub struct Client<Sock, Store> {
    sock: Sock,
    store: Store,
    state: Option<(u16, Serial)>,
    version: Option<u8>,
    timing: pdu::Timing,
}

impl<Sock, Store> Client<Sock, Store> {
    pub fn new(
        sock: Sock,
        store: Store,
        state: Option<(u16, Serial)>
    ) -> Self {
        Client {
            sock, store, state,
            version: None,
            timing: pdu::Timing::default(),
        }
    }
}

impl<Sock, Store> Client<Sock, Store>
where
    Sock: AsyncRead + AsyncWrite + Unpin,
    Store: VrpStore
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
                    self.serial(session, serial).await?;
                }
                None => {
                    self.reset().await?;
                }
            }
        }
    }

    async fn serial(
        &mut self, session: u16, serial: Serial
    ) -> Result<(), io::Error> {
        pdu::SerialQuery::new(
            self.version.unwrap_or(1), session, serial
        ).write(&mut self.sock).await?;
        let start = match self.try_io(|sock| {
            pdu::CacheResponse::try_read(sock)
        }).await? {
            Ok(start) => start,
            Err(err) => {
                debug!("Got error {} from server", err.session());
                return Err(io::Error::new(
                    io::ErrorKind::Other, "server reported error"
                ))
            }
        };
        self.check_version(start.version())?;

        self.store.start(false);
        loop {
            match pdu::Payload::read(&mut self.sock).await? {
                Ok(payload) => {
                    self.check_version(payload.version())?;
                    let (action, payload) = payload.into_payload();
                    self.store.push(action, payload);
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
        Ok(())
    }

    async fn reset(&mut self) -> Result<(), io::Error> {
        pdu::ResetQuery::new(
            self.version.unwrap_or(1)
        ).write(&mut self.sock).await?;
        let start = self.try_io(|sock| {
            pdu::CacheResponse::try_read(sock)
        }).await?;
        let start = match start {
            Ok(start) => start,
            Err(err) => {
                // If we get an Unsupported Version error and haven’t
                // negotiated a version yet, we fall back to version 0 try
                // reset again.
                if err.session() == 4 && self.version.is_none() {
                    self.version = Some(0);
                    // We can’t recurse without boxing. 
                    pdu::ResetQuery::new(
                        self.version.unwrap_or(1)
                    ).write(&mut self.sock).await?;
                    match self.try_io(|sock| {
                        pdu::CacheResponse::try_read(sock)
                    }).await? {
                        Ok(start) => start,
                        Err(err) => {
                            debug!("Got error {} from server", err.session());
                            return Err(io::Error::new(
                                io::ErrorKind::Other, "server reported error"
                            ))
                        }
                    }
                }
                else {
                    debug!("Got error {} from server", err.session());
                    return Err(io::Error::new(
                        io::ErrorKind::Other, "server reported error"
                    ))
                }
            }
        };
        self.check_version(start.version())?;

        self.store.start(true);
        loop {
            match pdu::Payload::read(&mut self.sock).await? {
                Ok(payload) => {
                    self.check_version(payload.version())?;
                    let (action, payload) = payload.into_payload();
                    self.store.push(action, payload);
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
        Ok(())
    }
}


impl<Sock, Store> Client<Sock, Store> {
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

