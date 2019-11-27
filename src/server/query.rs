//! Reading an RTR query from a client.

use std::io;
use std::marker::Unpin;
use futures::future;
use futures::future::Either;
use futures::io::AsyncRead;
use log::debug;
use pin_utils::pin_mut;
use crate::pdu;
use crate::serial::Serial;
use super::notify::NotifyReceiver;


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


//------------ QueryStream ---------------------------------------------------

pub struct QueryStream<A> {
    sock: A,
    notify: NotifyReceiver,
    version: Option<u8>
}

impl<A> QueryStream<A> {
    pub fn new(sock: A, notify: NotifyReceiver) -> Self {
        QueryStream { sock, notify, version: None }
    }

    pub fn version(&self) -> u8 {
        match self.version {
            Some(version) => version,
            None => 0
        }
    }
}

impl<A: AsyncRead + Unpin> QueryStream<A> {
    pub async fn recv(&mut self) -> Result<Option<Query>, io::Error> {
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

