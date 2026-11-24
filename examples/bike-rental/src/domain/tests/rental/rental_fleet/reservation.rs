use rostfrei::AggregateInstance;
use serde_json::json;

use crate::domain::rental_fleet::{
    self, AddCategorizedBicycleAction as _, BicycleCategory, BicycleCondition, BicycleId,
    BicycleReservationConfirmed, RentalFleetAggregate, RentalPeriod, ReservationId,
    ReserveBicycleCategoryAction as _,
};
use crate::domain::rental_fleet::{
    CollectReservedBicycleAction as _, ResolveCollectionAction as _, ReturnBicycleAction as _,
};
use crate::domain::rental_fleet::{RentalReturn, ReturnCollectedBicycleAction as _, ReturnTime};
use crate::domain::rental_payment::{PaymentId, PaymentOutcome};
use crate::domain::rental_tariff::RentalQuote;

fn quote() -> RentalQuote {
    RentalQuote {
        currency: "EUR".to_owned(),
        rental_charge_minor: 2500,
        security_deposit_minor: Some(10000),
    }
}

fn period(start: &str, end: &str) -> RentalPeriod {
    RentalPeriod::new(
        &format!("2026-12-01T{start}:00Z"),
        &format!("2026-12-01T{end}:00Z"),
    )
    .expect("valid rental period")
}

fn category(value: &str) -> BicycleCategory {
    BicycleCategory::new(value).expect("valid category")
}

fn fleet() -> AggregateInstance<RentalFleetAggregate> {
    let mut fleet = AggregateInstance::<RentalFleetAggregate>::rehydrate(
        rental_fleet::stream_id("city-fleet").expect("valid fleet"),
        vec![],
    );
    for (id, kind) in [
        ("city-1", "city"),
        ("city-2", "city"),
        ("electric-1", "electric"),
    ] {
        fleet
            .add_categorized_bicycle(
                BicycleId::new(id).expect("valid bicycle"),
                BicycleCondition::Serviceable,
                category(kind),
            )
            .expect("new bicycle");
    }
    fleet
}

#[test]
fn capacity_tracks_peak_overlapping_demand_in_the_requested_category() {
    let mut fleet = fleet();
    for (id, kind, start, end) in [
        ("morning", "city", "09:00", "12:00"),
        ("afternoon", "city", "12:00", "17:00"),
        ("all-day", "city", "09:00", "17:00"),
        ("electric", "electric", "10:00", "11:00"),
    ] {
        fleet
            .reserve_bicycle_category(
                ReservationId::new(id).expect("valid reservation"),
                "alex".to_owned(),
                category(kind),
                period(start, end),
                Some(quote()),
            )
            .expect("peak demand fits category capacity");
    }
    let rejected = fleet
        .reserve_bicycle_category(
            ReservationId::new("excess").expect("valid reservation"),
            "sam".to_owned(),
            category("city"),
            period("10:00", "11:00"),
            Some(quote()),
        )
        .expect_err("other categories cannot satisfy an overbooked city category");
    assert_eq!(rejected.reason, "category-unavailable");
    assert_eq!(fleet.state().reservations().len(), 4);
}

#[test]
fn reservation_identity_survives_replay_and_cannot_change_customer_or_period() {
    let mut fleet = fleet();
    let id = ReservationId::new("booking-42").expect("valid reservation");
    fleet
        .reserve_bicycle_category(
            id.clone(),
            "alex".to_owned(),
            category("city"),
            period("09:00", "17:00"),
            Some(quote()),
        )
        .expect("first reservation");
    let mut replay = AggregateInstance::<RentalFleetAggregate>::rehydrate(
        rental_fleet::stream_id("city-fleet").expect("valid fleet"),
        fleet.into_parts().2,
    );
    replay
        .reserve_bicycle_category(
            id.clone(),
            "alex".to_owned(),
            category("city"),
            period("09:00", "17:00"),
            Some(quote()),
        )
        .expect("same reservation");
    let rejected = replay
        .reserve_bicycle_category(
            id,
            "sam".to_owned(),
            category("city"),
            period("10:00", "11:00"),
            Some(quote()),
        )
        .expect_err("conflicting identity");
    assert_eq!(rejected.reason, "reservation-already-exists");
    assert!(replay.uncommitted_events().is_empty());
    assert_eq!(replay.state().reservations()[0].customer_id(), "alex");
}

#[test]
fn collection_holds_and_completed_reservations_do_not_double_count_capacity() {
    let mut fleet = fleet();
    let booking = ReservationId::new("collecting-booking").expect("valid reservation");
    let payment = PaymentId::new("payment-42").expect("valid payment");
    let bicycle = BicycleId::new("city-1").expect("valid bicycle");
    fleet
        .reserve_bicycle_category(
            booking.clone(),
            "alex".to_owned(),
            category("city"),
            period("09:00", "17:00"),
            Some(quote()),
        )
        .expect("first reservation");
    fleet
        .collect_reserved_bicycle(
            booking.clone(),
            bicycle.clone(),
            payment.clone(),
            "sandbox-approved".to_owned(),
        )
        .expect("collection hold");
    fleet
        .reserve_bicycle_category(
            ReservationId::new("second-booking").expect("valid reservation"),
            "sam".to_owned(),
            category("city"),
            period("09:00", "17:00"),
            Some(quote()),
        )
        .expect("hold must not consume capacity twice");
    fleet
        .resolve_collection(booking, bicycle.clone(), payment, PaymentOutcome::Approved)
        .expect("approved collection");
    fleet.return_bicycle(bicycle).expect("rental returned");
    fleet
        .reserve_bicycle_category(
            ReservationId::new("third-booking").expect("valid reservation"),
            "pat".to_owned(),
            category("city"),
            period("09:00", "17:00"),
            Some(quote()),
        )
        .expect("collected reservation must no longer occupy capacity after return");
}

#[test]
fn stale_payment_outcomes_cannot_resolve_a_new_hold_after_a_decline() {
    let mut fleet = fleet();
    let booking = ReservationId::new("stale-outcome-booking").expect("valid reservation");
    let bicycle = BicycleId::new("city-1").expect("valid bicycle");
    let declined = PaymentId::new("declined-attempt").expect("valid payment");
    let retry = PaymentId::new("fresh-attempt").expect("valid payment");
    fleet
        .reserve_bicycle_category(
            booking.clone(),
            "alex".to_owned(),
            category("city"),
            period("09:00", "17:00"),
            Some(quote()),
        )
        .expect("reservation");
    let collect = |fleet: &mut AggregateInstance<RentalFleetAggregate>, payment, method: &str| {
        fleet.collect_reserved_bicycle(booking.clone(), bicycle.clone(), payment, method.to_owned())
    };
    let decide = |fleet: &mut AggregateInstance<RentalFleetAggregate>, payment, outcome| {
        fleet.resolve_collection(booking.clone(), bicycle.clone(), payment, outcome)
    };

    let original = collect(&mut fleet, declined.clone(), "decline").expect("first hold");
    decide(&mut fleet, declined.clone(), PaymentOutcome::Declined).expect("release hold");
    let mut fleet = AggregateInstance::<RentalFleetAggregate>::rehydrate(
        rental_fleet::stream_id("city-fleet").expect("valid fleet"),
        fleet.into_parts().2,
    );
    assert_eq!(
        collect(&mut fleet, declined.clone(), "decline").expect("exact retry"),
        original
    );
    assert_eq!(
        collect(&mut fleet, declined.clone(), "approved")
            .expect_err("reused ID")
            .reason,
        "payment-identity-conflict"
    );
    assert!(fleet.uncommitted_events().is_empty());

    collect(&mut fleet, retry.clone(), "approved").expect("new hold");
    decide(&mut fleet, declined, PaymentOutcome::Approved).expect("stale outcome is a no-op");
    assert_eq!(fleet.uncommitted_events().len(), 1);
    assert!(fleet.state().pending_collection(&booking, &retry).is_some());
    assert_eq!(
        fleet
            .resolve_collection(
                booking.clone(),
                BicycleId::new("city-2").expect("valid bicycle"),
                retry.clone(),
                PaymentOutcome::Approved
            )
            .expect_err("wrong bicycle")
            .reason,
        "collection-identity-conflict"
    );
    decide(&mut fleet, retry, PaymentOutcome::Approved).expect("approve exact hold");
    assert!(fleet.state().active_reservation(&bicycle).is_some());
}

#[test]
fn refund_eligibility_uses_known_return_instants_and_the_booked_deadline() {
    let booked = period("09:00", "17:00");
    for (timestamp, damaged, expected) in [
        (Some("2026-12-01T18:00:00+01:00"), false, true),
        (Some("2026-12-01T16:30:00Z"), false, true),
        (Some("2026-12-01T17:00:01Z"), false, false),
        (Some("2026-12-01T08:59:59Z"), false, false),
        (Some("2026-12-01T17:00:00Z"), true, false),
        (None, false, false),
    ] {
        let returned = RentalReturn {
            returned_at: timestamp.map(|value| ReturnTime::new(value).expect("valid timestamp")),
            damaged,
        };
        assert_eq!(
            returned.permits_full_refund(&booked),
            expected,
            "{timestamp:?}, damaged={damaged}"
        );
    }
    assert!(serde_json::from_value::<ReturnTime>(json!("2026-12-01T17:00:00")).is_err());
}

#[test]
fn successive_rentals_return_against_their_own_reservations() {
    let mut fleet = fleet();
    let bicycle = BicycleId::new("city-1").expect("valid bicycle");
    for (id, start, end) in [("first", "09:00", "12:00"), ("second", "12:00", "17:00")] {
        let booking = ReservationId::new(id).expect("valid reservation");
        let payment = PaymentId::new(id).expect("valid payment");
        fleet
            .reserve_bicycle_category(
                booking.clone(),
                "alex".to_owned(),
                category("city"),
                period(start, end),
                Some(quote()),
            )
            .expect("booking");
        fleet
            .collect_reserved_bicycle(
                booking.clone(),
                bicycle.clone(),
                payment.clone(),
                "sandbox-approved".to_owned(),
            )
            .expect("collection");
        fleet
            .resolve_collection(booking, bicycle.clone(), payment, PaymentOutcome::Approved)
            .expect("approval");
        fleet
            .return_collected_bicycle(
                bicycle.clone(),
                Some(ReturnTime::new(&format!("2026-12-01T{end}:00Z")).expect("valid time")),
                None,
            )
            .expect("return");
    }
    let fleet = AggregateInstance::<RentalFleetAggregate>::rehydrate(
        rental_fleet::stream_id("city-fleet").expect("valid fleet"),
        fleet.into_parts().2,
    );
    let returns: Vec<_> = fleet
        .state()
        .reservations()
        .iter()
        .map(|reservation| {
            let collection = reservation.collection().expect("collected");
            assert!(collection.returned);
            (
                collection.payment_id.as_str(),
                collection
                    .return_details
                    .as_ref()
                    .expect("return details")
                    .returned_at
                    .as_ref()
                    .expect("return time")
                    .instant(),
            )
        })
        .collect();
    assert_eq!(
        returns,
        vec![
            ("first", period("09:00", "12:00").ends_at()),
            ("second", period("12:00", "17:00").ends_at())
        ]
    );
}

#[test]
fn historical_reservations_replay_without_inventing_a_quote_or_deposit() {
    for old_quote in [
        None,
        Some(json!({"currency": "EUR", "rental_charge_minor": 2500})),
    ] {
        let mut payload = json!({
            "fleet_id": "city-fleet", "reservation_id": "legacy-booking", "customer_id": "alex",
            "category": "city", "period": {"starts_at": "2026-12-01T09:00:00Z", "ends_at": "2026-12-01T17:00:00Z"}
        });
        if let Some(value) = &old_quote {
            payload["quote"] = value.clone();
        }
        let event: BicycleReservationConfirmed =
            serde_json::from_value(payload).expect("historical confirmation");
        let mut replay = AggregateInstance::<RentalFleetAggregate>::rehydrate(
            rental_fleet::stream_id("city-fleet").expect("valid fleet"),
            vec![event.into()],
        );
        replay
            .reserve_bicycle_category(
                ReservationId::new("legacy-booking").expect("valid reservation"),
                "alex".to_owned(),
                category("city"),
                period("09:00", "17:00"),
                Some(quote()),
            )
            .expect("historical reservation is accepted without repricing");
        assert!(replay.uncommitted_events().is_empty());
        let persisted = replay.state().reservations()[0]
            .quote()
            .map(|value| serde_json::to_value(value).expect("serialize quote"));
        assert_eq!(persisted, old_quote);
    }
}

#[test]
fn rental_periods_compare_instants_and_validate_persisted_input() {
    let utc = period("09:00", "17:00");
    let offset = RentalPeriod::new("2026-12-01T10:00:00+01:00", "2026-12-01T18:00:00+01:00")
        .expect("equivalent offset");
    assert_eq!(utc, offset);
    assert!(!utc.overlaps(&period("17:00", "18:00")));
    for (start, end) in [
        ("2026-12-01T09:00:00Z", "2026-12-01T09:00:00Z"),
        ("2026-12-01T09:00:00", "2026-12-01T17:00:00Z"),
    ] {
        assert!(
            serde_json::from_value::<RentalPeriod>(json!({"starts_at": start, "ends_at": end}))
                .is_err()
        );
    }
    assert_eq!(
        serde_json::from_value::<RentalPeriod>(serde_json::to_value(&utc).expect("encode period"))
            .expect("decode period"),
        utc
    );
}
