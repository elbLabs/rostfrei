use crate::domain::{
    rental_fleet::{BicycleId, RentalReturn},
    rental_payment::PaymentId,
};
use rostfrei::ValueObject;
use serde::{Deserialize, Serialize};

#[derive(ValueObject, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(id = "collection-assignment", label = "Collection assignment")]
pub struct CollectionAssignment {
    pub bicycle_id: BicycleId,
    pub payment_id: PaymentId,
    pub collected: bool,
    #[serde(default)]
    pub returned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_details: Option<RentalReturn>,
}
