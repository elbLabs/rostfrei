use rostfrei::ValueObject;
use serde::{Deserialize, Serialize};

#[derive(ValueObject, Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(
    id = "bicycle-transfer-rejection-reason",
    label = "Bicycle transfer rejection reason"
)]
pub enum BicycleTransferRejectionReason {
    SameFleet,
    SourceBicycleMissing,
    SourceBicycleNotAvailable,
    DestinationAlreadyContainsBicycle,
    DestinationFleetMismatch,
}
