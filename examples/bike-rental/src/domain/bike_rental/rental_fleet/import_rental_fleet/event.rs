use rostfrei::DomainEvent;
use serde::{Deserialize, Serialize};

use super::ImportedBicycle;
use crate::domain::rental_fleet::FleetId;

#[derive(DomainEvent, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(id = "rental-fleet-imported", label = "Rental fleet imported")]
pub struct RentalFleetImported {
    #[domain(identity)]
    pub fleet_id: FleetId,
    pub bicycles: Vec<ImportedBicycle>,
}
