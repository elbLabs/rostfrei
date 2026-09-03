use crate::domain::rental_fleet::{NormalizeRegistrationNumber, RegistrationNumber};

#[rostfrei::domain_action_test(
    <RegistrationNumber as NormalizeRegistrationNumber>::DESCRIPTOR
)]
fn normalizes_spacing_and_letter_case() {
    let number = RegistrationNumber::new("  bike   42 ");

    assert_eq!(number.normalize(), RegistrationNumber::new("BIKE-42"));
}
