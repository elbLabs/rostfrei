use crate::domain::rental_fleet::{
    ChooseRegistrationNumberFormatPolicy, RegistrationNumber, RegistrationNumberFormat,
};

#[rostfrei::domain_policy_test(
    <RegistrationNumber as ChooseRegistrationNumberFormatPolicy>::DESCRIPTOR
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
