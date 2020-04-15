//! The RTR server.
//!
//! This module implements a generic RTR server through [`Server`]. The server
//! receives its data from a type implementing [`VrpSource`].
//!
//! [`Server`]: struct.Server.html
//! [`VrpSource`]: trait.VrpSource.html
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
use crate::state::State;


//------------ VrpSource -----------------------------------------------------

/// A source of VRPs for an RTR server.
///
/// A type implementing this trait can be used by the [`Server`] as a source
/// for VRPs. The server needs four things from such a source:
///
/// *  the current state of the source through the [`notify`] method,
/// *  an iterator over the full set of VRPs via the [`full`] method,
/// *  an iterator over the difference of the data between the given state
///    and the current state via the [`diff`] method, and
/// *  the current timing values via the [`timing`] method.
///
/// The server will never ask for any of these things unless the [`ready`]
/// method returns `true`. This allows the source to finish its initial
/// validation.
///
/// [`ready`]: #method.ready
/// [`notify`]: #method.notify
/// [`full`]: #method.full
/// [`diff`]: #method.diff
/// [`timing`]: #method.timing
pub trait VrpSource: Clone + Sync + Send + 'static {
    /// An iterator over the complete set of VRPs.
    type FullIter: Iterator<Item = Payload> + Sync + Send + 'static;

    /// An iterator over a difference between two sets of VRPs.
    type DiffIter: Iterator<Item = (Action, Payload)>  + Sync + Send + 'static;

    /// Returns whether the source is ready to serve data.
    fn ready(&self) -> bool;

    /// Returns the current state of the source.
    ///
    /// This is used by the source when sending out a serial notify .
    fn notify(&self) -> State;

    /// Returns the current state and an iterator over the full set of VRPs.
    fn full(&self) -> (State, Self::FullIter);

    /// Returns the current state and an interator over differences in VPRs.
    ///
    /// The difference is between the state given in `state` and the current
    /// state. If the source cannot provide this difference, for instance
    /// because the serial is too old, it returns `None` instead.
    fn diff(&self, state: State) -> Option<(State, Self::DiffIter)>;

    /// Returns the timing information for the current state.
    fn timing(&self) -> pdu::Timing;
}


//------------ Server --------------------------------------------------------

/// An RTR server.
///
/// The server takes a stream socket listener – a stream of new sockets – and
/// a VRP source and serves RTR data. In order to also serve notifications
/// whenever new data is available, the server uses a notification dispatch
/// system via the [`Dispatch`] system.
///
/// [`Dispatch`]: struct.Dispatch.html
pub struct Server<Listener, Source> {
    /// The listener socket.
    listener: Listener,

    /// The dispatcher for notifications.
    dispatch: Dispatch,

    /// The source of VRPs.
    source: Source,
}

impl<Listener, Source> Server<Listener, Source> {
    /// Creates a new RTR server from its components.
    pub fn new(
        listener: Listener, dispatch: Dispatch, source: Source
    ) -> Self {
        Server { 
            listener, dispatch, source
        }
    }

    /// Runs the server.
    ///
    /// The asynchronous function will return successfully when the listener
    /// socket (which is a stream over new connectons) finishes. It will
    /// return with an error if the listener socket errors out.
    pub async fn run<Sock>(mut self) -> Result<(), io::Error>
    where
        Listener:
            Stream<Item = Result<Sock, io::Error>> + Unpin,
        Sock: AsyncRead + AsyncWrite + Unpin + Sync + Send + 'static,
        Source: VrpSource,
    {
        while let Some(sock) = self.listener.next().await {
            let _ = spawn(
                Connection::new(
                    sock?, self.dispatch.get_receiver(), self.source.clone()
                ).run()
            );
        }
        Ok(())
    }
}


//------------ Connection ----------------------------------------------------

/// A single server connection.
struct Connection<Sock, Source> {
    /// The socket to run the connection on.
    sock: Sock,

    /// The receiver for update notifications.
    notify: NotifyReceiver,

    /// The VRP source.
    source: Source,

    /// The RTR protocol version this connection is using.
    ///
    /// This will start out as `None` and will only be set once the client
    /// tells us its supported version.
    version: Option<u8>,
}

impl<Sock, Source> Connection<Sock, Source> {
    /// Wraps a socket into a connection value.
    fn new(sock: Sock, notify: NotifyReceiver, source: Source) -> Self {
        Connection {
            sock, notify, source,
            version: None,
        }
    }

    /// Returns the protocol version we agreed on.
    ///
    /// If there hasn’t been a negotation yet, returns the lowest protocol
    /// version we support, which currently is 0.
    fn version(&self) -> u8 {
        match self.version {
            Some(version) => version,
            None => 0
        }
    }
}

/// # High-level operation
///
impl<Sock, Source> Connection<Sock, Source>
where
    Sock: AsyncRead + AsyncWrite + Unpin + Sync + Send + 'static,
    Source: VrpSource
{
    /// Runs the connection until it is done.
    ///
    /// Returns successfully if the connection was closed cleanly. Returns an
    /// error if there was an error. However, those errors are basically
    /// ignored – this is only here for easy question mark use.
    async fn run(mut self) -> Result<(), io::Error> {
        while let Some(query) = self.recv().await? {
            match query {
                Query::Serial(state) => {
                    self.serial(state).await?
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
impl<Sock, Source> Connection<Sock, Source>
where Sock: AsyncRead + Unpin {
    /// Receives the next query.
    ///
    /// This can either be a notification that the source has updated data
    /// available or an actual query received from the client.
    ///
    /// It can also be an error if reading from the socket fails.
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
            pdu::SerialQuery::PDU => {
                debug!("RTR: Got serial query.");
                match Self::check_length(
                    header, pdu::SerialQuery::size()
                ) {
                    Ok(()) => {
                        let payload = pdu::SerialQueryPayload::read(
                            &mut self.sock
                        ).await?;
                        Ok(Some(Query::Serial(State::from_parts(
                            header.session(), payload.serial()
                        ))))
                    }
                    Err(err) => {
                        debug!("RTR: ... with bad length");
                        Ok(Some(err))
                    }
                }
            }
            pdu::ResetQuery::PDU => {
                debug!("RTR: Got reset query.");
                match Self::check_length(
                    header, pdu::ResetQuery::size()
                ) {
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

    /// Checks the version of a PDU-
    ///
    /// Returns an error with the error PDU if the version doesn’t match with
    /// what we agreed upon earlier.
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

    /// Checks that the size of a PDU matches an expected size.
    ///
    /// Returns an error response if not.
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
impl<Sock, Source> Connection<Sock, Source>
where
    Sock: AsyncWrite + Unpin + Sync + Send + 'static,
    Source: VrpSource
{
    /// Sends out a response to a serial query.
    ///
    /// The client’s current state is in `state`. Responds accordingly on
    /// whether the source is ready and there is or isn’t a diff for that
    /// state. Only returns an error when the socket goes kaputt.
    async fn serial(&mut self, state: State) -> Result<(), io::Error> {
        debug!("RTR server: request for serial {}", state.serial());
        if !self.source.ready() {
            return pdu::Error::new(
                self.version(), 2, (), *b"Running initial validation"
            ).write(&mut self.sock).await;
        }
        match self.source.diff(state) {
            Some((state, diff)) => {
                debug!("RTR server: source has a diff");
                pdu::CacheResponse::new(
                    self.version(), state,
                ).write(&mut self.sock).await?;
                for (action, payload) in diff {
                    pdu::Payload::new(
                        self.version(), action.into_flags(), payload
                    ).write(&mut self.sock).await?;
                }
                let timing = self.source.timing();
                pdu::EndOfData::new(
                    self.version(), state, timing
                ).write(&mut self.sock).await
            }
            None => {
                debug!("RTR server: source ain't got no diff for that.");
                pdu::CacheReset::new(self.version()).write(
                    &mut self.sock
                ).await
            }
        }
    }

    /// Sends out a sesponse to a reset query.
    ///
    /// Responds accordingly based on whether or not the source is ready.
    /// Only returns an error if writing to the socket fails.
    async fn reset(&mut self) -> Result<(), io::Error> {
        if !self.source.ready() {
            return pdu::Error::new(
                self.version(), 2, (), *b"Running initial validation"
            ).write(&mut self.sock).await;
        }
        let (state, iter) = self.source.full();
        pdu::CacheResponse::new(
            self.version(), state
        ).write(&mut self.sock).await?;
        for payload in iter {
            pdu::Payload::new(
                self.version(), Action::Announce.into_flags(), payload
            ).write(&mut self.sock).await?;
        }
        let timing = self.source.timing();
        pdu::EndOfData::new(
            self.version(), state, timing
        ).write(&mut self.sock).await
    }

    /// Sends an error response.
    async fn error(
        &mut self, err: pdu::BoxedError
    ) -> Result<(), io::Error> {
        err.write(&mut self.sock).await
    }

    /// Sends a serial notify query.
    ///
    /// The state for the notify is taken from the source.
    async fn notify(&mut self) -> Result<(), io::Error> {
        let state = self.source.notify();
        pdu::SerialNotify::new(
            self.version(), state
        ).write(&mut self.sock).await
    }
}


//------------ Query ---------------------------------------------------------

/// What a server was asked to do next.
enum Query {
    /// A serial query with the given state was received from the client.
    Serial(State),

    /// A reset query as received from the client.
    Reset,

    /// The client misbehaved resulting in this error to be sent to it.
    Error(pdu::BoxedError),

    /// The source has new data available.
    Notify
}


//------------ NotifySender --------------------------------------------------

/// A sender to notify a server that there are updates available.
#[derive(Clone, Debug)]
pub struct NotifySender {
    tx: Sender<Message>,
}

impl NotifySender {
    /// Notifies the server that there are updates available.
    pub fn notify(&mut self) {
        // Each sender gets one guaranteed message. Since we only ever send
        // notify messages, if we can’t queue a message, there’s already an
        // unprocessed notification and we are fine.
        let _ = self.tx.try_send(Message::Notify);
    }
}


//------------ NotifyReceiver ------------------------------------------------

/// The receiver for notifications.
///
/// This type is used by connections.
#[derive(Debug)]
struct NotifyReceiver {
    /// The receiving end of 
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

    fn get_receiver(&mut self) -> NotifyReceiver {
        let (tx, rx) = channel(1);
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
        let (tx, rx) = channel(1);
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

