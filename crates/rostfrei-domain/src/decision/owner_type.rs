use super::DecisionOwnerId;

pub trait DecisionOwnerType: 'static {
    const DECISION_OWNER_ID: DecisionOwnerId;
}
