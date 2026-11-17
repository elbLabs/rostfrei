use rostfrei::{Entity, EntityDefinition};

use super::{CollectionAssignment, RentalPeriod, ReservationId};
use crate::domain::rental_fleet::{BicycleCategory, RentalFleetAggregate};
use crate::domain::rental_tariff::RentalQuote;

#[allow(
    clippy::struct_field_names,
    reason = "reservation_id is the canonical domain identity name"
)]
#[derive(Entity, Clone, Debug, Eq, PartialEq)]
#[domain(id = "reservation", label = "Reservation")]
pub struct Reservation {
    pub(in crate::domain::bike_rental::rental_fleet) reservation_id: ReservationId,
    pub(in crate::domain::bike_rental::rental_fleet) customer_id: String,
    pub(in crate::domain::bike_rental::rental_fleet) category: BicycleCategory,
    pub(in crate::domain::bike_rental::rental_fleet) period: RentalPeriod,
    pub(in crate::domain::bike_rental::rental_fleet) quote: Option<RentalQuote>,
    pub(in crate::domain::bike_rental::rental_fleet) collection: Option<CollectionAssignment>,
}

impl EntityDefinition for Reservation {
    type Owner = RentalFleetAggregate;
    type Identity = ReservationId;

    fn identity(&self) -> &Self::Identity {
        &self.reservation_id
    }
}

impl Reservation {
    pub const fn collection(&self) -> Option<&CollectionAssignment> {
        self.collection.as_ref()
    }
    pub const fn quote(&self) -> Option<&RentalQuote> {
        self.quote.as_ref()
    }
    pub const fn reservation_id(&self) -> &ReservationId {
        &self.reservation_id
    }
    pub fn customer_id(&self) -> &str {
        &self.customer_id
    }
    pub const fn category(&self) -> &BicycleCategory {
        &self.category
    }
    pub const fn period(&self) -> &RentalPeriod {
        &self.period
    }
}
