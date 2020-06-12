use std::io;
use std::collections::HashSet;
use rpki_rtr::payload::{Action, Payload, Timing};
use rpki_rtr::client::{Client, VrpError, VrpTarget};
use tokio::net::TcpStream;

#[derive(Clone, Default)]
pub struct Store {
    payload: HashSet<Payload>,
}

impl VrpTarget for Store {
    type Update = Vec<(Action, Payload)>;

    fn start(&mut self, _: bool) -> Self::Update {
        Vec::new()
    }

    fn apply(
        &mut self, update: Self::Update, reset: bool, _timing: Timing
    ) -> Result<(), VrpError> {
        if reset {
            self.payload.clear();
        }
        for (action, payload)in update {
            match action {
                Action::Announce => {
                    println!("Adding {:?}", payload);
                    let _ = self.payload.insert(payload);
                }
                Action::Withdraw => {
                    println!("Removing {:?}", payload);
                    let _ = self.payload.remove(&payload);
                }
            }
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), io::Error> {
    let sock = TcpStream::connect("127.0.0.1:3323").await?;
    let mut client = Client::new(sock, Store::default(), None);
    client.run().await
}
