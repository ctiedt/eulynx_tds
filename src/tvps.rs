use miette::IntoDiagnostic;
use sci_rs::{scitds::OccupancyStatus, SCITelegram};
use tokio::net::UdpSocket;
use tracing::info;

use crate::{rasta::SciPacket, TvpsConfig};

pub struct Tvps {
    id: String,
    ixl_id: String,
    listener: UdpSocket,
    axes_in: i16,
    axes_out: i16,
}

impl Tvps {
    pub async fn new<S: Into<String>>(config: TvpsConfig, ixl_id: S) -> miette::Result<Self> {
        let listener = tokio::net::UdpSocket::bind(&config.address)
            .await
            .into_diagnostic()?;
        info!("TVPS {} is listening on {}", &config.name, &config.address);
        Ok(Self {
            id: config.name,
            ixl_id: ixl_id.into(),
            listener,
            axes_in: 0,
            axes_out: 0,
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn status(&self) -> OccupancyStatus {
        match self.axes_in.cmp(&self.axes_out) {
            std::cmp::Ordering::Less => OccupancyStatus::Disturbed,
            std::cmp::Ordering::Equal => OccupancyStatus::Vacant,
            std::cmp::Ordering::Greater => OccupancyStatus::Occupied,
        }
    }

    fn status_telegram(&self) -> SCITelegram {
        SCITelegram::tvps_occupancy_status(
            &self.id,
            &self.ixl_id,
            self.status(),
            false,
            self.filling_level(),
            sci_rs::scitds::POMStatus::Ok,
            sci_rs::scitds::DisturbanceStatus::Operational,
            sci_rs::scitds::ChangeTrigger::PassingDetected,
        )
    }

    pub fn filling_level(&self) -> i16 {
        self.axes_in - self.axes_out
    }

    fn reset(&mut self) {
        self.axes_in = 0;
        self.axes_out = 0;
    }

    pub async fn wait_for_passing_axle(
        &mut self,
        tx: tokio::sync::broadcast::Sender<SciPacket>,
    ) -> miette::Result<()> {
        loop {
            let mut buf = Vec::with_capacity(512);

            self.listener
                .recv_buf_from(&mut buf)
                .await
                .into_diagnostic()?;

            let msg = String::from_utf8_lossy(&buf);
            let msg = msg.trim();

            if msg == "+" {
                self.axes_in += 1;
                if self.axes_in == 1 {
                    tx.send(self.status_telegram().into()).into_diagnostic()?;
                }
            } else if msg == "-" {
                self.axes_out += 1;
                if self.filling_level() == 0 {
                    tx.send(self.status_telegram().into()).into_diagnostic()?;
                    self.reset();
                }
            }
        }
    }
}
