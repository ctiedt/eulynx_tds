mod decision_logic;
mod decision_strategy;
mod subsystem;

use std::{net::SocketAddr, time::Duration};

use crate::decision_strategy::{AlwaysTrustworthy, AlwaysUnreliable, ManualSwitch, TryUnreliable};
use decision_logic::{ActiveSubsystem, DecisionLogic};
use decision_strategy::DecisionStrategy;
use figment::{
    providers::{Format, Toml},
    Figment,
};
use futures::StreamExt;
use miette::IntoDiagnostic;
use rasta::{rasta_client::RastaClient, SciPacket};
use sci_rs::SCITelegram;
use serde::{Deserialize, Deserializer};
use tokio::sync::broadcast::Sender;
use tonic::{Request, Streaming};
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

pub async fn next_message(messages: &mut Streaming<SciPacket>) -> miette::Result<SCITelegram> {
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

pub struct AxleCounter<S: DecisionStrategy + 'static> {
    rasta_id: String,
    ixl_address: String,
    decision_logic: DecisionLogic<S>,
    sender: Sender<SCITelegram>,
    ixl_delay: Duration,
}

impl<S: DecisionStrategy> AxleCounter<S> {
    pub fn from_config(config: Config, strategy: S) -> AxleCounter<S> {
        let decision_logic = DecisionLogic::new(
            config.trustworthy,
            config.unreliable,
            config.timeout,
            strategy,
        );
        let sender = decision_logic.get_sender();
        Self {
            rasta_id: config.rasta_id,
            ixl_address: config.ixl_address,
            decision_logic,
            sender,
            ixl_delay: config.ixl_delay,
        }
    }

    pub async fn run(self) -> miette::Result<()> {
        let stream = self.decision_logic.run().await;

        tokio::time::sleep(self.ixl_delay).await;

        let mut client = RastaClient::connect(self.ixl_address)
            .await
            .into_diagnostic()?;
        let mut req = Request::new(stream.map(SciPacket::from));
        req.metadata_mut()
            .insert("rasta-id", self.rasta_id.parse().into_diagnostic()?);
        let mut incoming = client.stream(req).await.into_diagnostic()?.into_inner();

        while let Some(msg) = incoming.next().await {
            let msg = msg.into_diagnostic()?;
            let sci_msg = SCITelegram::try_from(msg.message.as_slice()).into_diagnostic()?;
            info!("Message from interlocking: {}", &sci_msg);
            if let Some(err) = self.sender.send(sci_msg).err() {
                panic!("{err}");
            }
        }

        info!("Closing Object Controller (IXL disconnected)");
        Ok(())
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct Config {
    ixl_address: String,
    rasta_id: String,
    #[serde(deserialize_with = "deserialize_timer_value")]
    ixl_delay: Duration,
    #[serde(deserialize_with = "deserialize_timer_value")]
    timeout: Duration,
    strategy: SelectedStrategy,
    trustworthy: SubsystemConfig,
    unreliable: SubsystemConfig,
}

#[derive(Clone, Debug, Deserialize)]
pub enum SelectedStrategy {
    AlwaysTrustworthy,
    AlwaysUnreliable,
    ManualSwitch,
    TryUnreliable,
}

#[derive(Deserialize, Clone, Debug)]
pub struct SubsystemConfig {
    ixl_address: SocketAddr,
}

fn deserialize_timer_value<'de, D>(d: D) -> Result<Duration, D::Error>
where
    D: Deserializer<'de>,
{
    let timer = u64::deserialize(d)?;

    Ok(Duration::from_millis(timer))
}

#[tokio::main]
async fn main() -> miette::Result<()> {
    let subscriber = tracing_subscriber::FmtSubscriber::builder()
        .pretty()
        .finish();

    tracing::subscriber::set_global_default(subscriber).into_diagnostic()?;

    let config: Config = Figment::new()
        .join(Toml::file("tds.toml"))
        .extract()
        .into_diagnostic()?;
    info!("{:?}", &config);

    match config.strategy {
        SelectedStrategy::AlwaysTrustworthy => {
            let axle_counter = AxleCounter::from_config(config, AlwaysTrustworthy);
            axle_counter.run().await?;
        }
        SelectedStrategy::AlwaysUnreliable => {
            let axle_counter = AxleCounter::from_config(config, AlwaysUnreliable);
            axle_counter.run().await?;
        }
        SelectedStrategy::ManualSwitch => {
            let axle_counter = AxleCounter::from_config(config, ManualSwitch::new());
            axle_counter.run().await?;
        }
        SelectedStrategy::TryUnreliable => {
            let axle_counter = AxleCounter::from_config(config, TryUnreliable);
            axle_counter.run().await?;
        }
    }

    Ok(())
}
