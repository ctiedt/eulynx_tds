use crate::ActiveSubsystem;
use sci_rs::SCITelegram;
use tracing::warn;

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
