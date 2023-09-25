use futures::stream::BoxStream;
use miette::IntoDiagnostic;
use rasta::{rasta_server::RastaServer, SciPacket};
use sci_rs::{scitds::OccupancyStatus, SCIMessageType, SCITelegram};

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
        .expect("Awaiting version check command")
        .into_diagnostic()?
        .message
        .as_slice()
        .try_into()
        .into_diagnostic()
}

struct TdsServer;

#[tonic::async_trait]
impl rasta::rasta_server::Rasta for TdsServer {
    type StreamStream = BoxStream<'static, Result<SciPacket, Status>>;

    async fn stream(
        &self,
        request: Request<Streaming<SciPacket>>,
    ) -> Result<Response<Self::StreamStream>, Status> {
        info!("Incoming transmission");
        let mut req = request.into_inner();

        let output = async_stream::try_stream! {
            yield SciPacket {message: SCITelegram::version_check(sci_rs::ProtocolType::SCIProtocolTDS, "ixl", "tds01", 0x01).into()};

            let version_response =  next_message(&mut req).await.unwrap();
            assert_eq!(version_response.message_type, SCIMessageType::pdi_version_response());

            yield SciPacket {message: SCITelegram::initialisation_request(sci_rs::ProtocolType::SCIProtocolTDS, "ixl", "tds01").into()};

            let init_response =  next_message(&mut req).await.unwrap();
            assert_eq!(init_response.message_type, SCIMessageType::pdi_initialisation_response());

            loop {
                let next_msg = next_message(&mut req).await.unwrap();
                if next_msg.message_type == SCIMessageType::scitds_tvps_occupancy_status() {
                    let occupancy = OccupancyStatus::try_from(next_msg.payload[0]).unwrap();
                    info!("{} : {:?}", next_msg.sender, occupancy);
                } else if next_msg.message_type == SCIMessageType::pdi_initialisation_completed() {
                    info!("TDS reports initialisation complete");
                    break;
                }
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

#[tokio::main]
async fn main() -> miette::Result<()> {
    let subscriber = tracing_subscriber::FmtSubscriber::builder()
        .pretty()
        .finish();

    tracing::subscriber::set_global_default(subscriber).into_diagnostic()?;

    let addr = "127.0.0.1:8001".parse().into_diagnostic()?;

    Server::builder()
        .add_service(RastaServer::new(TdsServer))
        .serve(addr)
        .await
        .into_diagnostic()?;

    Ok(())
}
