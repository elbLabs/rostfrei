use rostfrei::{Entity, EntityDefinition};

use super::{Bicycle, BicycleId, FleetId, RentalFleetAggregate, Reservation};
use super::{BicycleStatus, CollectionAssignment, ReservationId};
use crate::domain::rental_payment::PaymentId;

#[derive(Entity, Debug)]
#[domain(id = "rental-fleet-root", label = "Rental fleet")]
pub struct RentalFleet {
    pub(super) fleet_id: FleetId,
    #[domain(entity)]
    pub(super) bicycles: Vec<Bicycle>,
    #[domain(entity)]
    pub(super) reservations: Vec<Reservation>,
}

impl EntityDefinition for RentalFleet {
    type Owner = RentalFleetAggregate;
    type Identity = FleetId;

    fn identity(&self) -> &Self::Identity {
        &self.fleet_id
    }
}

impl RentalFleet {
    pub fn active_reservation(&self, bicycle_id: &BicycleId) -> Option<&Reservation> {
        self.reservations.iter().find(|reservation| {
            reservation.collection().is_some_and(|collection| {
                collection.collected && !collection.returned && &collection.bicycle_id == bicycle_id
            })
        })
    }
    pub fn pending_collection(
        &self,
        reservation_id: &ReservationId,
        payment_id: &PaymentId,
    ) -> Option<&CollectionAssignment> {
        self.reservations
            .iter()
            .find(|r| r.reservation_id() == reservation_id)
            .and_then(Reservation::collection)
            .filter(|collection| {
                !collection.collected
                    && &collection.payment_id == payment_id
                    && self.bicycles.iter().any(|bike| {
                        bike.bicycle_id() == &collection.bicycle_id
                            && bike.status() == BicycleStatus::AwaitingPayment
                    })
            })
    }
    pub const fn new(fleet_id: FleetId, bicycles: Vec<Bicycle>) -> Self {
        Self {
            fleet_id,
            bicycles,
            reservations: Vec::new(),
        }
    }

    pub fn bicycles(&self) -> &[Bicycle] {
        &self.bicycles
    }

    pub const fn fleet_id(&self) -> &FleetId {
        &self.fleet_id
    }

    pub fn reservations(&self) -> &[Reservation] {
        &self.reservations
    }
}
