use crate::domain::rental_fleet::BicycleStatus;

#[test]
fn preserves_status_wire_values_after_module_migration() {
    assert_eq!(
        serde_json::to_string(&BicycleStatus::Available).expect("status should serialize"),
        "\"available\""
    );
    assert_eq!(
        serde_json::from_str::<BicycleStatus>("\"rented\"").expect("status should deserialize"),
        BicycleStatus::Rented
    );
}
