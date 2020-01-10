use std::io;
use std::collections::HashSet;
use rpki_rtr::payload::{Action, Payload};
use rpki_rtr::client::{Client, VrpStore};
use tokio::net::TcpStream;

#[derive(Default)]
pub struct Store {
    payload: HashSet<Payload>,
}

impl VrpStore for Store {
    fn start(&mut self, reset: bool) {
        if reset {
            self.payload.clear();
        }
    }

    fn push(&mut self, action: Action, payload: Payload) {
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

    fn done(&mut self) {
        println!("Complete.");
    }
}

#[tokio::main]
async fn main() -> Result<(), io::Error> {
    let sock = TcpStream::connect("127.0.0.1:3323").await?;
    let mut client = Client::new(sock, Store::default(), None);
    client.run().await
}
