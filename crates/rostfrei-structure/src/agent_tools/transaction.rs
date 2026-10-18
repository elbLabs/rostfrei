use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
    Release, Result,
    install::{Kit, error, files},
    release::SKILLS,
};

const STATE: &str = ".rostfrei-managed.json";

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Baseline {
    state_version: u32,
    release: Release,
    sha256: BTreeMap<String, String>,
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn conflict(path: &str) -> super::ToolError {
    error("Preserving modified or unowned skill resource; resolve the conflict before updating")
        .detail("path", path)
}

fn snapshot(destination: &Path) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut result = BTreeMap::new();
    for name in SKILLS {
        let path = destination.join(name);
        if path.exists() || path.is_symlink() {
            for (relative, bytes) in files(&path)? {
                result.insert(format!("{name}/{relative}"), bytes);
            }
        }
    }
    Ok(result)
}

fn state_bytes(destination: &Path) -> Result<Option<Vec<u8>>> {
    let path = destination.join(STATE);
    if path.is_symlink() {
        return Err(conflict(STATE));
    }
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(conflict(STATE)),
    }
}

fn validate_state(state: &Baseline) -> Result<()> {
    if state.state_version != 1 || state.release.skills != SKILLS {
        return Err(error("Unsupported managed skills baseline"));
    }
    for (name, digest) in &state.sha256 {
        if !SKILLS
            .iter()
            .any(|skill| name.starts_with(&format!("{skill}/")))
            || Path::new(name)
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
            || name.contains('\\')
            || digest.len() != 64
            || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(error("Invalid managed resource baseline"));
        }
    }
    Ok(())
}

struct Plan {
    files: BTreeMap<String, Vec<u8>>,
    managed_directories: BTreeSet<String>,
}

fn plan(
    kit: &Kit,
    current: &BTreeMap<String, Vec<u8>>,
    previous: Option<&[u8]>,
    update: bool,
) -> Result<Plan> {
    let mut desired = current.clone();
    let mut managed_directories = BTreeSet::new();
    if let Some(bytes) = previous {
        let state: Baseline = serde_json::from_slice(bytes)
            .map_err(|_| error("Invalid managed skills baseline; preserving all resources"))?;
        validate_state(&state)?;
        for path in state.sha256.keys() {
            parents(path, &mut managed_directories);
        }
        let incoming: BTreeMap<_, _> = kit
            .files
            .iter()
            .map(|(path, bytes)| (path.clone(), hash(bytes)))
            .collect();
        if !update && (state.release != kit.release || state.sha256 != incoming) {
            return Err(error(
                "Installed skills target a different build; run cargo rostfrei skills update",
            ));
        }
        for (path, old_hash) in &state.sha256 {
            let actual = current.get(path);
            let next = kit.files.get(path);
            if actual != next && actual.is_none_or(|bytes| hash(bytes) != *old_hash) {
                return Err(conflict(path));
            }
            if next.is_none() {
                desired.remove(path);
            }
        }
        for (path, bytes) in &kit.files {
            if !state.sha256.contains_key(path) && current.get(path).is_some_and(|old| old != bytes)
            {
                return Err(conflict(path));
            }
        }
    } else {
        // Without a baseline, only absent or byte-identical complete skills may
        // be adopted. Never infer ownership from a release label alone.
        for skill in SKILLS {
            let prefix = format!("{skill}/");
            let existing: BTreeMap<_, _> = current
                .iter()
                .filter(|(path, _)| path.starts_with(&prefix))
                .collect();
            let expected: BTreeMap<_, _> = kit
                .files
                .iter()
                .filter(|(path, _)| path.starts_with(&prefix))
                .collect();
            if !existing.is_empty() && existing != expected {
                return Err(conflict(skill));
            }
        }
    }
    desired.extend(kit.files.clone());
    Ok(Plan {
        files: desired,
        managed_directories,
    })
}

struct Lock(std::path::PathBuf);
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn parents(path: &str, directories: &mut BTreeSet<String>) {
    let mut parent = Path::new(path).parent();
    while let Some(path) = parent.filter(|path| !path.as_os_str().is_empty()) {
        directories.insert(path.to_string_lossy().replace('\\', "/"));
        parent = path.parent();
    }
}

fn directory_modes(root: &Path) -> Result<BTreeMap<String, fs::Permissions>> {
    fn visit(
        root: &Path,
        relative: &str,
        modes: &mut BTreeMap<String, fs::Permissions>,
    ) -> Result<()> {
        let directory = root.join(relative);
        let metadata = fs::symlink_metadata(&directory)
            .map_err(|_| error("Could not inspect consumer directory"))?;
        if !metadata.is_dir() {
            return Err(conflict(relative));
        }
        modes.insert(relative.to_owned(), metadata.permissions());
        for entry in
            fs::read_dir(directory).map_err(|_| error("Could not inspect consumer directories"))?
        {
            let entry = entry.map_err(|_| error("Could not inspect consumer directory"))?;
            if entry
                .file_type()
                .map_err(|_| error("Could not inspect consumer resource"))?
                .is_dir()
            {
                visit(
                    root,
                    &format!("{relative}/{}", entry.file_name().to_string_lossy()),
                    modes,
                )?;
            }
        }
        Ok(())
    }
    let mut modes = BTreeMap::new();
    for name in SKILLS {
        if root.join(name).exists() {
            visit(root, name, &mut modes)?;
        }
    }
    Ok(modes)
}

fn directory_layout(
    files: &BTreeMap<String, Vec<u8>>,
    existing: &BTreeMap<String, fs::Permissions>,
    managed: &BTreeSet<String>,
) -> Result<BTreeSet<String>> {
    let mut directories = BTreeSet::new();
    for path in files.keys() {
        parents(path, &mut directories);
    }
    for path in existing.keys().filter(|path| !managed.contains(*path)) {
        directories.insert(path.clone());
        parents(path, &mut directories);
    }
    for path in files.keys() {
        if directories.contains(path) {
            return Err(conflict(path));
        }
    }
    Ok(directories)
}

pub(super) fn apply(kit: &Kit, destination: &Path, update: bool) -> Result<()> {
    let original = snapshot(destination)?;
    let modes = directory_modes(destination)?;
    let previous = state_bytes(destination)?;
    let Plan {
        files: desired,
        managed_directories,
    } = plan(kit, &original, previous.as_deref(), update)?;
    let directories = directory_layout(&desired, &modes, &managed_directories)?;
    let baseline = Baseline {
        state_version: 1,
        release: kit.release.clone(),
        sha256: kit
            .files
            .iter()
            .map(|(path, bytes)| (path.clone(), hash(bytes)))
            .collect(),
    };
    let metadata = serde_json::to_vec_pretty(&baseline)
        .map_err(|_| error("Could not encode managed baseline"))?;
    if desired == original
        && previous.as_ref() == Some(&metadata)
        && directories.iter().eq(modes.keys())
    {
        return Ok(());
    }
    fs::create_dir_all(destination).map_err(|_| error("Could not create skills destination"))?;
    let lock_path = destination.join(".rostfrei-skills.lock");
    let _lock_file = fs::OpenOptions::new().write(true).create_new(true).open(&lock_path)
        .map_err(|_| error("Another skills transaction or stale .rostfrei-skills.lock exists; no resources changed"))?;
    let _lock = Lock(lock_path);
    let stage = tempfile::Builder::new()
        .prefix(".rostfrei-stage-")
        .tempdir_in(destination)
        .map_err(|_| error("Could not stage skills transaction"))?;
    for path in &directories {
        fs::create_dir_all(stage.path().join("next").join(path))
            .map_err(|_| error("Could not stage consumer directories"))?;
    }
    for (path, bytes) in &desired {
        let file = stage.path().join("next").join(path);
        if let Some(parent) = file.parent() {
            fs::create_dir_all(parent).map_err(|_| error("Could not stage resource directory"))?;
        }
        fs::write(&file, bytes).map_err(|_| error("Could not stage skill resource"))?;
        if original.contains_key(path) {
            let permissions = fs::metadata(destination.join(path))
                .map_err(|_| error("Could not preserve resource permissions"))?
                .permissions();
            fs::set_permissions(file, permissions)
                .map_err(|_| error("Could not preserve resource permissions"))?;
        }
    }
    fs::write(stage.path().join("next").join(STATE), metadata)
        .map_err(|_| error("Could not stage managed baseline"))?;
    // Populate beneath the private staging root first, then restore children
    // before their parents so read-only/private modes do not obstruct staging.
    for path in directories.iter().rev() {
        if let Some(permissions) = modes.get(path) {
            fs::set_permissions(stage.path().join("next").join(path), permissions.clone())
                .map_err(|_| error("Could not preserve consumer directory permissions"))?;
        }
    }
    if snapshot(destination)? != original
        || state_bytes(destination)? != previous
        || directory_modes(destination)? != modes
    {
        return Err(error(
            "Consumer work changed during preflight; no resources replaced",
        ));
    }
    match commit(stage.path(), destination) {
        Ok(()) => Ok(()),
        Err(error) => Err(error.detail("recoveryDirectory", stage.keep().display().to_string())),
    }
}

fn commit(stage: &Path, destination: &Path) -> Result<()> {
    let backup = stage.join("backup");
    fs::create_dir(&backup).map_err(|_| error("Could not stage rollback directory"))?;
    let mut applied = Vec::new();
    for name in SKILLS.into_iter().chain([STATE]) {
        let target = destination.join(name);
        let existed = target.exists();
        if existed && fs::rename(&target, backup.join(name)).is_err() {
            rollback(&applied, stage, destination)?;
            return Err(error(
                "Could not replace skills; previous transaction state restored",
            ));
        }
        if fs::rename(stage.join("next").join(name), &target).is_err() {
            applied.push((name, existed));
            rollback(&applied, stage, destination)?;
            return Err(error(
                "Could not commit skills; previous transaction state restored",
            ));
        }
        applied.push((name, existed));
    }
    Ok(())
}

fn rollback(applied: &[(&str, bool)], stage: &Path, destination: &Path) -> Result<()> {
    for (name, existed) in applied.iter().rev() {
        let target = destination.join(name);
        if target.is_dir() {
            fs::remove_dir_all(&target).map_err(|_| error("Skills rollback failed"))?;
        } else if target.exists() {
            fs::remove_file(&target).map_err(|_| error("Skills rollback failed"))?;
        }
        if *existed {
            fs::rename(stage.join("backup").join(name), target)
                .map_err(|_| error("Skills rollback failed"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "filesystem rollback failure fixture")]
mod tests {
    use super::*;

    #[test]
    fn a_later_commit_failure_restores_already_replaced_skills() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("skills");
        let stage = root.path().join("stage");
        for name in SKILLS {
            fs::create_dir_all(destination.join(name)).unwrap();
            fs::write(destination.join(name).join("SKILL.md"), "original").unwrap();
        }
        fs::write(destination.join(STATE), "original baseline").unwrap();
        fs::create_dir_all(stage.join("next/rostfrei-develop-feature")).unwrap();
        fs::write(
            stage.join("next/rostfrei-develop-feature/SKILL.md"),
            "replacement",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(
                stage.join("next/rostfrei-develop-feature"),
                fs::Permissions::from_mode(0o500),
            )
            .unwrap();
        }
        // No staged second skill: fail after the first replacement, not preflight.
        assert!(commit(&stage, &destination).is_err());
        for name in SKILLS {
            assert_eq!(
                fs::read_to_string(destination.join(name).join("SKILL.md")).unwrap(),
                "original"
            );
        }
        assert_eq!(
            fs::read_to_string(destination.join(STATE)).unwrap(),
            "original baseline"
        );
    }
}
