use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[test]
fn cargo_command_checks_the_workspace_end_to_end() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let result = Command::new(env!("CARGO_BIN_EXE_cargo-rostfrei"))
        .current_dir(&workspace_root)
        .args(["rostfrei", "check", "--workspace", "--manifest-path"])
        .arg(workspace_root.join("Cargo.toml"))
        .output();

    assert!(
        result.is_ok(),
        "could not execute cargo-rostfrei: {result:?}"
    );
    let Some(output) = result.ok() else {
        return;
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "cargo-rostfrei failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("rostfrei check passed (1 configured package(s))"),
        "unexpected stdout:\n{stdout}"
    );
}

#[test]
fn configured_package_without_check_target_emits_rf009() {
    let fixture = Fixture::new("missing-target", None);
    assert!(fixture.is_ok(), "could not create fixture: {fixture:?}");
    let Some(fixture) = fixture.ok() else {
        return;
    };
    let output = run_checker(&fixture.root);
    assert!(
        output.is_ok(),
        "could not execute cargo-rostfrei: {output:?}"
    );
    let Some(output) = output.ok() else {
        return;
    };
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success(), "checker unexpectedly passed");
    assert!(stderr.contains("RF009"), "unexpected stderr:\n{stderr}");
    assert!(
        stderr.contains("has no `rostfrei-domain-check` binary target"),
        "unexpected stderr:\n{stderr}"
    );
}

#[test]
fn failing_check_target_emits_rf010_and_preserves_output() {
    let binary = r#"
fn main() -> std::process::ExitCode {
    eprintln!("intentional compiled-domain failure");
    std::process::ExitCode::FAILURE
}
"#;
    let fixture = Fixture::new("failing-target", Some(binary));
    assert!(fixture.is_ok(), "could not create fixture: {fixture:?}");
    let Some(fixture) = fixture.ok() else {
        return;
    };
    let output = run_checker(&fixture.root);
    assert!(
        output.is_ok(),
        "could not execute cargo-rostfrei: {output:?}"
    );
    let Some(output) = output.ok() else {
        return;
    };
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success(), "checker unexpectedly passed");
    assert!(stderr.contains("RF010"), "unexpected stderr:\n{stderr}");
    assert!(
        stderr.contains("intentional compiled-domain failure"),
        "unexpected stderr:\n{stderr}"
    );
}

fn run_checker(workspace_root: &Path) -> io::Result<Output> {
    Command::new(env!("CARGO_BIN_EXE_cargo-rostfrei"))
        .current_dir(workspace_root)
        .args(["rostfrei", "check", "--workspace", "--manifest-path"])
        .arg(workspace_root.join("Cargo.toml"))
        .output()
}

#[derive(Debug)]
struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(name: &str, binary: Option<&str>) -> io::Result<Self> {
        let root =
            std::env::temp_dir().join(format!("rostfrei-structure-{}-{name}", std::process::id()));
        remove_existing_fixture(&root)?;
        std::fs::create_dir_all(root.join("src/domain/bike_rental"))?;
        std::fs::create_dir_all(root.join("src/domain/tests"))?;
        std::fs::write(root.join("Cargo.toml"), fixture_manifest(name))?;
        std::fs::write(root.join("src/lib.rs"), "")?;
        std::fs::write(
            root.join("src/domain/mod.rs"),
            "mod bike_rental;\nmod model;\n#[cfg(test)]\nmod tests;\n",
        )?;
        std::fs::write(
            root.join("src/domain/model.rs"),
            "fn domain_model() { domain_model! {} }\n",
        )?;
        std::fs::write(root.join("src/domain/bike_rental/mod.rs"), "mod context;\n")?;
        std::fs::write(
            root.join("src/domain/bike_rental/context.rs"),
            "#[derive(BoundedContext)]\npub struct BikeRental;\n",
        )?;
        std::fs::write(root.join("src/domain/tests/mod.rs"), "")?;
        if let Some(binary) = binary {
            std::fs::create_dir_all(root.join("src/bin"))?;
            std::fs::write(root.join("src/bin/rostfrei-domain-check.rs"), binary)?;
        }
        Ok(Self { root })
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn fixture_manifest(name: &str) -> String {
    format!(
        r#"[workspace]

[package]
name = "{name}"
version = "0.0.0"
edition = "2024"

[package.metadata.rostfrei.structure]
version = 1
domain-root = "src/domain"
"#
    )
}

fn remove_existing_fixture(path: &Path) -> io::Result<()> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}
