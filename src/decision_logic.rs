use futures::stream::BoxStream;
use sci_rs::SCITelegram;
use tokio::sync::{
    broadcast::{Receiver, Sender},
    RwLock,
};
use tonic::transport::Server;

use crate::{
    decision_strategy::DecisionStrategy,
    rasta::rasta_server::RastaServer,
    subsystem::{ProtocolType, Subsystem},
    SubsystemConfig,
};
use std::{sync::Arc, time::Duration};
use tracing::{error, info};

#[derive(Copy, Clone, Debug)]
pub enum ActiveSubsystem {
    Trustworthy,
    Unreliable,
}

pub struct DecisionLogic<S: DecisionStrategy + Send> {
    trustworthy: Subsystem,
    unreliable: Subsystem,
    timeout: Duration,
    strategy: S,
    active: Arc<RwLock<ActiveSubsystem>>,
    outgoing_messages: Sender<SCITelegram>,
    trustworthy_incoming: Receiver<SCITelegram>,
    unreliable_incoming: Receiver<SCITelegram>,
}

impl<S: DecisionStrategy + 'static> DecisionLogic<S> {
    pub fn new(
        trustworthy_config: SubsystemConfig,
        unreliable_config: SubsystemConfig,
        timeout: Duration,
        strategy: S,
    ) -> Self {
        let (tx, rx) = tokio::sync::broadcast::channel(100);
        let trustworthy = Subsystem::new(trustworthy_config, rx, ProtocolType::NeuPro);
        let unreliable = Subsystem::new(unreliable_config, tx.subscribe(), ProtocolType::EULYNX);
        let trustworthy_incoming = trustworthy.incoming_messages();
        let unreliable_incoming = unreliable.incoming_messages();
        Self {
            trustworthy,
            unreliable,
            timeout,
            strategy,
            active: Arc::new(RwLock::new(S::INITIAL_SUBSYSTEM)),
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
        let strategy = self.strategy;
        let stream = async_stream::stream! {
                loop {
                    let (t, u) = futures::join!(tokio::time::timeout(timeout, trustworthy_incoming.recv()), tokio::time::timeout(timeout, unreliable_incoming.recv()));

                    let t = t.ok().map(Result::unwrap);
                    let u = u.ok().map(Result::unwrap);

                    if t.is_none() && u.is_none() {
                        continue;
                    }

                    if let Some(switch) = strategy.switch_to(&t, &u) {
                        error!("Decision Module requires switch to {switch:?} subsystem.");
                        *active.write().await = switch;
                        std::process::exit(0);
                    }

                    match *active.read().await {
                        ActiveSubsystem::Trustworthy => {
                            match t {
                                Some(t) => yield t,
                                None => {
                                    panic!("The trustworthy controller timed out");
                                }
                            }
                        }
                        ActiveSubsystem::Unreliable => {
                            match u {
                                Some(u) => yield u,
                                None => {
                                    panic!("The unreliable controller timed out");
                                }
                            }
                        }
                    }
                }

        };
        Box::pin(stream) as BoxStream<'static, SCITelegram>
    }
}
