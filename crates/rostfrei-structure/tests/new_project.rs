use std::{fs, path::Path, process::Command};

use rostfrei_structure::{NewProjectError, check_domain_root, create_project};

#[test]
fn new_project_contains_the_complete_runnable_scaffold() {
    let temporary = tempfile::tempdir();
    assert!(temporary.is_ok(), "could not create temporary directory");
    let Some(temporary) = temporary.ok() else {
        return;
    };
    let destination = temporary.path().join("bike-rental");

    let created = create_project(&destination);
    assert!(created.is_ok(), "project creation failed: {created:?}");

    for relative_path in [
        ".gitignore",
        "Cargo.toml",
        "README.md",
        "clippy.toml",
        "compose.yaml",
        "nats-server.conf",
        "rust-toolchain.toml",
        "src/main.rs",
        "src/lib.rs",
        "src/bin/rostfrei-domain-check.rs",
        "src/domain/mod.rs",
        "src/domain/model.rs",
        "src/domain/bike_rental/mod.rs",
        "src/domain/bike_rental/context.rs",
        "src/domain/tests/mod.rs",
        "src/domain/tests/model.rs",
    ] {
        assert!(
            destination.join(relative_path).is_file(),
            "missing generated file `{relative_path}`"
        );
    }

    let manifest = read(&destination.join("Cargo.toml"));
    assert!(manifest.starts_with("[workspace]\nresolver = \"3\""));
    assert!(manifest.contains("name = \"bike-rental\""));
    assert!(manifest.contains("rostfrei-nats = \"0.1.0\""));
    assert!(manifest.contains("[lints.clippy]"));
    assert!(manifest.contains("arithmetic_side_effects = \"deny\""));

    let context = read(&destination.join("src/domain/bike_rental/context.rs"));
    assert!(context.contains("pub struct BikeRental;"));
    assert!(context.contains("id = \"bike-rental\""));
    assert!(context.contains("label = \"Bike Rental\""));

    let main = read(&destination.join("src/main.rs"));
    assert!(main.contains("ROSTFREI_NATS_URL"));
    assert!(main.contains("connect(&config).await?"));
    assert!(main.contains("connection.drain().await?"));

    let compose = read(&destination.join("compose.yaml"));
    assert!(compose.contains("image: nats:2.12-alpine"));
    assert!(compose.contains("127.0.0.1:4222:4222"));

    let diagnostics = check_domain_root(&destination.join("src/domain"));
    assert!(
        diagnostics.is_empty(),
        "generated domain structure is invalid: {diagnostics:#?}"
    );
}

#[test]
fn generated_module_declarations_are_formatted_for_later_context_names() {
    let temporary = tempfile::tempdir();
    assert!(temporary.is_ok(), "could not create temporary directory");
    let Some(temporary) = temporary.ok() else {
        return;
    };
    let destination = temporary.path().join("order-service");

    let created = create_project(&destination);
    assert!(created.is_ok(), "project creation failed: {created:?}");

    assert_eq!(
        read(&destination.join("src/domain/mod.rs")),
        "mod model;\nmod order_service;\n#[cfg(test)]\nmod tests;\n\npub use model::domain_model;\npub use order_service::OrderService;\n"
    );
}

#[test]
fn new_project_refuses_to_modify_an_existing_destination() {
    let temporary = tempfile::tempdir();
    assert!(temporary.is_ok(), "could not create temporary directory");
    let Some(temporary) = temporary.ok() else {
        return;
    };
    let destination = temporary.path().join("existing-project");
    let created = fs::create_dir(&destination);
    assert!(created.is_ok(), "could not create existing destination");
    let marker = destination.join("keep-me");
    let written = fs::write(&marker, "original");
    assert!(written.is_ok(), "could not write marker");

    let result = create_project(&destination);

    assert!(matches!(
        result,
        Err(NewProjectError::DestinationExists(path)) if path == destination
    ));
    assert_eq!(read(&marker), "original");
    let entries = fs::read_dir(&destination);
    assert!(entries.is_ok(), "could not inspect existing destination");
    assert_eq!(entries.map(Iterator::count).ok(), Some(1));
}

#[test]
fn cargo_new_command_generates_a_project() {
    let temporary = tempfile::tempdir();
    assert!(temporary.is_ok(), "could not create temporary directory");
    let Some(temporary) = temporary.ok() else {
        return;
    };
    let destination = temporary.path().join("customer-support");

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-rostfrei"))
        .args(["rostfrei", "new"])
        .arg(&destination)
        .output();
    assert!(output.is_ok(), "could not execute cargo-rostfrei");
    let Some(output) = output.ok() else {
        return;
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "new command failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.contains("Created Rostfrei project `customer-support`"));
    assert!(destination.join("Cargo.toml").is_file());
    assert!(
        destination
            .join("src/domain/customer_support/context.rs")
            .is_file()
    );
}

fn read(path: &Path) -> String {
    let contents = fs::read_to_string(path);
    assert!(
        contents.is_ok(),
        "could not read {}: {contents:?}",
        path.display()
    );
    contents.unwrap_or_default()
}
