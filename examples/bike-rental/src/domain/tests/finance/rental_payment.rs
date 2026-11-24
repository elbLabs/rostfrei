use crate::domain::{
    Finance,
    rental_fleet::{BicycleId, FleetId, ReservationId},
    rental_payment::{
        AssessRentalReturn, CollectRentalPayment, CollectionPayment, PaymentOutcome,
        RecordDepositRefundAction as _, RecordPaymentResultAction as _, RecordRentalPaymentRequest,
        RecordRentalReturnEligibilityAction as _, RefundEligibleRentalDeposit,
        RentalPaymentAggregate, RentalPaymentEvent, RequestRentalPaymentAction as _,
    },
    rental_tariff::RentalQuote,
};
use rostfrei::{
    Aggregate, AggregateInstance, Command, StreamAggregateId, StreamAggregateType, StreamId,
};

#[test]
fn payment_commands_belong_to_finance() {
    fn assert_finance_command<C: Command<Context = Finance>>() {}

    assert_finance_command::<AssessRentalReturn>();
    assert_finance_command::<CollectRentalPayment>();
    assert_finance_command::<RecordRentalPaymentRequest>();
    assert_finance_command::<RefundEligibleRentalDeposit>();
}

#[test]
fn another_fleet_cannot_take_over_a_payment_identity() {
    let stream = StreamId::new(
        StreamAggregateType::new(RentalPaymentAggregate::aggregate_type()).expect("payment type"),
        StreamAggregateId::new("cross-fleet-payment").expect("payment ID"),
    );
    let mut payment =
        AggregateInstance::<RentalPaymentAggregate>::rehydrate(stream.clone(), vec![]);
    let first = CollectionPayment {
        fleet_id: FleetId::new("first-fleet").expect("fleet ID"),
        reservation_id: ReservationId::new("reservation").expect("reservation ID"),
        bicycle_id: BicycleId::new("bicycle").expect("bicycle ID"),
        customer_id: "alex".to_owned(),
        quote: RentalQuote {
            currency: "EUR".to_owned(),
            rental_charge_minor: 2500,
            security_deposit_minor: Some(0),
        },
        payment_method: "sandbox-approved".to_owned(),
    };
    let reused = CollectionPayment {
        fleet_id: FleetId::new("other-fleet").expect("fleet ID"),
        ..first.clone()
    };
    payment
        .request_rental_payment(first.clone())
        .expect("owns payment ID");
    payment
        .request_rental_payment(reused.clone())
        .expect("records conflicting hold");
    assert_eq!(payment.state().details(), Some(&first));
    assert!(
        matches!(&payment.uncommitted_events()[1], RentalPaymentEvent::RentalPaymentIdentityConflictRecorded(conflict) if conflict.fleet_id == reused.fleet_id)
    );

    let mut payment =
        AggregateInstance::<RentalPaymentAggregate>::rehydrate(stream, payment.into_parts().2);
    payment
        .request_rental_payment(reused)
        .expect("duplicate collision is a no-op");
    assert!(payment.uncommitted_events().is_empty());
}

#[test]
fn payment_identity_and_refundable_return_survive_replay() {
    let stream = StreamId::new(
        StreamAggregateType::new(RentalPaymentAggregate::aggregate_type()).expect("payment type"),
        StreamAggregateId::new("finance-test-payment").expect("payment ID"),
    );
    let mut payment =
        AggregateInstance::<RentalPaymentAggregate>::rehydrate(stream.clone(), vec![]);
    let details = CollectionPayment {
        fleet_id: FleetId::new("finance-test-fleet").expect("fleet ID"),
        reservation_id: ReservationId::new("finance-test-reservation").expect("reservation ID"),
        bicycle_id: BicycleId::new("finance-test-bicycle").expect("bicycle ID"),
        customer_id: "alex".to_owned(),
        quote: RentalQuote {
            currency: "EUR".to_owned(),
            rental_charge_minor: 2500,
            security_deposit_minor: Some(10000),
        },
        payment_method: "sandbox-approved".to_owned(),
    };
    payment
        .request_rental_payment(details.clone())
        .expect("request");
    assert_eq!(
        payment
            .record_deposit_refund("refund".to_owned())
            .expect_err("unapproved")
            .reason,
        "payment-not-approved"
    );
    payment
        .record_payment_result(PaymentOutcome::Approved, "charge".to_owned())
        .expect("charge");
    assert_eq!(
        payment
            .record_deposit_refund("refund".to_owned())
            .expect_err("not yet eligible")
            .reason,
        "return-not-eligible"
    );

    let wrong_bicycle = BicycleId::new("other-bicycle").expect("bicycle ID");
    let conflicting = CollectionPayment {
        bicycle_id: wrong_bicycle.clone(),
        ..details.clone()
    };
    assert_eq!(
        payment
            .request_rental_payment(conflicting)
            .expect_err("identity reuse")
            .reason,
        "payment-identity-conflict"
    );
    assert_eq!(
        payment
            .record_rental_return_eligibility(
                &details.fleet_id,
                &details.reservation_id,
                &wrong_bicycle,
                true
            )
            .expect_err("mismatched return")
            .reason,
        "payment-identity-conflict"
    );
    payment
        .record_rental_return_eligibility(
            &details.fleet_id,
            &details.reservation_id,
            &details.bicycle_id,
            true,
        )
        .expect("eligible return");

    let mut payment =
        AggregateInstance::<RentalPaymentAggregate>::rehydrate(stream, payment.into_parts().2);
    assert_eq!(
        payment
            .record_rental_return_eligibility(
                &details.fleet_id,
                &details.reservation_id,
                &details.bicycle_id,
                false
            )
            .expect_err("contradictory return")
            .reason,
        "return-eligibility-conflict"
    );
    payment
        .request_rental_payment(details)
        .expect("exact replay");
    payment
        .record_deposit_refund("refund".to_owned())
        .expect("refund eligible deposit");
    payment
        .record_deposit_refund("refund".to_owned())
        .expect("exact refund replay");
    assert_eq!(payment.uncommitted_events().len(), 1);
}
