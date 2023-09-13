use std::marker::PhantomData;

use figment::{
    providers::{Format, Toml},
    Figment,
};
use miette::IntoDiagnostic;
use rasta::{rasta_client::RastaClient, SciPacket};
use sci_rs::{SCIMessageType, SCITelegram};
use serde_derive::Deserialize;
use tds_state::{Initialising, Operational};
use tokio_stream::{wrappers::BroadcastStream, StreamExt};
use tonic::Streaming;
use tracing::info;

const SCI_TDS_VERSION: u8 = 0x03;

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

macro_rules! type_states {
    ($mod_name:ident, $s_trait:ident, $($state:ident),*) => {
        mod $mod_name {
            pub trait $s_trait {}
        $(
            pub struct $state;
            impl $s_trait for $state {}
        )*
    }
    };
}

type_states!(tds_state, TdsState, Initialising, Operational);

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

struct AxleCounter<State: tds_state::TdsState> {
    id: String,
    connection: RastaClient<tonic::transport::Channel>,
    outgoing_messages: tokio::sync::broadcast::Sender<SciPacket>,
    _state: PhantomData<State>,
}

impl<T: tds_state::TdsState> AxleCounter<T> {
    fn send(&self, message: SCITelegram) -> miette::Result<()> {
        self.outgoing_messages
            .send(message.into())
            .into_diagnostic()?;
        Ok(())
    }
}

impl AxleCounter<Initialising> {
    pub async fn from_config(config: Config) -> miette::Result<Self> {
        info!(config.tds_id, "Setting up Axle Counter");
        let connection = RastaClient::connect(config.ixl_address)
            .await
            .into_diagnostic()?;
        let (tx, _) = tokio::sync::broadcast::channel(10);
        Ok(Self {
            id: config.tds_id,
            connection,
            outgoing_messages: tx,
            _state: PhantomData,
        })
    }

    pub async fn init(mut self) -> miette::Result<AxleCounter<Operational>> {
        let outgoing_stream =
            BroadcastStream::new(self.outgoing_messages.subscribe()).map(|m| m.unwrap());
        let mut incoming_messages = self
            .connection
            .stream(outgoing_stream)
            .await
            .into_diagnostic()?
            .into_inner();

        let telegram: SCITelegram = next_message(&mut incoming_messages).await?;

        assert_eq!(telegram.message_type, SCIMessageType::pdi_version_check());

        self.send(SCITelegram::version_response(
            telegram.protocol_type,
            &self.id,
            &telegram.sender,
            SCI_TDS_VERSION,
            if telegram.payload[0] == SCI_TDS_VERSION {
                sci_rs::SCIVersionCheckResult::VersionsAreEqual
            } else {
                sci_rs::SCIVersionCheckResult::VersionsAreNotEqual
            },
            // There should be a checksum here, have to figure out which kind...
            &[],
        ))?;

        let telegram: SCITelegram = next_message(&mut incoming_messages).await?;

        assert_eq!(
            telegram.message_type,
            SCIMessageType::pdi_initialisation_request()
        );

        self.send(SCITelegram::initialisation_response(
            telegram.protocol_type,
            &self.id,
            &telegram.sender,
        ))?;

        // Status Report

        self.send(SCITelegram::tvps_occupancy_status(
            &self.id,
            &telegram.sender,
            sci_rs::scitds::OccupancyStatus::Disturbed,
            true,
            0,
            sci_rs::scitds::POMStatus::Ok,
            sci_rs::scitds::DisturbanceStatus::Operational,
            sci_rs::scitds::ChangeTrigger::InitialSectionState,
        ))?;

        self.send(SCITelegram::tdp_status(
            &self.id,
            &telegram.sender,
            sci_rs::scitds::StateOfPassing::NotPassed,
            sci_rs::scitds::DirectionOfPassing::WithoutIndicatedDirection,
        ))?;

        // Status Report done

        self.send(SCITelegram::initialisation_completed(
            telegram.protocol_type,
            &self.id,
            &telegram.sender,
        ))?;

        Ok(AxleCounter {
            id: self.id,
            connection: self.connection,
            outgoing_messages: self.outgoing_messages,
            _state: PhantomData,
        })
    }
}

#[derive(Clone, Deserialize)]
struct Config {
    tds_id: String,
    tds_address: String,
    ixl_address: String,
}

#[tokio::main]
async fn main() -> miette::Result<()> {
    let config = Figment::new()
        .merge(Toml::file("tds.toml"))
        .extract()
        .into_diagnostic()?;

    let subscriber = tracing_subscriber::FmtSubscriber::builder()
        .pretty()
        .finish();

    tracing::subscriber::set_global_default(subscriber).into_diagnostic()?;

    let axle_counter = AxleCounter::from_config(config).await?;
    let axle_counter = axle_counter.init().await?;
    Ok(())
}
