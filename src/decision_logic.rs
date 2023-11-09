use futures::stream::BoxStream;
use sci_rs::SCITelegram;
use tokio::sync::{
    broadcast::{Receiver, Sender},
    RwLock,
};
use tonic::transport::Server;

use crate::{rasta::rasta_server::RastaServer, subsystem::Subsystem, SubsystemConfig};
use std::{sync::Arc, time::Duration};
use tracing::{info, warn};

pub trait DecisionStrategy {}

pub enum ActiveSubsystem {
    Trustworthy,
    Unreliable,
}

pub struct DecisionLogic {
    trustworthy: Subsystem,
    unreliable: Subsystem,
    timeout: Duration,
    active: Arc<RwLock<ActiveSubsystem>>,
    outgoing_messages: Sender<SCITelegram>,
    trustworthy_incoming: Receiver<SCITelegram>,
    unreliable_incoming: Receiver<SCITelegram>,
}

impl DecisionLogic {
    pub fn new(
        trustworthy_config: SubsystemConfig,
        unreliable_config: SubsystemConfig,
        timeout: Duration,
    ) -> Self {
        let (tx, rx) = tokio::sync::broadcast::channel(100);
        let trustworthy = Subsystem::new(trustworthy_config, rx);
        let unreliable = Subsystem::new(unreliable_config, tx.subscribe());
        let trustworthy_incoming = trustworthy.incoming_messages();
        let unreliable_incoming = unreliable.incoming_messages();
        Self {
            trustworthy,
            unreliable,
            timeout,
            active: Arc::new(RwLock::new(ActiveSubsystem::Unreliable)),
            outgoing_messages: tx,
            trustworthy_incoming,
            unreliable_incoming,
        }
    }

    pub fn get_sender(&self) -> Sender<SCITelegram> {
        self.outgoing_messages.clone()
    }

    pub fn active(&self) -> Arc<RwLock<ActiveSubsystem>> {
        self.active.clone()
    }

    pub async fn run(self) -> BoxStream<'static, SCITelegram> {
        let unreliable_addr = self.unreliable.addr();
        let trustworthy_addr = self.trustworthy.addr();
        let unreliable_server = Server::builder()
            .add_service(RastaServer::new(self.unreliable))
            .serve(unreliable_addr);
        let trustworthy_server = Server::builder()
            .add_service(RastaServer::new(self.trustworthy))
            .serve(trustworthy_addr);
        info!(
            "Running trustworthy on {} and unreliable on {}",
            trustworthy_addr, unreliable_addr
        );

        tokio::spawn(unreliable_server);
        tokio::spawn(trustworthy_server);

        let mut trustworthy_incoming = self.trustworthy_incoming.resubscribe();
        let mut unreliable_incoming = self.unreliable_incoming.resubscribe();
        let active = self.active.clone();
        let timeout = self.timeout;
        let stream = async_stream::stream! {
                loop {
                    let (t, u) = futures::join!(tokio::time::timeout(timeout, trustworthy_incoming.recv()), tokio::time::timeout(timeout, unreliable_incoming.recv()));

                    // This means both timed out, i.e. no new message
                    if t.is_err() && u.is_err() {
                        continue;
                    }

                    if t.is_ok() && u.is_err() {
                        *active.write().await = ActiveSubsystem::Trustworthy;
                        warn!("Switching to trustworthy subsystem");
                    }

                    match *active.read().await {
                        ActiveSubsystem::Trustworthy => {
                            match t {
                                Ok(t) => yield t.unwrap(),
                                Err(_) => {
                                    panic!("The trustworthy controller timed out");
                                }
                            }
                        }
                        ActiveSubsystem::Unreliable => {
                            match u {
                                Ok(u) => yield u.unwrap(),
                                Err(_) => {
                                    match t {
                                        Ok(t) => yield t.unwrap(),
                                        Err(_) => {
                                            panic!("The trustworthy controller timed out");
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

        };
        Box::pin(stream) as BoxStream<'static, SCITelegram>
    }
}
