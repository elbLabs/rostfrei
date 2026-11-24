use rostfrei::{AggregateInstance, StreamAggregateId, StreamAggregateType, StreamId};

use crate::domain::{
    rental_fleet::RentalPeriod,
    rental_tariff::{ConfigureRentalTariffAction as _, RentalTariffAggregate, RentalTariffTerms},
};

#[test]
fn invalid_tariff_changes_preserve_the_current_terms() {
    let stream = StreamId::new(
        StreamAggregateType::new("bike-rental/rental-tariff").expect("valid aggregate type"),
        StreamAggregateId::new("city").expect("valid tariff identity"),
    );
    let valid = RentalTariffTerms {
        currency: "EUR".to_owned(),
        hourly_rate_minor: 500,
        daily_cap_minor: 2500,
        security_deposit_minor: Some(10000),
    };
    let period =
        RentalPeriod::new("2026-12-01T09:00:00Z", "2026-12-01T17:00:00Z").expect("valid period");
    let mut tariff = AggregateInstance::<RentalTariffAggregate>::rehydrate(stream.clone(), vec![]);
    tariff
        .configure_rental_tariff(valid.clone())
        .expect("valid tariff");
    let mut tariff =
        AggregateInstance::<RentalTariffAggregate>::rehydrate(stream, tariff.into_parts().2);
    for (currency, hourly_rate_minor, daily_cap_minor) in
        [("eur", 500, 2500), ("EUR", 0, 2500), ("EUR", 500, 0)]
    {
        let invalid = RentalTariffTerms {
            currency: currency.to_owned(),
            hourly_rate_minor,
            daily_cap_minor,
            security_deposit_minor: Some(10000),
        };
        assert!(invalid.quote(&period).is_none());
        assert!(tariff.configure_rental_tariff(invalid).is_err());
        assert_eq!(tariff.state().terms(), Some(&valid));
        assert!(tariff.uncommitted_events().is_empty());
    }
    let missing_deposit = RentalTariffTerms {
        security_deposit_minor: None,
        ..valid.clone()
    };
    assert!(missing_deposit.quote(&period).is_none());
    assert!(tariff.configure_rental_tariff(missing_deposit).is_err());
    assert_eq!(tariff.state().terms(), Some(&valid));
    assert!(tariff.uncommitted_events().is_empty());
}
