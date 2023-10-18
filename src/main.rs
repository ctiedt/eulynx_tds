pub mod neupro_connection;
mod tvps;

use std::{marker::PhantomData, time::Duration};

use figment::{
    providers::{Format, Toml},
    Figment,
};

use miette::IntoDiagnostic;
use rasta::{rasta_client::RastaClient, SciPacket};
use sci_rs::{SCIMessageType, SCITelegram};
use serde::{Deserialize, Deserializer};
use serde_derive::Deserialize;
use tds_state::{Initialising, Operational};
use tokio_stream::{wrappers::BroadcastStream, StreamExt};
use tonic::Streaming;
use tracing::info;

use crate::tvps::Tvps;

const SCI_TDS_VERSION: u8 = 0x01;

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
    tvps: Vec<tvps::Tvps>,
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
        let ixl_address = config.ixl_address;
        let connection = RastaClient::connect(ixl_address.clone())
            .await
            .into_diagnostic()?;
        let (tx, _) = tokio::sync::broadcast::channel(10);
        let tvps = config
            .tvps
            .into_iter()
            .map(|cfg| futures::executor::block_on(Tvps::new(cfg, &ixl_address)).unwrap())
            .collect();
        Ok(Self {
            id: config.tds_id,
            connection,
            outgoing_messages: tx,
            tvps,
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

        for tvps in &self.tvps {
            self.send(SCITelegram::tvps_occupancy_status(
                tvps.id(),
                &telegram.sender,
                sci_rs::scitds::OccupancyStatus::Disturbed,
                true,
                0,
                sci_rs::scitds::POMStatus::Ok,
                sci_rs::scitds::DisturbanceStatus::Operational,
                sci_rs::scitds::ChangeTrigger::InitialSectionState,
            ))?;
        }

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

        info!("Initialisation complete");

        Ok(AxleCounter {
            id: self.id,
            connection: self.connection,
            outgoing_messages: self.outgoing_messages,
            tvps: self.tvps,
            _state: PhantomData,
        })
    }
}

impl AxleCounter<Operational> {
    pub async fn run(&mut self) -> miette::Result<()> {
        for tvps in &mut self.tvps {
            tvps.wait_for_passing_axle(self.outgoing_messages.clone())
                .await?;
        }
        Ok(())
    }
}

fn deserialize_timer_value<'de, D>(d: D) -> Result<Duration, D::Error>
where
    D: Deserializer<'de>,
{
    let timer = u64::deserialize(d)?;

    Ok(Duration::from_millis(timer))
}

#[derive(Clone, Deserialize, Debug)]
pub struct TvpsConfig {
    name: String,
    address: String,
}

#[derive(Clone, Deserialize, Debug)]
struct Config {
    tds_id: String,
    ixl_address: String,
    tvps: Vec<TvpsConfig>,
    #[serde(deserialize_with = "deserialize_timer_value")]
    con_t_inhibition_timer: Duration,
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

    let axle_counter = AxleCounter::from_config(config).await?;
    let mut axle_counter = axle_counter.init().await?;
    axle_counter.run().await?;
    Ok(())
}
