use serde_json::json;
use xtask::candidate::{prepare_bike_rental, verify_crates, verify_release, verify_rustls};

#[test]
fn same_version_cannot_hide_mixed_tool_or_crate_identity() {
    let revision = "a".repeat(40);
    let release = json!({"sourceRevision":revision,"sourceTreeDirty":false,"frameworkVersion":"0.0.5-alpha","kitVersion":1,
        "contracts":{"catalog":1,"behavioralTest":2,"messageSeries":2,"fixture":1},"skills":["rostfrei-develop-feature","rostfrei-tracer"]});
    verify_release(&release, &revision, "0.0.5-alpha").unwrap();
    for (field, value) in [
        ("sourceRevision", json!("b".repeat(40))),
        ("sourceTreeDirty", json!(true)),
        (
            "contracts",
            json!({"catalog":1,"behavioralTest":2,"messageSeries":1,"fixture":1}),
        ),
    ] {
        let mut changed = release.clone();
        changed[field] = value;
        assert!(verify_release(&changed, &revision, "0.0.5-alpha").is_err());
    }
    let temp = tempfile::tempdir().unwrap();
    let mut metadata = json!({"packages":[{"id":"rostfrei-core","name":"rostfrei-core","version":"0.0.5-alpha","source":format!("git+file:///candidate#{revision}"),"manifest_path":temp.path().join("outside/Cargo.toml")}]});
    verify_crates(
        &metadata,
        &temp.path().join("candidate"),
        &revision,
        "0.0.5-alpha",
    )
    .unwrap();
    for source in [
        json!(format!("git+file:///other#{}", "b".repeat(40))),
        json!(null),
    ] {
        metadata["packages"][0]["source"] = source;
        assert!(
            verify_crates(
                &metadata,
                &temp.path().join("candidate"),
                &revision,
                "0.0.5-alpha"
            )
            .unwrap_err()
            .to_string()
            .contains("rostfrei-core")
        );
    }
}

#[test]
fn external_recipe_expands_inheritance_and_pins_every_framework_dependency() {
    let temp = tempfile::tempdir().unwrap();
    let app = temp.path().join("app");
    let revision = "a".repeat(40);
    prepare_bike_rental(&xtask::root(), &app, &revision).unwrap();
    let manifest = xtask::versions::read(&app.join("Cargo.toml")).unwrap();
    assert!(manifest["package"]["version"].is_str());
    assert!(manifest["lints"].get("workspace").is_none());
    let mut count = 0;
    for section in ["dependencies", "dev-dependencies"] {
        for (name, dependency) in manifest[section].as_table().unwrap() {
            assert!(dependency.get("workspace").is_none());
            if dependency
                .get("package")
                .and_then(toml::Value::as_str)
                .unwrap_or(name)
                .starts_with("rostfrei")
            {
                assert_eq!(name, "rostfrei");
                count += 1;
                assert!(dependency.get("path").is_none());
                assert_eq!(dependency["rev"].as_str(), Some(revision.as_str()));
                assert_eq!(
                    dependency["git"].as_str(),
                    Some(format!("file://{}", xtask::root().display()).as_str())
                );
            }
        }
    }
    assert_eq!(count, 2, "one facade dependency in each dependency section");
    assert_eq!(
        manifest["dependencies"]["rostfrei"]["features"],
        toml::Value::try_from(vec!["nats", "tracer"]).unwrap()
    );
    assert_eq!(
        manifest["dev-dependencies"]["rostfrei"]["features"],
        toml::Value::try_from(vec!["testing"]).unwrap()
    );
    assert!(!app.join(".agents").exists());
    assert_eq!(
        std::fs::read(app.join("src/bin/bike-rental-api.rs")).unwrap(),
        std::fs::read(xtask::root().join("examples/bike-rental/src/bin/bike-rental-api.rs"))
            .unwrap()
    );
}

#[test]
fn unpatched_cli_lock_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let lock = temp.path().join("Cargo.lock");
    std::fs::write(&lock, "[[package]]\nname=\"rustls\"\nversion=\"0.23.43\"\n").unwrap();
    assert!(
        verify_rustls(&lock)
            .unwrap_err()
            .to_string()
            .contains("lockfile owner")
    );
    std::fs::write(&lock, "[[package]]\nname=\"rustls\"\nversion=\"0.23.45\"\n").unwrap();
    assert_eq!(verify_rustls(&lock).unwrap(), json!(["0.23.45"]));
}
