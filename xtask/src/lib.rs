pub mod acceptance;
pub mod candidate;
pub mod capabilities;
pub mod live;
pub mod nats;
pub mod process;
pub mod skills;
pub mod versions;

use anyhow::{Context, Result, ensure};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

pub type Environment = BTreeMap<String, String>;

pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace parent")
        .to_path_buf()
}

pub fn environment() -> Environment {
    std::env::vars().collect()
}
pub fn nonce() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}
pub fn read_json(path: &Path) -> Result<Value> {
    serde_json::from_slice(&fs::read(path)?).with_context(|| path.display().to_string())
}
pub fn save_json(path: &Path, value: &Value) -> Result<()> {
    fs::write(path, format!("{}\n", serde_json::to_string_pretty(value)?))?;
    Ok(())
}
pub fn digest(path: &Path) -> Result<String> {
    Ok(hash(&fs::read(path)?))
}
pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn text(value: &Value) -> Result<&str> {
    value.as_str().context("expected string")
}
pub fn array(value: &Value) -> Result<&Vec<Value>> {
    value.as_array().context("expected array")
}
pub fn absolute(path: &Path) -> Result<PathBuf> {
    let path = std::path::absolute(path)?;
    if path.exists() {
        return Ok(path.canonicalize()?);
    }
    let parent = path.parent().context("missing parent")?;
    Ok(absolute(parent)?.join(path.file_name().context("missing filename")?))
}
pub fn target() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root().join("target"))
}
pub fn cli() -> PathBuf {
    std::env::var_os("ROSTFREI_CLI")
        .map(PathBuf::from)
        .unwrap_or_else(|| target().join("debug/cargo-rostfrei"))
}
pub fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    for entry in walkdir::WalkDir::new(source)
        .into_iter()
        .filter_entry(|e| e.file_name() != "target")
    {
        let entry = entry?;
        let to = destination.join(entry.path().strip_prefix(source)?);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&to)?;
        } else if entry.file_type().is_symlink() {
            #[cfg(unix)]
            std::os::unix::fs::symlink(fs::read_link(entry.path())?, to)?;
        } else {
            fs::copy(entry.path(), to)?;
        }
    }
    Ok(())
}
pub fn files(path: &Path) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut result = BTreeMap::new();
    for entry in walkdir::WalkDir::new(path) {
        let entry = entry?;
        ensure!(
            !entry.file_type().is_symlink(),
            "symlink in {}",
            entry.path().display()
        );
        if entry.file_type().is_file() {
            result.insert(
                entry
                    .path()
                    .strip_prefix(path)?
                    .to_string_lossy()
                    .into_owned(),
                fs::read(entry.path())?,
            );
        }
    }
    Ok(result)
}
pub fn only_file(path: &Path, suffix: &str) -> Result<PathBuf> {
    let matches = fs::read_dir(path)?
        .collect::<std::io::Result<Vec<_>>>()?
        .into_iter()
        .map(|e| e.path())
        .filter(|p| p.to_string_lossy().ends_with(suffix))
        .collect::<Vec<_>>();
    ensure!(
        matches.len() == 1,
        "expected one {suffix} in {}",
        path.display()
    );
    Ok(matches[0].clone())
}
