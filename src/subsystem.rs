use std::{collections::HashSet, net::SocketAddr};

use futures::{stream::BoxStream, FutureExt};
use sci_rs::{scitds::OccupancyStatus, SCIMessageType, SCITelegram};
use tokio::sync::broadcast::{Receiver, Sender};
use tonic::{Request, Response, Status, Streaming};
use tracing::{event, info, Level};

use crate::{next_message, rasta::SciPacket, SubsystemConfig};

pub struct Subsystem {
    addr: SocketAddr,
    own_id: String,
    tds_id: String,
    outgoing_messages: Receiver<SCITelegram>,
    incoming_messages: Sender<SCITelegram>,
}

impl Subsystem {
    pub fn new(config: SubsystemConfig, outgoing_messages: Receiver<SCITelegram>) -> Self {
        Self {
            addr: config.ixl_address,
            own_id: config.ixl_id,
            tds_id: config.tds_id,
            outgoing_messages,
            incoming_messages: Sender::new(10),
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
        //let mut tvps = HashSet::new();

        //let own_id = self.own_id.clone();
        //let tds_id = self.tds_id.clone();
        let incoming_messages = self.incoming_messages.clone();
        let mut outgoing_messages = self.outgoing_messages.resubscribe();
        let output = async_stream::try_stream! {
            // This is the code for initialising the subsystem... Currently IXL commands are just passed through, but this might become necessary again later.

                    //yield SciPacket {message: SCITelegram::version_check(sci_rs::ProtocolType::SCIProtocolTDS, &own_id, &tds_id, 0x01).into()};
        //
                    //let version_response =  next_message(&mut req).await.unwrap();
                    //assert_eq!(version_response.message_type, SCIMessageType::pdi_version_response());
        //
                    //yield SciPacket {message: SCITelegram::initialisation_request(sci_rs::ProtocolType::SCIProtocolTDS, &own_id, &tds_id).into()};
        //
                    //let init_response =  next_message(&mut req).await.unwrap();
                    //assert_eq!(init_response.message_type, SCIMessageType::pdi_initialisation_response());
        //
                    //loop {
                    //    let next_msg = next_message(&mut req).await.unwrap();
                    //    if next_msg.message_type == SCIMessageType::scitds_tvps_occupancy_status() {
                    //        let occupancy = OccupancyStatus::try_from(next_msg.payload[0]).unwrap();
                    //        info!("{} : {:?}", next_msg.sender, occupancy);
                    //        tvps.insert(next_msg.sender.trim_end_matches('_').to_string());
                    //    } else if next_msg.message_type == SCIMessageType::pdi_initialisation_completed() {
                    //        info!("TDS reports initialisation complete");
                    //        break;
                    //    }
                    //}
        //
                    //info!("The following TVPS were reported: {tvps:?}");

                    // for tvps_ in tvps {
                    //     yield SciPacket { message: SCITelegram::fc(&own_id, &tvps_, sci_rs::scitds::FCMode::U).into() };
                    // }

                    loop {
                        futures::select! {
                            inc = next_message(&mut req).fuse() => {
                                let inc = inc.unwrap();
                                event!(Level::INFO, "{}", &inc);
                                if let Some(err) = incoming_messages.send(inc).err() {
                                    panic!("{err}");
                                }
                            }
                            out = outgoing_messages.recv().fuse() => {
                                let out = out.unwrap();
                                event!(Level::INFO, "{}", &out);
                                yield SciPacket { message: out.into() }
                            }
                        }
                    }
                };
        Ok(Response::new(Box::pin(output) as Self::StreamStream))
    }
}
