use crate::ActiveSubsystem;
use sci_rs::{SCIMessageType, SCITelegram};
use std::{net::UdpSocket, sync::Arc};
use tokio::sync::RwLock;
use tracing::{info, warn};

pub trait DecisionStrategy: Send + Sync {
    const INITIAL_SUBSYSTEM: ActiveSubsystem;

    fn switch_to(
        &self,
        msg_trustworthy: &Option<SCITelegram>,
        msg_unreliable: &Option<SCITelegram>,
    ) -> Option<ActiveSubsystem>;
}

pub struct AlwaysUnreliable;

impl DecisionStrategy for AlwaysUnreliable {
    const INITIAL_SUBSYSTEM: ActiveSubsystem = ActiveSubsystem::Unreliable;

    fn switch_to(
        &self,
        msg_trustworthy: &Option<SCITelegram>,
        msg_unreliable: &Option<SCITelegram>,
    ) -> Option<ActiveSubsystem> {
        if msg_unreliable.is_none() {
            warn!("No message from Unreliable subsystem");
        }
        if msg_trustworthy.is_none() {
            warn!("No message from Trustworthy subsystem");
        }
        None
    }
}

pub struct AlwaysTrustworthy;

impl DecisionStrategy for AlwaysTrustworthy {
    const INITIAL_SUBSYSTEM: ActiveSubsystem = ActiveSubsystem::Trustworthy;

    fn switch_to(
        &self,
        msg_trustworthy: &Option<SCITelegram>,
        msg_unreliable: &Option<SCITelegram>,
    ) -> Option<ActiveSubsystem> {
        if msg_unreliable.is_none() {
            warn!("No message from Unreliable subsystem");
        }
        if msg_trustworthy.is_none() {
            warn!("No message from Trustworthy subsystem");
        }
        None
    }
}

pub struct TryUnreliable;

impl DecisionStrategy for TryUnreliable {
    const INITIAL_SUBSYSTEM: ActiveSubsystem = ActiveSubsystem::Unreliable;

    fn switch_to(
        &self,
        msg_trustworthy: &Option<SCITelegram>,
        msg_unreliable: &Option<SCITelegram>,
    ) -> Option<ActiveSubsystem> {
        match (msg_trustworthy, msg_unreliable) {
            (None, None) => unreachable!(),
            (None, Some(_)) | (Some(_), None) => Some(ActiveSubsystem::Trustworthy),
            (Some(tw), Some(ur)) => {
                if tw.message_type != ur.message_type {
                    warn!("Different message types - {tw} and {ur}");
                    return Some(ActiveSubsystem::Trustworthy);
                }
                if tw.message_type == SCIMessageType::scitds_tvps_occupancy_status() {
                    let occupancy_status_differs = tw.payload[0] != ur.payload[0];
                    let ability_to_fc_differs = tw.payload[1] != ur.payload[1];
                    let filling_level_differs = tw.payload[2..3] != ur.payload[2..3];
                    info!("{:?} {:?}", &*tw.payload, &*ur.payload);
                    info!("{occupancy_status_differs} {ability_to_fc_differs} {filling_level_differs}");
                    if occupancy_status_differs | ability_to_fc_differs {
                        Some(ActiveSubsystem::Trustworthy)
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
        }
    }
}

pub struct ManualSwitch {
    active: Arc<RwLock<ActiveSubsystem>>,
}

impl ManualSwitch {
    pub fn new() -> Self {
        let active = Arc::new(RwLock::new(Self::INITIAL_SUBSYSTEM));
        let active_clone = active.clone();
        std::thread::spawn(move || {
            let listener = UdpSocket::bind("0.0.0.0:3333").unwrap();
            let mut buf = [0; 1024];
            loop {
                let _ = listener.recv(&mut buf);
                if buf[0] == b'T' {
                    info!("Got request to switch to trustworthy");
                    *active_clone.blocking_write() = ActiveSubsystem::Trustworthy;
                }
                if buf[0] == b'U' {
                    info!("Got request to switch to unreliable");
                    *active_clone.blocking_write() = ActiveSubsystem::Unreliable;
                }
            }
        });
        ManualSwitch { active }
    }
}

impl DecisionStrategy for ManualSwitch {
    const INITIAL_SUBSYSTEM: ActiveSubsystem = ActiveSubsystem::Unreliable;

    fn switch_to(
        &self,
        _msg_trustworthy: &Option<SCITelegram>,
        _msg_unreliable: &Option<SCITelegram>,
    ) -> Option<ActiveSubsystem> {
        let active = futures::executor::block_on(self.active.read());
        Some(*active)
    }
}
