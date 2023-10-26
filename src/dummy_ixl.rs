use futures::stream::BoxStream;
use miette::IntoDiagnostic;
use rasta::{rasta_server::RastaServer, SciPacket};
use sci_rs::{scitds::OccupancyStatus, SCIMessageType, SCITelegram};

use figment::{
    providers::{Format, Toml},
    Figment,
};
use serde::Deserialize;
use std::collections::HashSet;
use tokio_stream::StreamExt;
use tonic::{transport::Server, Request, Response, Status, Streaming};
use tracing::info;

mod rasta {
    tonic::include_proto!("sci");
}

impl From<SCITelegram> for SciPacket {
    fn from(value: SCITelegram) -> Self {
        SciPacket {
            message: value.into(),
        }
    }
}

async fn next_message(messages: &mut Streaming<SciPacket>) -> miette::Result<SCITelegram> {
    messages
        .next()
        .await
        .unwrap()
        .into_diagnostic()?
        .message
        .as_slice()
        .try_into()
        .into_diagnostic()
}

struct TdsServer {
    own_id: String,
    oc_id: String,
}

impl TdsServer {
    pub fn from_config(config: Config) -> Self {
        Self {
            own_id: config.ixl_id,
            oc_id: config.tds_id,
        }
    }
}

#[tonic::async_trait]
impl rasta::rasta_server::Rasta for TdsServer {
    type StreamStream = BoxStream<'static, Result<SciPacket, Status>>;

    async fn stream(
        &self,
        request: Request<Streaming<SciPacket>>,
    ) -> Result<Response<Self::StreamStream>, Status> {
        info!("Incoming transmission");
        let mut req = request.into_inner();
        let mut tvps = HashSet::new();

        let own_id = self.own_id.clone();
        let oc_id = self.oc_id.clone();
        let output = async_stream::try_stream! {
            yield SciPacket {message: SCITelegram::version_check(sci_rs::ProtocolType::SCIProtocolTDS, &own_id, &oc_id, 0x01).into()};

            let version_response =  next_message(&mut req).await.unwrap();
            assert_eq!(version_response.message_type, SCIMessageType::pdi_version_response());

            yield SciPacket {message: SCITelegram::initialisation_request(sci_rs::ProtocolType::SCIProtocolTDS, &own_id, &oc_id).into()};

            let init_response =  next_message(&mut req).await.unwrap();
            assert_eq!(init_response.message_type, SCIMessageType::pdi_initialisation_response());

            loop {
                let next_msg = next_message(&mut req).await.unwrap();
                if next_msg.message_type == SCIMessageType::scitds_tvps_occupancy_status() {
                    let occupancy = OccupancyStatus::try_from(next_msg.payload[0]).unwrap();
                    info!("{} : {:?}", next_msg.sender, occupancy);
                    tvps.insert(next_msg.sender.trim_end_matches('_').to_string());
                } else if next_msg.message_type == SCIMessageType::pdi_initialisation_completed() {
                    info!("TDS reports initialisation complete");
                    break;
                }
            }

            info!("The following TVPS were reported: {tvps:?}");

            for tvps_ in tvps {
                yield SciPacket { message: SCITelegram::fc(&own_id, &tvps_, sci_rs::scitds::FCMode::U).into() };
            }

            loop {
                let next_msg = next_message(&mut req).await.unwrap();
                assert_eq!(next_msg.message_type, SCIMessageType::scitds_tvps_occupancy_status());
                info!("{}", next_msg);
                let occupancy = OccupancyStatus::try_from(next_msg.payload[0]).unwrap();
                info!("{} : {:?}", next_msg.sender, occupancy);
            }
        };
        Ok(Response::new(Box::pin(output) as Self::StreamStream))
    }
}

#[derive(Clone, Deserialize, Debug)]
struct Config {
    tds_id: String,
    ixl_id: String,
    ixl_address: String,
}

#[tokio::main]
async fn main() -> miette::Result<()> {
    let subscriber = tracing_subscriber::FmtSubscriber::builder()
        .pretty()
        .finish();

    tracing::subscriber::set_global_default(subscriber).into_diagnostic()?;

    let config: Config = Figment::new()
        .merge(Toml::file("tds.toml"))
        .extract()
        .into_diagnostic()?;

    let addr = config.ixl_address.parse().into_diagnostic()?;

    Server::builder()
        .add_service(RastaServer::new(TdsServer::from_config(config)))
        .serve(addr)
        .await
        .into_diagnostic()?;

    Ok(())
}
