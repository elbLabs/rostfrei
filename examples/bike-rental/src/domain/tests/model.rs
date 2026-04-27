use serde_json::json;

#[test]
fn composed_domain_model_builds() {
    assert!(crate::domain::model::domain_model().is_ok());
}

#[test]
fn omits_unattached_aggregate_contracts_and_projects_the_bicycle_lifecycle() {
    let model = crate::domain::model::domain_model().expect("comparison domain model should build");
    assert!(
        model["actions"]
            .as_array()
            .expect("actions should be an array")
            .iter()
            .all(|action| action["id"]["owner"]["kind"] != "aggregate")
    );
    assert_eq!(model["decisions"], json!([]));
    assert_eq!(model["invariants"], json!([]));

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
