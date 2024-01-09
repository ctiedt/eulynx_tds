use std::net::SocketAddr;

use futures::{stream::BoxStream, FutureExt};
use sci_rs::{
    scitds::{NeuProOccupancyStatusPayload, OccupancyStatusPayload},
    SCIMessageType, SCITelegram,
};
use tokio::sync::broadcast::{Receiver, Sender};
use tonic::{Request, Response, Status, Streaming};
use tracing::{event, info, Level};

use crate::{next_message, rasta::SciPacket, SubsystemConfig};

#[derive(Clone, Copy, Debug)]
pub enum ProtocolType {
    EULYNX,
    NeuPro,
}

fn neupro_to_eulynx(mut message: SCITelegram) -> SCITelegram {
    if message.message_type == SCIMessageType::scitds_tvps_occupancy_status() {
        let payload = NeuProOccupancyStatusPayload::try_from(message.payload).unwrap();
        let new_payload = OccupancyStatusPayload::from(payload);
        message.payload = new_payload.into();
    }
    message
}

pub struct Subsystem {
    addr: SocketAddr,
    outgoing_messages: Receiver<SCITelegram>,
    incoming_messages: Sender<SCITelegram>,
    protocol_type: ProtocolType,
}

impl Subsystem {
    pub fn new(
        config: SubsystemConfig,
        outgoing_messages: Receiver<SCITelegram>,
        protocol_type: ProtocolType,
    ) -> Self {
        Self {
            addr: config.ixl_address,
            outgoing_messages,
            incoming_messages: Sender::new(10),
            protocol_type,
        }
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn incoming_messages(&self) -> Receiver<SCITelegram> {
        self.incoming_messages.subscribe()
    }
}

#[tonic::async_trait]
impl crate::rasta::rasta_server::Rasta for Subsystem {
    type StreamStream = BoxStream<'static, Result<SciPacket, Status>>;

    async fn stream(
        &self,
        request: Request<Streaming<SciPacket>>,
    ) -> Result<Response<Self::StreamStream>, Status> {
        info!("Incoming transmission");
        let mut req = request.into_inner();

        let incoming_messages = self.incoming_messages.clone();
        let mut outgoing_messages = self.outgoing_messages.resubscribe();
        let protocol_type = self.protocol_type;
        let output = async_stream::try_stream! {

            loop {
                futures::select! {
                    inc = next_message(&mut req).fuse() => {
                        let mut inc = inc.unwrap();
                        match protocol_type {
                            ProtocolType::EULYNX => {},
                            ProtocolType::NeuPro => {inc = neupro_to_eulynx(inc);},
                        }
                        event!(Level::INFO, "{:?} {} ({})", protocol_type, &inc, inc.payload.len());
                        if let Some(err) = incoming_messages.send(inc).err() {
                            panic!("{err}");
                        }
                    }
                    out = outgoing_messages.recv().fuse() => {
                        let out = out.unwrap();
                        event!(Level::INFO, "{:?} {}", protocol_type, &out);
                        yield SciPacket { message: out.into() }
                    }
                }
            }
        };
        Ok(Response::new(Box::pin(output) as Self::StreamStream))
    }
}
