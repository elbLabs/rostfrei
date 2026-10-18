use std::{env, fs, path::Path, process::Command};

// Including the same inputs here also refreshes build provenance when a linked
// authoritative resource changes, even if no CLI Rust source changed.
#[path = "src/agent_tools/bundle_files.rs"]
mod bundle_files;
#[path = "src/agent_tools/source_identity.rs"]
mod source_identity;

fn git(root: &Path, arguments: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(root)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn main() {
    let _ = bundle_files::FILES;
    let _ = bundle_files::OBSERVED_SCHEMA;
    let manifest = env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default();
    let directory = Path::new(&manifest);
    println!("cargo:rerun-if-env-changed=ROSTFREI_SOURCE_STATE");
    // Cargo packages retain provenance without requiring Git at build/runtime.
    let vcs = directory.join(".cargo_vcs_info.json");
    println!("cargo:rerun-if-changed={}", vcs.display());
    let packaged = fs::read(&vcs)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
    let is_package = packaged.is_some();
    let (revision, dirty) = packaged.map_or_else(
        || source_metadata(directory),
        |value| {
            (
                value
                    .pointer("/git/sha1")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("unknown")
                    .to_owned(),
                value
                    .pointer("/git/dirty")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
            )
        },
    );
    println!("cargo:rustc-env=ROSTFREI_CLI_REVISION={revision}");
    println!("cargo:rustc-env=ROSTFREI_CLI_DIRTY={dirty}");
    let identity = if is_package {
        source_identity::package(directory)
    } else {
        source_identity::source(directory)
    };
    let fingerprint = match identity {
        Ok((fingerprint, inputs)) => {
            for input in inputs {
                println!("cargo:rerun-if-changed={}", input.display());
            }
            if dirty { fingerprint } else { String::new() }
        }
        Err(_) => String::new(), // Unknown dirty provenance is rejected by Release.
    };
    println!("cargo:rustc-env=ROSTFREI_CLI_FINGERPRINT={fingerprint}");
}

fn source_metadata(directory: &Path) -> (String, bool) {
    if git(
        directory,
        &["ls-files", "--error-unmatch", "--", "Cargo.toml"],
    )
    .is_some()
    {
        // Watch both the worktree HEAD and its branch ref, including linked worktrees.
        for name in [
            Some("HEAD".to_owned()),
            Some("index".to_owned()),
            git(directory, &["symbolic-ref", "HEAD"]),
        ]
        .into_iter()
        .flatten()
        {
            if let Some(path) = git(
                directory,
                &["rev-parse", "--path-format=absolute", "--git-path", &name],
            ) {
                println!("cargo:rerun-if-changed={path}");
            }
        }
        if let Some(root) = git(directory, &["rev-parse", "--show-toplevel"]) {
            // Source CLI changes must also refresh the recorded dirty-build flag.
            println!("cargo:rerun-if-changed={root}/crates/rostfrei-structure/src");
            println!("cargo:rerun-if-changed={root}/crates/rostfrei-structure/Cargo.toml");
        }
        (
            git(directory, &["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".to_owned()),
            // Cargo's Git-cache completion marker is not a source change.
            // Retain all other tracked and untracked changes in provenance.
            git(
                directory,
                &[
                    "status",
                    "--porcelain",
                    "--",
                    ":/",
                    ":(top,exclude).cargo-ok",
                ],
            )
            .is_none_or(|status| !status.is_empty()),
        )
    } else {
        ("unknown".to_owned(), true)
    }
}
