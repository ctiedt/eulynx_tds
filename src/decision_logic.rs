use futures::stream::BoxStream;
use sci_rs::SCITelegram;
use tokio::sync::broadcast::{Receiver, Sender};
use tonic::transport::Server;

use crate::{rasta::rasta_server::RastaServer, subsystem::Subsystem, SubsystemConfig};

#[derive(Clone, Copy)]
pub enum ActiveSubsystem {
    Trustworthy,
    Unreliable,
}

pub struct DecisionLogic {
    trustworthy: Subsystem,
    unreliable: Subsystem,
    active: ActiveSubsystem,
    outgoing_messages: Sender<SCITelegram>,
    trustworthy_incoming: Receiver<SCITelegram>,
    unreliable_incoming: Receiver<SCITelegram>,
}

impl DecisionLogic {
    pub fn new(trustworthy_config: SubsystemConfig, unreliable_config: SubsystemConfig) -> Self {
        let (tx, rx) = tokio::sync::broadcast::channel(10);
        let trustworthy = Subsystem::new(trustworthy_config, rx);
        let unreliable = Subsystem::new(unreliable_config, tx.subscribe());
        let trustworthy_incoming = trustworthy.incoming_messages();
        let unreliable_incoming = unreliable.incoming_messages();
        Self {
            trustworthy,
            unreliable,
            active: ActiveSubsystem::Unreliable,
            outgoing_messages: tx,
            trustworthy_incoming,
            unreliable_incoming,
        }
    }

    pub fn active(&self) -> ActiveSubsystem {
        self.active
    }

    pub fn send_message(&self, message: SCITelegram) {
        let _ = self.outgoing_messages.send(message);
    }

    pub fn incoming_messages<'a>(&'a self) -> BoxStream<'a, SCITelegram> {
        let mut trustworthy_incoming = self.trustworthy_incoming.resubscribe();
        let mut unreliable_incoming = self.unreliable_incoming.resubscribe();
        let stream = async_stream::stream! {
            loop {
                let (t, u) = futures::join!(trustworthy_incoming.recv(), unreliable_incoming.recv());
                match self.active {
                    ActiveSubsystem::Trustworthy => {
                        yield t.unwrap();
                    }
                    ActiveSubsystem::Unreliable => {
                        yield u.unwrap();
                    }
                }
            }
        };
        Box::pin(stream)
    }

    pub async fn run(self) {
        let unreliable_addr = self.unreliable.addr();
        let trustworthy_addr = self.trustworthy.addr();
        let unreliable_server = Server::builder()
            .add_service(RastaServer::new(self.unreliable))
            .serve(unreliable_addr);
        let trustworthy_server = Server::builder()
            .add_service(RastaServer::new(self.trustworthy))
            .serve(trustworthy_addr);
        let _ = futures::join!(unreliable_server, trustworthy_server);
    }
}
