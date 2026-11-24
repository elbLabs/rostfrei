use crate::domain::{rental_fleet::RentalPeriod, rental_tariff::RentalTariffTerms};

#[test]
fn hourly_quotes_round_up_and_cap_each_elapsed_day() {
    let terms = RentalTariffTerms {
        currency: "EUR".to_owned(),
        hourly_rate_minor: 500,
        daily_cap_minor: 2500,
        security_deposit_minor: Some(10000),
    };
    for (end, expected) in [
        ("2026-12-01T10:00:00Z", 500),
        ("2026-12-01T10:00:00.000000001Z", 1000),
        ("2026-12-01T10:10:00Z", 1000),
        ("2026-12-01T17:00:00Z", 2500),
        ("2026-12-02T10:10:00Z", 3500),
        ("2026-12-03T09:00:00Z", 5000),
    ] {
        let period = RentalPeriod::new("2026-12-01T09:00:00Z", end).expect("valid period");
        let quote = terms.quote(&period).expect("representable quote");
        assert_eq!(quote.rental_charge_minor, expected, "end: {end}");
        assert_eq!(quote.currency, "EUR");
        assert_eq!(quote.security_deposit_minor, Some(10000));
    }
}

#[test]
fn a_cap_never_increases_hourly_charges_and_overflow_cannot_become_a_quote() {
    let day =
        RentalPeriod::new("2026-12-01T09:00:00Z", "2026-12-02T09:00:00Z").expect("valid period");
    let high_cap = RentalTariffTerms {
        currency: "EUR".to_owned(),
        hourly_rate_minor: 500,
        daily_cap_minor: 50_000,
        security_deposit_minor: Some(10000),
    };
    assert_eq!(
        high_cap.quote(&day).expect("quote").rental_charge_minor,
        12_000
    );
    let overflowing = RentalTariffTerms {
        currency: "EUR".to_owned(),
        hourly_rate_minor: u64::MAX,
        daily_cap_minor: u64::MAX,
        security_deposit_minor: Some(10000),
    };
    assert_eq!(
        overflowing
            .quote(&day)
            .expect("cap fits even when hourly multiplication exceeds u64")
            .rental_charge_minor,
        u64::MAX
    );
    let longer =
        RentalPeriod::new("2026-12-01T09:00:00Z", "2026-12-02T10:00:00Z").expect("valid period");
    assert!(overflowing.quote(&longer).is_none());
}
