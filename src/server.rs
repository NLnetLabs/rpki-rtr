use std::io;
use std::marker::Unpin;
use std::sync::{Arc, Mutex};
use futures::future;
use futures::future::Either;
use log::debug;
use pin_utils::pin_mut;
use slab::Slab;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::stream::{Stream, StreamExt};
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::sync::mpsc::error::TrySendError;
use tokio::task::spawn;
use crate::payload::{Action, Payload};
use crate::pdu;
use crate::serial::Serial;


//------------ VrpStore ------------------------------------------------------

pub trait VrpStore: Clone + Sync + Send + 'static {
    type FullIter: Iterator<Item = Payload> + Sync + Send + 'static;
    type DiffIter: Iterator<Item = (Action, Payload)>  + Sync + Send + 'static;

    fn notify(&self) -> (u16, Serial);
    fn full(&self) -> (u16, Serial, Self::FullIter);
    fn diff(
        &self, session: u16, serial: Serial
    ) -> Option<(u16, Serial, Self::DiffIter)>;
    fn timing(&self) -> Timing;
}


//------------ Server --------------------------------------------------------

pub struct Server<Listener, Store> {
    listener: Listener,
    dispatch: Dispatch,
    store: Store,
}

impl<Listener, Store> Server<Listener, Store> {
    pub fn new(
        listener: Listener, dispatch: Dispatch, store: Store
    ) -> Self {
        Server { 
            listener, dispatch, store
        }
    }

    pub async fn run<Sock>(mut self) -> Result<(), io::Error>
    where
        Listener:
            Stream<Item = Result<Sock, io::Error>> + Unpin + Send + 'static,
        Sock: AsyncRead + AsyncWrite + Unpin + Sync + Send + 'static,
        Store: VrpStore,
    {
        while let Some(sock) = self.listener.next().await {
            let _ = spawn(
                Connection::new(
                    sock?, self.dispatch.get_receiver(), self.store.clone()
                ).run()
            );
        }
        Ok(())
    }
}


//------------ Connection ----------------------------------------------------

struct Connection<Sock, Store> {
    sock: Sock,
    notify: NotifyReceiver,
    store: Store,
    version: Option<u8>,
}

impl<Sock, Store> Connection<Sock, Store> {
    fn new(sock: Sock, notify: NotifyReceiver, store: Store) -> Self {
        Connection {
            sock, notify, store,
            version: None,
        }
    }

    fn version(&self) -> u8 {
        match self.version {
            Some(version) => version,
            None => 0
        }
    }
}

/// # High-level operation
///
impl<Sock, Store> Connection<Sock, Store>
where
    Sock: AsyncRead + AsyncWrite + Unpin + Sync + Send + 'static,
    Store: VrpStore
{
    async fn run(mut self) -> Result<(), io::Error> {
        while let Some(query) = self.recv().await? {
            match query {
                Query::Serial { session, serial } => {
                    self.serial(session, serial).await?
                }
                Query::Reset => {
                    self.reset().await?
                }
                Query::Error(err) => {
                    self.error(err).await?
                }
                Query::Notify => {
                    self.notify().await?
                }
            }
        }
        Ok(())
    }
}


/// # Receiving
///
impl<Sock, Store> Connection<Sock, Store>
where Sock: AsyncRead + Unpin {
    async fn recv(&mut self) -> Result<Option<Query>, io::Error> {
        let header = {
            let notify = self.notify.recv();
            let header = pdu::Header::read(&mut self.sock);
            pin_mut!(notify);
            pin_mut!(header);
            match future::select(notify, header).await {
                Either::Left(_) => return Ok(Some(Query::Notify)),
                Either::Right((Ok(header), _)) => header,
                Either::Right((Err(err), _)) => {
                    if err.kind() == io::ErrorKind::UnexpectedEof {
                        return Ok(None)
                    }
                    else {
                        return Err(err)
                    }
                }
            }
        };
        if let Err(err) = self.check_version(header) {
            return Ok(Some(err))
        }
        match header.pdu() {
            pdu::SERIAL_QUERY_PDU => {
                debug!("RTR: Got serial query.");
                match Self::check_length(header, pdu::SERIAL_QUERY_LEN) {
                    Ok(()) => {
                        let payload = pdu::SerialQueryPayload::read(
                            &mut self.sock
                        ).await?;
                        Ok(Some(Query::Serial {
                            session: header.session(),
                            serial: payload.serial()
                        }))
                    }
                    Err(err) => {
                        debug!("RTR: ... with bad length");
                        Ok(Some(err))
                    }
                }
            }
            pdu::ResetQuery::PDU => {
                debug!("RTR: Got reset query.");
                match Self::check_length(header, pdu::ResetQuery::LEN) {
                    Ok(()) => Ok(Some(Query::Reset)),
                    Err(err) => {
                        debug!("RTR: ... with bad length");
                        Ok(Some(err))
                    }
                }
            }
            pdu => {
                debug!("RTR: Got query with PDU {}.", pdu);
                Ok(Some(Query::Error(
                    pdu::Error::new(
                        header.version(),
                        3,
                        header,
                        "expected Serial Query or Reset Query"
                    ).boxed()
                )))
            }
        }
    }

    fn check_version(
        &mut self,
        header: pdu::Header
    ) -> Result<(), Query> {
        if let Some(current) = self.version {
            if current != header.version() {
                Err(Query::Error(
                    pdu::Error::new(
                        header.version(),
                        8,
                        header,
                        "version switched during connection"
                    ).boxed()
                ))
            }
            else {
                Ok(())
            }
        }
        else if header.version() > 1 {
            Err(Query::Error(
                pdu::Error::new(
                    header.version(),
                    4,
                    header,
                    "only versions 0 and 1 supported"
                ).boxed()
            ))
        }
        else {
            self.version = Some(header.version());
            Ok(())
        }
    }

    fn check_length(header: pdu::Header, expected: u32) -> Result<(), Query> {
        if header.length() != expected {
            Err(Query::Error(
                pdu::Error::new(
                    header.version(),
                    3,
                    header,
                    "invalid length"
                ).boxed()
            ))
        }
        else {
            Ok(())
        }
    }
    
}

/// # Sending
///
impl<Sock, Store> Connection<Sock, Store>
where
    Sock: AsyncWrite + Unpin + Sync + Send + 'static,
    Store: VrpStore
{
    async fn serial(
        &mut self, session: u16, serial: Serial
    ) -> Result<(), io::Error> {
        match self.store.diff(session, serial) {
            Some((session, serial, diff)) => {
                pdu::CacheResponse::new(
                    self.version(), session
                ).write(&mut self.sock).await?;
                for (action, payload) in diff {
                    pdu::Payload::new(
                        self.version(), action.into_flags(), payload
                    ).write(&mut self.sock).await?;
                }
                let timing = self.store.timing();
                pdu::EndOfData::new(
                    self.version(), session, serial,
                    timing.refresh, timing.retry, timing.expire
                ).write(&mut self.sock).await
            }
            None => {
                pdu::CacheReset::new(self.version()).write(&mut self.sock).await
            }
        }
    }

    async fn reset(&mut self) -> Result<(), io::Error> {
        let (session, serial, iter) = self.store.full();
        pdu::CacheResponse::new(
            self.version(), session
        ).write(&mut self.sock).await?;
        for payload in iter {
            pdu::Payload::new(
                self.version(), Action::Announce.into_flags(), payload
            ).write(&mut self.sock).await?;
        }
        let timing = self.store.timing();
        pdu::EndOfData::new(
            self.version(), session, serial,
            timing.refresh, timing.retry, timing.expire
        ).write(&mut self.sock).await
    }

    async fn error(
        &mut self, err: pdu::BoxedError
    ) -> Result<(), io::Error> {
        err.write(&mut self.sock).await
    }

    async fn notify(&mut self) -> Result<(), io::Error> {
        let (session, serial) = self.store.notify();
        pdu::SerialNotify::new(
            self.version(), session, serial
        ).write(&mut self.sock).await
    }
}


//------------ Query ---------------------------------------------------------

pub enum Query {
    Serial {
        session: u16,
        serial: Serial,
    },
    Reset,
    Error(pdu::BoxedError),
    Notify
}


//------------ Timing --------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct Timing {
    pub refresh: u32,
    pub retry: u32,
    pub expire: u32
}


//------------ NotifySender --------------------------------------------------

#[derive(Clone, Debug)]
pub struct NotifySender {
    tx: Sender<Message>,
}

impl NotifySender {
    pub fn notify(&mut self) {
        // Each sender gets one guaranteed message. Since we only ever send
        // notify messages, if we can’t queue a message, there’s already an
        // unprocessed notification and we are fine.
        let _ = self.tx.try_send(Message::Notify);
    }
}


//------------ NotifyReceiver ------------------------------------------------

#[derive(Debug)]
pub struct NotifyReceiver {
    rx: Option<Receiver<()>>,
    tx: Sender<Message>,
    id: usize,
}

impl NotifyReceiver {
    pub async fn recv(&mut self)
    where Self: std::marker::Unpin {
        match self.rx {
            None => return future::pending().await,
            Some(ref mut rx) => {
                if let Some(()) = rx.next().await {
                    return
                }
            }
        }
        self.rx = None;
        future::pending().await
    }
}

impl Drop for NotifyReceiver {
    fn drop(&mut self) {
        let _ = self.tx.try_send(Message::Close(self.id));
    }
}


//------------ Dispatch ------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Dispatch(Arc<Mutex<DispatchInner>>);

#[derive(Clone, Debug)]
struct DispatchInner {
    connections: Slab<Sender<()>>,
    tx: Sender<Message>,
}

impl Dispatch {
    pub fn get_sender(&self) -> NotifySender {
        NotifySender {
            tx: self.0.lock().unwrap().tx.clone()
        }
    }

    pub fn get_receiver(&mut self) -> NotifyReceiver {
        let (tx, rx) = channel(0);
        let mut inner = self.0.lock().unwrap();
        NotifyReceiver {
            rx: Some(rx),
            tx: inner.tx.clone(),
            id: inner.connections.insert(tx),
        }
    }

    fn notify(&mut self) {
        self.0.lock().unwrap().connections.retain(|_, tx| {
            match tx.try_send(()) {
                Err(TrySendError::Closed(_)) => false,
                _ => true
            }
        })
    }

    fn close(&mut self, id: usize) {
        let _ = self.0.lock().unwrap().connections.remove(id);
    }
}


//------------ DispatchRunner ------------------------------------------------

pub struct DispatchRunner {
    dispatch: Dispatch,
    rx: Option<Receiver<Message>>,
}

impl DispatchRunner {
    pub fn new() -> Self {
        let (tx, rx) = channel(0);
        let dispatch = Dispatch(Arc::new(Mutex::new(
            DispatchInner {
                connections: Slab::new(),
                tx,
            }
        )));
        DispatchRunner {
            dispatch,
            rx: Some(rx)
        }
    }

    pub fn dispatch(&self) -> Dispatch {
        self.dispatch.clone()
    }

    pub async fn run(&mut self) {
        loop {
            let msg = match self.rx {
                None => return future::pending().await,
                Some(ref mut rx) => rx.next().await,
            };
            match msg {
                Some(Message::Notify) => self.dispatch.notify(),
                Some(Message::Close(id)) => self.dispatch.close(id),
                None => {
                    self.rx = None;
                }
            }
        }
    }
}


//------------ Message -------------------------------------------------------

#[derive(Clone, Copy, Debug)]
enum Message {
    // Send a new notification, please.
    Notify,

    // The connection with the given index is done.
    Close(usize),
}

