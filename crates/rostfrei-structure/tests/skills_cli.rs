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
