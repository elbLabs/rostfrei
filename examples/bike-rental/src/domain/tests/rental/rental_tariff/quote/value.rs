use crate::domain::rental_tariff::RentalQuote;

#[test]
fn collection_totals_require_a_known_deposit_and_cannot_wrap() {
    let quote = RentalQuote {
        currency: "EUR".to_owned(),
        rental_charge_minor: 2500,
        security_deposit_minor: Some(10000),
    };
    assert_eq!(quote.total_due_minor(), Some(12500));
    assert_eq!(
        RentalQuote {
            security_deposit_minor: None,
            ..quote.clone()
        }
        .total_due_minor(),
        None
    );
    assert_eq!(
        RentalQuote {
            security_deposit_minor: Some(u64::MAX),
            ..quote
        }
        .total_due_minor(),
        None
    );
}
