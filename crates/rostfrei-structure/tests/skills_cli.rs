#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "bounded installer fixture assertions"
)]

use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    process::{Command, Output},
};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

const NAMES: [&str; 2] = ["rostfrei-develop-feature", "rostfrei-tracer"];

fn write(root: &Path, name: &str, bytes: &[u8]) {
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn fixture(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let info = Command::new(env!("CARGO_BIN_EXE_cargo-rostfrei"))
        .args(["rostfrei", "skills", "info"])
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(info.status.success());
    let mut files = BTreeMap::from([("release.json".to_owned(), info.stdout.clone())]);
    for name in NAMES {
        files.insert(
            format!("skills/{name}/SKILL.md"),
            format!("---\nname: {name}\ndescription: Test fixture\n---\nInstructions\n")
                .into_bytes(),
        );
        files.insert(
            format!("skills/{name}/references/release.json"),
            info.stdout.clone(),
        );
    }
    seal(root, &files);
    files
}

fn seal(root: &Path, files: &BTreeMap<String, Vec<u8>>) {
    for (name, bytes) in files {
        write(root, name, bytes);
    }
    let hashes: BTreeMap<_, _> = files
        .iter()
        .map(|(name, data)| (name, format!("{:x}", Sha256::digest(data))))
        .collect();
    write(
        root,
        "manifest.json",
        json!({"sha256": hashes}).to_string().as_bytes(),
    );
}

fn install(root: &Path, destination: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cargo-rostfrei"))
        .args(["rostfrei", "skills", "install", "--kit"])
        .arg(root)
        .arg("--destination")
        .arg(destination)
        .env("PATH", "")
        .output()
        .unwrap()
}

fn update(root: &Path, destination: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cargo-rostfrei"))
        .args(["rostfrei", "skills", "update", "--kit"])
        .arg(root)
        .arg("--destination")
        .arg(destination)
        .env("PATH", "")
        .output()
        .unwrap()
}

#[cfg(unix)]
#[test]
fn updates_preserve_private_directories_and_skill_root_modes() {
    use std::os::unix::fs::PermissionsExt as _;
    let temporary = TempDir::new().unwrap();
    let kit = temporary.path().join("kit");
    let destination = temporary.path().join("skills");
    let mut resources = fixture(&kit);
    assert!(install(&kit, &destination).status.success());
    write(
        &destination,
        "rostfrei-tracer/private/nested/notes",
        b"private consumer notes",
    );
    let directories = [
        "rostfrei-tracer",
        "rostfrei-tracer/private",
        "rostfrei-tracer/private/nested",
    ];
    for directory in directories {
        fs::set_permissions(
            destination.join(directory),
            fs::Permissions::from_mode(0o700),
        )
        .unwrap();
    }
    resources.insert(
        "skills/rostfrei-develop-feature/references/new.md".to_owned(),
        b"upstream".to_vec(),
    );
    seal(&kit, &resources);
    for _ in 0..2 {
        let result = update(&kit, &destination);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        for directory in directories {
            assert_eq!(
                fs::metadata(destination.join(directory))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o7777,
                0o700,
                "{directory}"
            );
        }
        assert_eq!(
            fs::read(destination.join("rostfrei-tracer/private/nested/notes")).unwrap(),
            b"private consumer notes"
        );
    }
}

#[test]
fn managed_directory_file_transitions_preserve_consumer_collisions_atomically() {
    for collision in [
        None,
        Some("notes"),
        Some("empty-directory"),
        Some("edited-managed"),
    ] {
        let temporary = TempDir::new().unwrap();
        let kit = temporary.path().join("kit");
        let destination = temporary.path().join("skills");
        let mut resources = fixture(&kit);
        let old = "skills/rostfrei-tracer/references/guide/intro.md";
        resources.insert(old.to_owned(), b"old managed guide".to_vec());
        seal(&kit, &resources);
        assert!(install(&kit, &destination).status.success());
        let guide = destination.join("rostfrei-tracer/references/guide");
        let first = destination.join("rostfrei-develop-feature/SKILL.md");
        let original_first = fs::read(&first).unwrap();
        let baseline = fs::read(destination.join(".rostfrei-managed.json")).unwrap();
        match collision {
            Some("notes") => fs::write(guide.join("notes"), "user notes").unwrap(),
            Some("empty-directory") => fs::create_dir(guide.join("empty-directory")).unwrap(),
            Some("edited-managed") => fs::write(guide.join("intro.md"), "user edit").unwrap(),
            _ => {}
        }
        resources.remove(old);
        fs::remove_dir_all(kit.join("skills/rostfrei-tracer/references/guide")).unwrap();
        resources.insert(
            "skills/rostfrei-tracer/references/guide".to_owned(),
            b"consolidated guide".to_vec(),
        );
        resources.insert(
            "skills/rostfrei-develop-feature/SKILL.md".to_owned(),
            b"upstream first skill".to_vec(),
        );
        seal(&kit, &resources);
        let result = update(&kit, &destination);
        if collision.is_some() {
            assert!(!result.status.success());
            assert!(String::from_utf8_lossy(&result.stderr).contains("Preserving"));
            assert_eq!(fs::read(&first).unwrap(), original_first);
            assert_eq!(
                fs::read(destination.join(".rostfrei-managed.json")).unwrap(),
                baseline
            );
            assert!(
                guide
                    .join(
                        collision
                            .filter(|name| *name != "edited-managed")
                            .unwrap_or("intro.md")
                    )
                    .exists()
            );
        } else {
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(fs::read(&guide).unwrap(), b"consolidated guide");
            // The reverse managed file -> directory transition is also legitimate.
            fs::remove_file(kit.join("skills/rostfrei-tracer/references/guide")).unwrap();
            resources.remove("skills/rostfrei-tracer/references/guide");
            resources.insert(old.to_owned(), b"split guide again".to_vec());
            seal(&kit, &resources);
            assert!(update(&kit, &destination).status.success());
            assert_eq!(
                fs::read(guide.join("intro.md")).unwrap(),
                b"split guide again"
            );
        }
    }
}

#[test]
fn relocatable_install_is_idempotent_and_preflights_all_consumer_conflicts() {
    let temporary = TempDir::new().unwrap();
    let kit = temporary.path().join("extracted");
    fixture(&kit);
    let relocated = temporary.path().join("moved kit");
    fs::rename(kit, &relocated).unwrap();
    let consumer = temporary.path().join("consumer/.agents/skills");
    write(&consumer, "other-skill/SKILL.md", b"unrelated work");
    let result = install(&relocated, &consumer);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(install(&relocated, &consumer).status.success());
    write(&consumer, "rostfrei-tracer/SKILL.md", b"consumer edits");
    fs::remove_dir_all(consumer.join("rostfrei-develop-feature")).unwrap();
    assert!(!install(&relocated, &consumer).status.success());
    assert!(!consumer.join("rostfrei-develop-feature").exists());
    assert_eq!(
        fs::read(consumer.join("rostfrei-tracer/SKILL.md")).unwrap(),
        b"consumer edits"
    );
    assert_eq!(
        fs::read(consumer.join("other-skill/SKILL.md")).unwrap(),
        b"unrelated work"
    );
}

#[test]
fn corrupted_kit_is_rejected_before_creating_destination() {
    let temporary = TempDir::new().unwrap();
    let kit = temporary.path().join("kit");
    fixture(&kit);
    write(&kit, "skills/rostfrei-tracer/SKILL.md", b"corrupted");
    let destination = temporary.path().join("consumer");
    assert!(!install(&kit, &destination).status.success());
    assert!(!destination.exists());
}

#[test]
fn valid_manifest_does_not_make_a_different_revision_compatible() {
    let temporary = TempDir::new().unwrap();
    let kit = temporary.path().join("kit");
    let mut files = fixture(&kit);
    let mut release: Value = serde_json::from_slice(&files["release.json"]).unwrap();
    release["sourceRevision"] = Value::String("0".repeat(40));
    let bytes = serde_json::to_vec(&release).unwrap();
    files.insert("release.json".to_owned(), bytes.clone());
    for name in NAMES {
        files.insert(
            format!("skills/{name}/references/release.json"),
            bytes.clone(),
        );
    }
    seal(&kit, &files);
    let destination = temporary.path().join("consumer");
    let result = install(&kit, &destination);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("release or protocol mismatch"));
    assert!(!destination.exists());
    let result = Command::new(env!("CARGO_BIN_EXE_cargo-rostfrei"))
        .args(["rostfrei", "skills", "install", "--kit"])
        .arg(&kit)
        .current_dir(temporary.path())
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("release or protocol mismatch"));
    assert!(!temporary.path().join(".agents").exists());
}

#[cfg(unix)]
#[test]
fn destination_symlinks_preserve_their_target() {
    let temporary = TempDir::new().unwrap();
    let kit = temporary.path().join("kit");
    fixture(&kit);
    let destination = temporary.path().join("consumer");
    fs::create_dir(&destination).unwrap();
    let work = temporary.path().join("user-work");
    fs::create_dir(&work).unwrap();
    std::os::unix::fs::symlink(&work, destination.join("rostfrei-tracer")).unwrap();
    assert!(!install(&kit, &destination).status.success());
    assert_eq!(fs::read_dir(work).unwrap().count(), 0);
    assert!(!destination.join("rostfrei-develop-feature").exists());
}

#[test]
fn explicit_update_is_transactional_and_preserves_user_additions() {
    let temporary = TempDir::new().unwrap();
    let kit = temporary.path().join("kit");
    let destination = temporary.path().join("skills");
    let mut resources = fixture(&kit);
    resources.insert(
        "skills/rostfrei-tracer/references/obsolete.md".to_owned(),
        b"old managed resource".to_vec(),
    );
    seal(&kit, &resources);
    assert!(install(&kit, &destination).status.success());
    let baseline = fs::read(destination.join(".rostfrei-managed.json")).unwrap();
    let first = destination.join("rostfrei-develop-feature/SKILL.md");
    let original_first = fs::read(&first).unwrap();
    let second = destination.join("rostfrei-tracer/SKILL.md");
    let original_second = fs::read(&second).unwrap();
    fs::write(&second, "local edit").unwrap();
    write(
        &destination,
        "rostfrei-tracer/user-notes.md",
        b"unmanaged notes",
    );
    resources.insert(
        "skills/rostfrei-develop-feature/SKILL.md".to_owned(),
        b"upstream change".to_vec(),
    );
    resources.insert(
        "skills/rostfrei-tracer/SKILL.md".to_owned(),
        b"upstream second change".to_vec(),
    );
    resources.remove("skills/rostfrei-tracer/references/obsolete.md");
    fs::remove_file(kit.join("skills/rostfrei-tracer/references/obsolete.md")).unwrap();
    resources.insert(
        "skills/rostfrei-tracer/references/added.md".to_owned(),
        b"new managed resource".to_vec(),
    );
    seal(&kit, &resources);
    let update = || {
        Command::new(env!("CARGO_BIN_EXE_cargo-rostfrei"))
            .args(["rostfrei", "skills", "update", "--kit"])
            .arg(&kit)
            .arg("--destination")
            .arg(&destination)
            .env("PATH", "")
            .output()
            .unwrap()
    };
    let rejected = update();
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("Preserving"));
    assert_eq!(fs::read(&first).unwrap(), original_first);
    assert_eq!(fs::read(&second).unwrap(), b"local edit");
    assert_eq!(
        fs::read(destination.join(".rostfrei-managed.json")).unwrap(),
        baseline
    );
    assert!(
        destination
            .join("rostfrei-tracer/references/obsolete.md")
            .exists()
    );
    fs::write(&second, original_second).unwrap();
    let updated = update();
    assert!(
        updated.status.success(),
        "{}",
        String::from_utf8_lossy(&updated.stderr)
    );
    assert_eq!(fs::read(first).unwrap(), b"upstream change");
    assert!(
        !destination
            .join("rostfrei-tracer/references/obsolete.md")
            .exists()
    );
    assert_eq!(
        fs::read(destination.join("rostfrei-tracer/references/added.md")).unwrap(),
        b"new managed resource"
    );
    assert_eq!(
        fs::read(destination.join("rostfrei-tracer/user-notes.md")).unwrap(),
        b"unmanaged notes"
    );
    assert!(update().status.success());
    assert!(install(&kit, &destination).status.success());
}
