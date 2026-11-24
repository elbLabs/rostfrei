use crate::domain::rental_fleet::{RegistrationNumber, RegistrationNumberValidity};

#[rostfrei::domain_invariant_test(
    <RegistrationNumber as RegistrationNumberValidity>::DESCRIPTOR
)]
fn rejects_non_normalized_registration_numbers() {
    let valid = RegistrationNumber::new("BIKE-42");
    let invalid = RegistrationNumber::new("bike 42");

    assert_eq!(valid.validate(), None);
    assert_eq!(
        invalid.validate(),
        Some(rostfrei::InvariantViolation::new(
            "registration_number",
            "must use uppercase letters and digits separated by single hyphens",
        ))
    );
}
