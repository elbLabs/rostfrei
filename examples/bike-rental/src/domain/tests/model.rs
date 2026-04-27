use serde_json::json;

#[test]
fn composed_domain_model_builds() {
    assert!(crate::domain::model::domain_model().is_ok());
}

#[test]
fn projects_the_fleet_invariant_and_bicycle_lifecycle() {
    let model = crate::domain::model::domain_model().expect("comparison domain model should build");
    assert_eq!(
        model["invariants"],
        json!([{
            "id": {
                "owner": {
                    "kind": "aggregate",
                    "id": {
                        "context": "bike-rental",
                        "local": "rental-fleet"
                    }
                },
                "local": "unique-bicycle-identities"
            },
            "label": "Bicycle identities are unique"
        }])
    );

    let bicycle = model["entities"]
        .as_array()
        .expect("entities should be an array")
        .iter()
        .find(|entity| entity["id"]["local"] == "bicycle")
        .expect("bicycle entity should be projected");
    assert_eq!(bicycle["lifecycle"]["id"], "rental-status");
    assert_eq!(
        bicycle["lifecycle"]["states"],
        json!([
            { "id": "available", "label": "Available" },
            { "id": "rented", "label": "Rented" }
        ])
    );
    assert_eq!(
        bicycle["lifecycle"]["transitions"]
            .as_array()
            .expect("lifecycle transitions should be an array")
            .len(),
        2
    );
}
