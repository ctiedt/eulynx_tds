mod decision_logic;
mod subsystem;

use std::net::SocketAddr;

use decision_logic::DecisionLogic;
use figment::{
    providers::{Format, Toml},
    Figment,
};
use futures::StreamExt;
use miette::IntoDiagnostic;
use rasta::SciPacket;
use sci_rs::SCITelegram;
use serde::Deserialize;
use tonic::Streaming;

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
    decision_logic: DecisionLogic,
}

impl AxleCounter {
    pub fn from_config(config: Config) -> Self {
        Self {
            tds_id: config.tds_id,
            ixl_id: config.ixl_id,
            decision_logic: DecisionLogic::new(config.trustworthy, config.unreliable),
        }
    }

    pub async fn run(self) {
        self.decision_logic.run().await;
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct Config {
    tds_id: String,
    ixl_id: String,
    trustworthy: SubsystemConfig,
    unreliable: SubsystemConfig,
}

#[derive(Deserialize, Clone, Debug)]
pub struct SubsystemConfig {
    tds_id: String,
    ixl_id: String,
    ixl_address: SocketAddr,
}

#[tokio::main]
async fn main() -> miette::Result<()> {
    let config: Config = Figment::new()
        .join(Toml::file("tds.toml"))
        .extract()
        .into_diagnostic()?;
    dbg!(config);
    Ok(())
}
