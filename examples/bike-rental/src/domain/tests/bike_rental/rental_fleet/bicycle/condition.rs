use crate::domain::rental_fleet::BicycleCondition;

#[test]
fn preserves_condition_wire_values_after_module_migration() {
    assert_eq!(
        serde_json::to_string(&BicycleCondition::Serviceable).expect("condition should serialize"),
        "\"serviceable\""
    );
    assert_eq!(
        serde_json::from_str::<BicycleCondition>("\"maintenance-required\"")
            .expect("condition should deserialize"),
        BicycleCondition::MaintenanceRequired
    );
}
