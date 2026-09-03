use crate::domain::rental_fleet::{
    ChooseRegistrationNumberFormat, RegistrationNumber, RegistrationNumberFormat,
};

#[rostfrei::domain_decision_test(
    <RegistrationNumber as ChooseRegistrationNumberFormat>::DESCRIPTOR
)]
fn distinguishes_compact_and_segmented_formats() {
    assert_eq!(
        RegistrationNumber::new("BIKE42").choose_format(),
        RegistrationNumberFormat::Compact
    );
    assert_eq!(
        RegistrationNumber::new("BIKE-42").choose_format(),
        RegistrationNumberFormat::Segmented
    );
}
