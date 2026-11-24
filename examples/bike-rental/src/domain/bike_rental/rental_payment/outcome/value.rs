use rostfrei::ValueObject;
use serde::{Deserialize, Serialize};

#[derive(ValueObject, Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(id = "payment-outcome", label = "Payment outcome")]
#[serde(rename_all = "kebab-case")]
pub enum PaymentOutcome {
    Approved,
    Declined,
}
