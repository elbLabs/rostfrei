use crate::domain::rental_fleet::RegistrationNumber;

#[test]
fn preserves_the_opaque_value_and_transparent_json() {
    let number = RegistrationNumber::new("BIKE-42");

    assert_eq!(number.as_str(), "BIKE-42");
    assert_eq!(
        serde_json::to_string(&number).expect("registration number should serialize"),
        "\"BIKE-42\""
    );
    assert_eq!(
        serde_json::from_str::<RegistrationNumber>("\"BIKE-42\"")
            .expect("registration number should deserialize"),
        number
    );
}
