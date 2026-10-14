use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Component, Path},
};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::{ErrorKind, Phase, Release, Result, ToolError};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    sha256: BTreeMap<String, String>,
}

fn error(message: &str) -> ToolError {
    ToolError::new(ErrorKind::SetupFailed, Phase::Installation, message)
}

fn files(root: &Path) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut output = BTreeMap::new();
    visit(root, root, &mut output)?;
    Ok(output)
}

fn visit(root: &Path, path: &Path, output: &mut BTreeMap<String, Vec<u8>>) -> Result<()> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| error("Could not inspect kit/skill resources"))?;
    if metadata.is_symlink() {
        return Err(error(
            "Preserving symlink: kit and skill resources must be regular files/directories",
        ));
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(path).map_err(|_| error("Could not read kit/skill directory"))? {
            let entry = entry.map_err(|_| error("Could not read directory entry"))?;
            visit(root, &entry.path(), output)?;
        }
    } else if metadata.is_file() {
        let relative = path
            .strip_prefix(root)
            .map_err(|_| error("Resource escaped its root"))?;
        let key = relative
            .components()
            .map(|component| {
                if let Component::Normal(value) = component {
                    value.to_str()
                } else {
                    None
                }
            })
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| error("Invalid resource path"))?
            .join("/");
        if key.is_empty() {
            return Err(error("Expected a skill directory"));
        }
        output.insert(
            key,
            fs::read(path).map_err(|_| error("Could not read kit/skill file"))?,
        );
    } else {
        return Err(error("Preserving non-regular kit/skill resource"));
    }
    Ok(())
}

pub fn install_skills(kit: &Path, destination: &Path) -> Result<()> {
    let mut contents = files(kit)?;
    let manifest = contents
        .remove("manifest.json")
        .ok_or_else(|| error("Kit manifest is missing"))?;
    let manifest: Manifest =
        serde_json::from_slice(&manifest).map_err(|_| error("Invalid kit manifest"))?;
    let actual = contents
        .iter()
        .map(|(name, bytes)| (name.clone(), format!("{:x}", Sha256::digest(bytes))))
        .collect();
    if manifest.sha256 != actual {
        return Err(error(
            "Kit integrity check failed; extract a fresh verified archive",
        ));
    }
    let metadata = contents
        .get("release.json")
        .ok_or_else(|| error("Kit release metadata is missing"))?;
    let release: Release =
        serde_json::from_slice(metadata).map_err(|_| error("Invalid kit release metadata"))?;
    release.check(Phase::Installation)?;
    let mut pending = Vec::new();
    for name in &release.skills {
        let source = files(&kit.join("skills").join(name))?;
        if !source.contains_key("SKILL.md")
            || source.get("references/release.json") != Some(metadata)
        {
            return Err(error(
                "Skill resources are incomplete or not release-matched",
            ));
        }
        let target = destination.join(name);
        if target
            .try_exists()
            .map_err(|_| error("Could not inspect destination"))?
            || target.is_symlink()
        {
            if files(&target)? != source {
                return Err(error(
                    "Preserving existing modified skill; install elsewhere and merge deliberately",
                )
                .detail("path", target.display().to_string()));
            }
        } else {
            pending.push((target, source));
        }
    }
    // Preflight all conflicts before writing either skill. create_new also
    // preserves any consumer work created concurrently with this installation.
    fs::create_dir_all(destination).map_err(|_| error("Could not create destination directory"))?;
    for (target, source) in pending {
        fs::create_dir(&target)
            .map_err(|_| error("Preserving destination created since preflight"))?;
        for (name, bytes) in source {
            let file = target.join(name);
            if let Some(parent) = file.parent() {
                fs::create_dir_all(parent)
                    .map_err(|_| error("Could not create skill directory"))?;
            }
            fs::OpenOptions::new().write(true).create_new(true).open(file)
                .and_then(|mut file| file.write_all(&bytes))
                .map_err(|_| error("Could not install skill file; preserving partial installation for inspection"))?;
        }
    }
    println!(
        "Installed Rostfrei {} skills in {}",
        release.framework_version,
        destination.display()
    );
    Ok(())
}
