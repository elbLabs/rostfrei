use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path},
};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::{
    ErrorKind, Phase, Release, Result, ToolError, bundle_files, release::SKILLS, transaction,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    sha256: BTreeMap<String, String>,
}

pub(super) fn error(message: &str) -> ToolError {
    ToolError::new(ErrorKind::SetupFailed, Phase::Installation, message)
}

pub(super) fn files(root: &Path) -> Result<BTreeMap<String, Vec<u8>>> {
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

pub(super) struct Kit {
    pub release: Release,
    pub files: BTreeMap<String, Vec<u8>>,
}

pub(super) fn bundled() -> Result<Kit> {
    let release = Release::current();
    let metadata = serde_json::to_vec_pretty(&release)
        .map_err(|_| error("Could not encode release metadata"))?;
    let mut files: BTreeMap<_, _> = bundle_files::FILES
        .iter()
        .map(|(name, bytes)| ((*name).to_owned(), bytes.to_vec()))
        .collect();
    for name in SKILLS {
        files.insert(format!("{name}/references/release.json"), metadata.clone());
    }
    Ok(Kit { release, files })
}

pub(super) fn external(kit: &Path) -> Result<Kit> {
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
    // Reject unexpected skill names before interpreting paths from the document.
    if release.skills != SKILLS {
        return Err(error("Unsupported kit skill set"));
    }
    let mut resources = BTreeMap::new();
    for name in &release.skills {
        let prefix = format!("skills/{name}/");
        let source: BTreeMap<_, _> = contents
            .iter()
            .filter_map(|(path, bytes)| {
                path.strip_prefix(&prefix)
                    .map(|relative| (relative.to_owned(), bytes.clone()))
            })
            .collect();
        if !source.contains_key("SKILL.md")
            || source.get("references/release.json") != Some(metadata)
        {
            return Err(error(
                "Skill resources are incomplete or not release-matched",
            ));
        }
        for (path, bytes) in source {
            resources.insert(format!("{name}/{path}"), bytes);
        }
    }
    Ok(Kit {
        release,
        files: resources,
    })
}

pub fn install_skills(kit: Option<&Path>, destination: &Path, update: bool) -> Result<()> {
    let kit = kit.map_or_else(bundled, external)?;
    kit.release.check(Phase::Installation)?;
    transaction::apply(&kit, destination, update)?;
    println!(
        "Installed Rostfrei {} skills in {}",
        kit.release.framework_version,
        destination.display()
    );
    Ok(())
}
