use serde_json::json;

#[test]
fn composed_domain_model_builds() {
    assert!(crate::domain::model::domain_model().is_ok());
}

#[test]
fn omits_unattached_capabilities_and_lifecycle_metadata() {
    let model = crate::domain::model::domain_model().expect("comparison domain model should build");
    assert_eq!(model["actions"], json!([]));
    assert_eq!(model["domainServices"], json!([]));
    assert_eq!(model["decisions"], json!([]));
    assert_eq!(model["invariants"], json!([]));
    assert!(model.get("commands").is_none());
    let identities = model["domainIdentities"]
        .as_array()
        .expect("domain identities should be an array");
    assert_eq!(identities.len(), 2);
    assert!(
        identities
            .iter()
            .all(|identity| identity.get("scalar").is_none())
    );
    assert_eq!(identities[0]["id"]["owner"]["local"], "rental-fleet-root");
    assert_eq!(identities[1]["id"]["owner"]["local"], "bicycle");

    assert_eq!(
        model["valueObjects"],
        json!([
            { "id": "bicycle-status", "label": "Bicycle status" },
            { "id": "bicycle-condition", "label": "Bicycle condition" }
        ])
    );

    let availability = model["queries"]
        .as_array()
        .expect("queries should be an array")
        .iter()
        .find(|query| query["id"]["local"] == "bicycle-availability")
        .expect("bicycle availability query should be projected");
    assert!(availability.get("input").is_none());
    assert!(availability.get("output").is_none());

    let bicycle = model["entities"]
        .as_array()
        .expect("entities should be an array")
        .iter()
        .find(|entity| entity["id"]["local"] == "bicycle")
        .expect("bicycle entity should be projected");
    assert_eq!(bicycle["lifecycle"], json!(null));
}
