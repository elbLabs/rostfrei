use serde_json::json;

#[test]
fn composed_domain_model_builds() {
    assert!(crate::domain::model::domain_model().is_ok());
}

#[test]
fn projects_registered_command_field_declarations_without_inventing_links() {
    let bindings = crate::tracer::command_bindings().expect("commands should register");
    let command = bindings
        .registry()
        .command("bike-rental", "reserve-bicycle-category", 1)
        .expect("reservation command should be registered");
    let fields = command.project_modeled_fields();
    assert_eq!(
        fields.iter().find(|field| field["name"] == "category"),
        Some(&json!({
            "name": "category",
            "declaredType": "BicycleCategory",
            "value": { "kind": "opaque" }
        }))
    );
}

#[test]
fn projects_registered_services_and_omits_unattached_capabilities() {
    let model = crate::domain::model::domain_model().expect("comparison domain model should build");
    assert_eq!(model["actions"], json!([]));
    assert_eq!(
        model["domainServices"],
        json!([{
            "id": { "context": "bike-rental", "local": "bicycle-transfer" },
            "label": "Bicycle transfer"
        }])
    );
    assert_eq!(model["policies"], json!([]));
    assert_eq!(model["invariants"], json!([]));
    assert!(model.get("commands").is_none());
    let identities = model["domainIdentities"]
        .as_array()
        .expect("domain identities should be an array");
    assert_eq!(identities.len(), 6);
    assert!(
        identities
            .iter()
            .all(|identity| identity.get("scalar").is_none())
    );
    assert_eq!(identities[0]["id"]["owner"]["local"], "rental-fleet-root");
    assert_eq!(identities[1]["id"]["owner"]["local"], "bicycle");
    assert_eq!(identities[2]["id"]["owner"]["local"], "reservation");
    assert_eq!(identities[3]["id"]["owner"]["local"], "repair-job-root");
    assert_eq!(identities[4]["id"]["owner"]["local"], "rental-tariff-root");
    assert_eq!(identities[5]["id"]["owner"]["local"], "rental-payment-root");

    assert_eq!(
        model["valueObjects"],
        json!([
            { "id": "bicycle-condition", "label": "Bicycle condition" },
            { "id": "registration-number", "label": "Registration number" },
            {
                "id": "bicycle-transfer-rejection-reason",
                "label": "Bicycle transfer rejection reason"
            },
            { "id": "repair-details", "label": "Repair details" },
            { "id": "bicycle-category", "label": "Bicycle category" },
            { "id": "rental-period", "label": "Rental period" },
            { "id": "rental-tariff-terms", "label": "Rental tariff terms" },
            { "id": "rental-quote", "label": "Rental quote" },
            { "id": "collection-payment", "label": "Collection payment" },
            { "id": "collection-assignment", "label": "Collection assignment" },
            { "id": "payment-outcome", "label": "Payment outcome" },
            { "id": "rental-return", "label": "Rental return" },
            { "id": "return-time", "label": "Return time" }
        ])
    );

    assert_eq!(model["queries"], json!([]));
    assert_eq!(model["domainErrors"][2]["id"], "bicycle-already-in-fleet");
    assert_eq!(model["domainErrors"][2]["code"], "BICYCLE_ALREADY_IN_FLEET");
    assert_eq!(model["domainErrors"][3]["id"], "bicycle-cannot-be-retired");
    assert_eq!(model["domainErrors"][4]["id"], "bicycle-transfer-rejected");
    assert_eq!(
        model["domainErrors"][4]["code"],
        "BICYCLE_TRANSFER_REJECTED"
    );

    let bicycle = model["entities"]
        .as_array()
        .expect("entities should be an array")
        .iter()
        .find(|entity| entity["id"]["local"] == "bicycle")
        .expect("bicycle entity should be projected");
    assert!(bicycle["identity"].get("field").is_none());
    assert_eq!(bicycle["identity"]["owner"]["local"], "bicycle");
    assert_eq!(
        bicycle["fields"],
        json!([
            {"name": "bicycle_id", "declaredType": "BicycleId", "value": {"kind": "opaque"}},
            {"name": "status", "declaredType": "BicycleStatus", "value": {"kind": "opaque"}},
            {"name": "condition", "declaredType": "BicycleCondition", "value": {"kind": "opaque"}},
            {"name": "category", "declaredType": "Option<BicycleCategory>", "value": {"kind": "optional", "value": {"kind": "opaque"}}}
        ])
    );
    assert_eq!(bicycle["lifecycle"], json!(null));
}
