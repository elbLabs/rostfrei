//! Content identity for mutable source builds. Shared by setup and build.rs;
//! neither absolute checkout paths nor timestamps enter the fingerprint.
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

fn git(directory: &Path, args: &[&str]) -> io::Result<Vec<u8>> {
    let output = Command::new("git")
        .args(args)
        .current_dir(directory)
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other("Git source identity unavailable"));
    }
    Ok(output.stdout)
}

fn hash_files(
    root: &Path,
    names: BTreeSet<String>,
    kind: &[u8],
) -> io::Result<(String, Vec<PathBuf>)> {
    let mut hash = Sha256::new();
    hash.update(kind);
    let mut inputs = Vec::new();
    for name in names {
        let path = root.join(&name);
        hash.update(
            u64::try_from(name.len())
                .map_err(io::Error::other)?
                .to_le_bytes(),
        );
        hash.update(name.as_bytes());
        match fs::read(&path) {
            Ok(bytes) => {
                hash.update([1]);
                hash.update(
                    u64::try_from(bytes.len())
                        .map_err(io::Error::other)?
                        .to_le_bytes(),
                );
                hash.update(bytes);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => hash.update([0]),
            Err(error) => return Err(error),
        }
        inputs.push(path);
    }
    Ok((format!("{:x}", hash.finalize()), inputs))
}

pub fn source(directory: &Path) -> io::Result<(String, Vec<PathBuf>)> {
    let root = String::from_utf8(git(directory, &["rev-parse", "--show-toplevel"])?)
        .map_err(io::Error::other)?;
    let root = Path::new(root.trim());
    let names = String::from_utf8(git(
        root,
        &[
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ],
    )?)
    .map_err(io::Error::other)?;
    hash_files(
        root,
        names
            .split('\0')
            .filter(|name| !name.is_empty() && *name != ".cargo-ok")
            .map(str::to_owned)
            .collect(),
        b"rostfrei-source-v1",
    )
}

// A dirty Cargo package is its own content identity; its removed source tree
// cannot be inferred from a dirty boolean. The package carries all these inputs.
#[allow(dead_code, reason = "package identity is used by build.rs only")]
pub fn package(directory: &Path) -> io::Result<(String, Vec<PathBuf>)> {
    fn collect(root: &Path, path: &Path, names: &mut BTreeSet<String>) -> io::Result<()> {
        if path.is_dir() {
            for entry in fs::read_dir(path)? {
                collect(root, &entry?.path(), names)?;
            }
        } else if path.is_file() {
            names.insert(
                path.strip_prefix(root)
                    .map_err(io::Error::other)?
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
        Ok(())
    }
    let mut names = BTreeSet::new();
    for name in [
        "src",
        "templates",
        "resources",
        "Cargo.toml",
        "Cargo.lock",
        "build.rs",
    ] {
        collect(directory, &directory.join(name), &mut names)?;
    }
    hash_files(directory, names, b"rostfrei-package-v1")
}
