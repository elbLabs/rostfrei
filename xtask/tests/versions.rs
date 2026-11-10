use std::{fs, path::Path};

fn fixture() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        r#"[workspace]
members = ["crates/a", "crates/b"]
[workspace.package]
version = "1.2.3-alpha"
[workspace.dependencies]
a = { path = "crates/a", version = "1.2.3-alpha" }
alias = { package = "b", path = "crates/b", version = "1.2.3-alpha" }
external = "1.2.3-alpha"
"#,
    )
    .unwrap();
    for name in ["a", "b"] {
        let dir = temp.path().join("crates").join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("Cargo.toml"),
            format!("[package]\nname = {name:?}\nversion.workspace = true\n"),
        )
        .unwrap();
    }
    temp
}

fn replace(path: &Path, from: &str, to: &str) {
    fs::write(path, fs::read_to_string(path).unwrap().replace(from, to)).unwrap();
}

#[test]
fn versions_validate_tags_aliases_and_inheritance() {
    let temp = fixture();
    let root = temp.path();
    assert_eq!(
        xtask::versions::check(root, Some("v1.2.3-alpha")).unwrap(),
        "1.2.3-alpha"
    );
    for tag in ["v1.2.4-alpha", "1.2.3-alpha", "v1.2.3-alpha\n", "v1.2.3"] {
        assert!(xtask::versions::check(root, Some(tag)).is_err());
    }
    let manifest = root.join("Cargo.toml");
    replace(
        &manifest,
        "package = \"b\", path = \"crates/b\", version = \"1.2.3-alpha\"",
        "package = \"b\", path = \"crates/b\", version = \"1.2.2\"",
    );
    assert!(
        xtask::versions::check(root, None)
            .unwrap_err()
            .to_string()
            .contains("alias")
    );
    replace(&manifest, "1.2.2", "1.2.3-alpha");
    replace(
        &manifest,
        "path = \"crates/a\", version = \"1.2.3-alpha\"",
        "path = \"crates/a\"",
    );
    assert!(
        xtask::versions::check(root, None)
            .unwrap_err()
            .to_string()
            .contains("workspace.dependencies.a")
    );
    replace(
        &manifest,
        "path = \"crates/a\"",
        "path = \"crates/a\", version = \"1.2.3-alpha\"",
    );
    let member = root.join("crates/a/Cargo.toml");
    let original = fs::read_to_string(&member).unwrap();
    for declaration in [
        "\n[dev-dependencies]\nb = \"1.2.2\"\n",
        "\n[target.'cfg(unix)'.build-dependencies]\nrenamed = { package = \"b\", path = \"../b\" }\n",
    ] {
        fs::write(&member, format!("{original}{declaration}")).unwrap();
        assert!(
            xtask::versions::check(root, None)
                .unwrap_err()
                .to_string()
                .contains("inherit")
        );
    }
    for version in [
        "",
        "version = \"1.2.3-alpha\"",
        "version = { workspace = true, extra = true }",
    ] {
        fs::write(
            &member,
            original.replace("version.workspace = true", version),
        )
        .unwrap();
        assert!(xtask::versions::check(root, None).is_err());
    }
}

#[test]
fn version_bumps_reject_invalid_semver_before_writing() {
    let temp = fixture();
    let manifest = temp.path().join("Cargo.toml");
    let original = fs::read(&manifest).unwrap();
    for version in ["v1.2.3", "01.2.3", "1.2", "1.2.3-01", "1.2.3\n"] {
        assert!(
            xtask::versions::bump_with(temp.path(), version, |_| panic!(
                "invalid version reached lockfile update"
            ))
            .is_err()
        );
        assert_eq!(fs::read(&manifest).unwrap(), original);
    }
    for version in ["1.2.3", "1.2.3-rc.1", "1.2.3+build.01"] {
        xtask::versions::bump_with(temp.path(), version, |_| Ok(())).unwrap();
        assert_eq!(xtask::versions::check(temp.path(), None).unwrap(), version);
    }
}

#[test]
fn bump_preserves_external_versions_and_rolls_back_both_locks_on_failure() {
    let temp = fixture();
    let root = temp.path();
    let fixture = root.join(xtask::versions::FIXTURE);
    fs::create_dir_all(fixture.parent().unwrap()).unwrap();
    fs::write(&fixture, "[workspace]\nmembers = []\n").unwrap();
    let files = [
        root.join("Cargo.toml"),
        root.join("Cargo.lock"),
        fixture.with_file_name("Cargo.lock"),
    ];
    fs::write(&files[1], "root lock\n").unwrap();
    fs::write(&files[2], "fixture lock\n").unwrap();
    let before: Vec<_> = files.iter().map(|p| fs::read(p).unwrap()).collect();
    let mut count = 0;
    assert!(
        xtask::versions::bump_with(root, "2.0.0", |_| {
            count += 1;
            fs::write(&files[1], "partial")?;
            fs::write(&files[2], "partial")?;
            anyhow::ensure!(count < 2, "second update failed");
            Ok(())
        })
        .is_err()
    );
    assert_eq!(
        files
            .iter()
            .map(|p| fs::read(p).unwrap())
            .collect::<Vec<_>>(),
        before
    );
    xtask::versions::bump_with(root, "2.0.0-rc.1", |_| Ok(())).unwrap();
    assert_eq!(xtask::versions::check(root, None).unwrap(), "2.0.0-rc.1");
    assert!(
        fs::read_to_string(&files[0])
            .unwrap()
            .contains("external = \"1.2.3-alpha\"")
    );
}
