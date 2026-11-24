use rostfrei::AggregateInstance;

use super::{BicycleReservationConfirmed, ReservationRejected, ReserveBicycleCategoryAction};
use crate::domain::rental_fleet::{
    BicycleCategory, BicycleCondition, BicycleStatus, RentalFleet, RentalFleetAggregate,
    RentalPeriod, Reservation, ReservationId,
};
use crate::domain::rental_tariff::RentalQuote;

impl ReserveBicycleCategoryAction for AggregateInstance<RentalFleetAggregate> {
    fn reserve_bicycle_category(
        &mut self,
        reservation_id: ReservationId,
        customer_id: String,
        category: BicycleCategory,
        period: RentalPeriod,
        quote: Option<RentalQuote>,
    ) -> Result<(), ReservationRejected> {
        let reject = |reason: &str| ReservationRejected {
            reservation_id: reservation_id.clone(),
            reason: reason.to_owned(),
        };
        if customer_id.trim().is_empty() {
            return Err(reject("missing-customer"));
        }
        let requested = Reservation {
            reservation_id,
            customer_id,
            category,
            period,
            quote,
            collection: None,
        };
        let root = self.state();
        if let Some(existing) = root
            .reservations
            .iter()
            .find(|existing| existing.reservation_id == requested.reservation_id)
        {
            return if existing.customer_id == requested.customer_id
                && existing.category == requested.category
                && existing.period == requested.period
            {
                Ok(())
            } else {
                Err(ReservationRejected {
                    reservation_id: requested.reservation_id,
                    reason: "reservation-already-exists".to_owned(),
                })
            };
        }
        if requested.quote.is_none() {
            return Err(ReservationRejected {
                reservation_id: requested.reservation_id,
                reason: "quote-unavailable".to_owned(),
            });
        }
        if !has_capacity(root, &requested) {
            return Err(ReservationRejected {
                reservation_id: requested.reservation_id,
                reason: "category-unavailable".to_owned(),
            });
        }
        self.raise(BicycleReservationConfirmed {
            fleet_id: root.fleet_id().clone(),
            reservation_id: requested.reservation_id,
            customer_id: requested.customer_id,
            category: requested.category,
            period: requested.period,
            quote: requested.quote,
        });
        Ok(())
    }
}

fn has_capacity(root: &RentalFleet, requested: &Reservation) -> bool {
    let capacity = root
        .bicycles
        .iter()
        .filter(|bike| {
            bike.category() == Some(&requested.category)
                && bike.condition() == BicycleCondition::Serviceable
                && matches!(
                    bike.status(),
                    BicycleStatus::Available | BicycleStatus::AwaitingPayment
                )
        })
        .count();
    let mut boundaries = root
        .reservations
        .iter()
        .filter(|existing| {
            existing.category == requested.category
                && !existing
                    .collection()
                    .is_some_and(|collection| collection.collected)
                && existing.period.overlaps(&requested.period)
        })
        .flat_map(|existing| {
            [
                (
                    existing
                        .period
                        .starts_at()
                        .max(requested.period.starts_at()),
                    true,
                ),
                (
                    existing.period.ends_at().min(requested.period.ends_at()),
                    false,
                ),
            ]
        })
        .collect::<Vec<_>>();
    // Ends sort before starts, so adjacent bookings do not overlap.
    boundaries.sort_unstable();
    let mut occupied = 0_usize;
    let mut peak = 0;
    for (_, starts) in boundaries {
        occupied = if starts {
            occupied.saturating_add(1)
        } else {
            occupied.saturating_sub(1)
        };
        peak = peak.max(occupied);
    }
    peak < capacity
}
