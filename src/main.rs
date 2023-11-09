mod decision_logic;
mod subsystem;

use std::{net::SocketAddr, sync::Arc, time::Duration};

use decision_logic::{ActiveSubsystem, DecisionLogic};
use figment::{
    providers::{Format, Toml},
    Figment,
};
use futures::StreamExt;
use miette::IntoDiagnostic;
use rasta::{rasta_client::RastaClient, SciPacket};
use sci_rs::SCITelegram;
use serde::{Deserialize, Deserializer};
use tokio::sync::{broadcast::Sender, RwLock};
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

pub struct AxleCounter {
    tds_id: String,
    ixl_id: String,
    rasta_id: String,
    ixl_address: String,
    decision_logic: DecisionLogic,
    sender: Sender<SCITelegram>,
    active: Arc<RwLock<ActiveSubsystem>>,
}

impl AxleCounter {
    pub fn from_config(config: Config) -> Self {
        let decision_logic =
            DecisionLogic::new(config.trustworthy, config.unreliable, config.timeout);
        let sender = decision_logic.get_sender();
        let active = decision_logic.active();
        Self {
            tds_id: config.tds_id,
            ixl_id: config.ixl_id,
            rasta_id: config.rasta_id,
            ixl_address: config.ixl_address,
            decision_logic,
            sender,
            active,
        }
    }

    pub async fn run(self) -> miette::Result<()> {
        let stream = self.decision_logic.run().await;

        tokio::time::sleep(Duration::from_secs(10)).await;

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

        info!("Closing Object Controller");
        Ok(())
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct Config {
    tds_id: String,
    ixl_id: String,
    ixl_address: String,
    rasta_id: String,
    #[serde(deserialize_with = "deserialize_timer_value")]
    timeout: Duration,
    trustworthy: SubsystemConfig,
    unreliable: SubsystemConfig,
}

#[derive(Deserialize, Clone, Debug)]
pub struct SubsystemConfig {
    tds_id: String,
    ixl_id: String,
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

    let axle_counter = AxleCounter::from_config(config);
    axle_counter.run().await?;

    Ok(())
}
