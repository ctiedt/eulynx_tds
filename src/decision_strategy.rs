use crate::ActiveSubsystem;
use sci_rs::SCITelegram;

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
        _msg_unreliable: &Option<SCITelegram>,
        _msg_trustworthy: &Option<SCITelegram>,
    ) -> Option<ActiveSubsystem> {
        None
    }
}
