use crate::{environment, process};
use anyhow::{Context, Result, ensure};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
use toml::Value;

pub const FIXTURE: &str = "crates/rostfrei-macros/tests/dependency-matrix/Cargo.toml";
const TABLES: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];
pub fn read(path: &Path) -> Result<Value> {
    Ok(toml::from_str(&fs::read_to_string(path)?)?)
}
fn workspace(root: &Path) -> Result<(Value, BTreeMap<PathBuf, Value>)> {
    let manifest = read(&root.join("Cargo.toml"))?;
    let mut members = BTreeMap::new();
    let mut excluded = Vec::new();
    if let Some(patterns) = manifest["workspace"]
        .get("exclude")
        .and_then(Value::as_array)
    {
        for pattern in patterns {
            for path in glob::glob(
                &root
                    .join(pattern.as_str().context("exclude")?)
                    .to_string_lossy(),
            )? {
                excluded.push(path?.canonicalize()?);
            }
        }
    }
    for pattern in manifest["workspace"]["members"]
        .as_array()
        .context("workspace members")?
    {
        for path in glob::glob(
            &root
                .join(pattern.as_str().context("member")?)
                .to_string_lossy(),
        )? {
            let path = path?.canonicalize()?;
            if !excluded.contains(&path) {
                members.insert(path.join("Cargo.toml"), read(&path.join("Cargo.toml"))?);
            }
        }
    }
    ensure!(!members.is_empty(), "No workspace members found");
    Ok((manifest, members))
}
fn internal(
    root: &Path,
    manifest: &Value,
    members: &BTreeMap<PathBuf, Value>,
) -> BTreeMap<String, Value> {
    manifest["workspace"]
        .get("dependencies")
        .and_then(Value::as_table)
        .into_iter()
        .flatten()
        .filter(|(alias, dep)| {
            let name = dep.get("package").and_then(Value::as_str).unwrap_or(alias);
            members
                .values()
                .any(|m| m["package"]["name"].as_str() == Some(name))
                || dep
                    .get("path")
                    .and_then(Value::as_str)
                    .and_then(|p| root.join(p).canonicalize().ok())
                    .is_some_and(|p| members.contains_key(&p.join("Cargo.toml")))
        })
        .map(|(a, d)| (a.clone(), d.clone()))
        .collect()
}
pub fn check(root: &Path, tag: Option<&str>) -> Result<String> {
    let (manifest, members) = workspace(root)?;
    let version = manifest["workspace"]["package"]["version"]
        .as_str()
        .context("workspace version")?;
    semver::Version::parse(version)?;
    let internal = internal(root, &manifest, &members);
    for (alias, dep) in &internal {
        ensure!(
            dep.get("version").and_then(Value::as_str) == Some(version),
            "workspace.dependencies.{alias} must specify version = {version:?}"
        );
        let path = dep.get("path").and_then(Value::as_str).with_context(|| {
            format!("workspace.dependencies.{alias} must specify its local path")
        })?;
        ensure!(
            members.contains_key(&root.join(path).canonicalize()?.join("Cargo.toml")),
            "workspace.dependencies.{alias} must point to a workspace member"
        );
    }
    for (path, member) in &members {
        ensure!(
            member["package"]
                .get("version")
                .and_then(Value::as_table)
                .is_some_and(|version| version.len() == 1
                    && version.get("workspace").and_then(Value::as_bool) == Some(true)),
            "{}: package version must inherit",
            path.display()
        );
        let scopes = std::iter::once(member).chain(
            member
                .get("target")
                .and_then(Value::as_table)
                .into_iter()
                .flat_map(|t| t.values()),
        );
        for scope in scopes {
            for table in TABLES {
                if let Some(table) = scope.get(table).and_then(Value::as_table) {
                    for (alias, dep) in table {
                        let name = dep.get("package").and_then(Value::as_str).unwrap_or(alias);
                        let is_internal = internal.contains_key(alias)
                            || members
                                .values()
                                .any(|m| m["package"]["name"].as_str() == Some(name))
                            || dep
                                .get("path")
                                .and_then(Value::as_str)
                                .and_then(|p| path.parent()?.join(p).canonicalize().ok())
                                .is_some_and(|p| members.contains_key(&p.join("Cargo.toml")));
                        if is_internal {
                            ensure!(
                                dep.get("workspace").and_then(Value::as_bool) == Some(true)
                                    && internal.contains_key(alias)
                                    && ["version", "path", "git", "package"]
                                        .iter()
                                        .all(|k| dep.get(k).is_none()),
                                "{}: {alias} must inherit its workspace dependency",
                                path.display()
                            );
                        }
                    }
                }
            }
        }
    }
    if let Some(tag) = tag {
        ensure!(
            tag == format!("v{version}"),
            "Release tag {tag:?} must match workspace version: v{version}"
        );
    }
    Ok(version.to_owned())
}
pub fn bump_with(
    root: &Path,
    version: &str,
    mut update: impl FnMut(&Path) -> Result<()>,
) -> Result<()> {
    semver::Version::parse(version)?;
    check(root, None)?;
    let (manifest, members) = workspace(root)?;
    let mut document =
        fs::read_to_string(root.join("Cargo.toml"))?.parse::<toml_edit::DocumentMut>()?;
    document["workspace"]["package"]["version"] = toml_edit::value(version);
    for alias in internal(root, &manifest, &members).keys() {
        document["workspace"]["dependencies"][alias]["version"] = toml_edit::value(version);
    }
    let files = [
        root.join("Cargo.toml"),
        root.join("Cargo.lock"),
        root.join(FIXTURE).with_file_name("Cargo.lock"),
    ];
    let originals = files
        .iter()
        .map(|p| {
            if p.exists() {
                fs::read(p).map(Some)
            } else {
                Ok(None)
            }
        })
        .collect::<std::io::Result<Vec<_>>>()?;
    let result = (|| {
        fs::write(&files[0], document.to_string())?;
        update(&files[0])?;
        update(&root.join(FIXTURE))?;
        check(root, None)?;
        Ok(())
    })();
    if result.is_err() {
        for (path, original) in files.iter().zip(originals) {
            if let Some(bytes) = original {
                fs::write(path, bytes)?;
            } else if path.exists() {
                fs::remove_file(path)?;
            }
        }
    }
    result
}
pub fn bump(root: &Path, version: &str) -> Result<()> {
    bump_with(root, version, |manifest| {
        process::run(
            &[
                "cargo",
                "update",
                "--workspace",
                "--offline",
                "--manifest-path",
                &manifest.to_string_lossy(),
            ],
            root,
            &environment(),
        )?;
        Ok(())
    })
}
