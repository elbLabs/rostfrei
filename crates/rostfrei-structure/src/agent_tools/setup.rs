use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use serde::Deserialize;
use serde_json::Value;

use super::{ErrorKind, Phase, Release, Result, ToolError, install, source_identity};

pub struct SetupOptions {
    pub kit: Option<PathBuf>,
    pub destination: Option<PathBuf>,
    pub manifest_path: Option<PathBuf>,
    pub package: Option<String>,
    pub update: bool,
}

#[derive(Clone, Debug)]
enum Source {
    Git { url: String, revision: String },
    Registry,
    Local(PathBuf),
}

struct Framework {
    version: String,
    revision: String,
    source: Source,
}

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    workspace_members: Vec<String>,
    workspace_default_members: Vec<String>,
    workspace_root: PathBuf,
    resolve: Resolve,
}
#[derive(Deserialize)]
struct Package {
    id: String,
    name: String,
    version: String,
    manifest_path: PathBuf,
    source: Option<String>,
}
#[derive(Deserialize)]
struct Resolve {
    root: Option<String>,
    nodes: Vec<Node>,
}
#[derive(Deserialize)]
struct Node {
    id: String,
    dependencies: Vec<String>,
}

fn error(message: &str) -> ToolError {
    ToolError::new(ErrorKind::SetupFailed, Phase::Installation, message)
}

fn valid_revision(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn local_revision(directory: &Path) -> Result<String> {
    let output = Command::new("git").args(["rev-parse", "HEAD"]).current_dir(directory).output()
        .map_err(|_| error("A local path dependency requires Git provenance; no matching revision could be established"))?;
    let revision = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if !output.status.success() || !valid_revision(&revision) {
        return Err(error(
            "Local framework dependency has no exact Git revision",
        ));
    }
    Ok(revision)
}

fn framework(package: &Package) -> Result<Framework> {
    let directory = package
        .manifest_path
        .parent()
        .ok_or_else(|| error("Invalid framework manifest path"))?;
    let (source, revision) = if let Some(source) = &package.source {
        if let Some(git) = source.strip_prefix("git+") {
            let (address, revision) = git
                .rsplit_once('#')
                .ok_or_else(|| error("Cargo did not resolve an exact framework Git revision"))?;
            let address = address
                .split('?')
                .next()
                .ok_or_else(|| error("Invalid Git source"))?;
            let url = reqwest::Url::parse(address)
                .map_err(|_| error("Unsupported Git dependency source"))?;
            if !matches!(url.scheme(), "https" | "http" | "file" | "ssh")
                || url.password().is_some()
                || (!url.username().is_empty() && url.scheme() != "ssh")
                || !valid_revision(revision)
            {
                return Err(error(
                    "Unsupported or credential-bearing framework Git source",
                ));
            }
            (
                Source::Git {
                    url: address.to_owned(),
                    revision: revision.to_owned(),
                },
                revision.to_owned(),
            )
        } else if matches!(
            source.as_str(),
            "registry+https://github.com/rust-lang/crates.io-index"
                | "sparse+https://index.crates.io/"
        ) {
            let bytes = fs::read(directory.join(".cargo_vcs_info.json")).map_err(|_| error("Published framework package has no exact source provenance; unsupported release"))?;
            let value: Value = serde_json::from_slice(&bytes)
                .map_err(|_| error("Invalid published framework provenance"))?;
            let revision = value
                .pointer("/git/sha1")
                .and_then(Value::as_str)
                .filter(|value| valid_revision(value))
                .ok_or_else(|| error("Published framework has no supported source revision"))?
                .to_owned();
            (Source::Registry, revision)
        } else {
            return Err(error(
                "Only crates.io, exact Git, and Git-backed local path framework dependencies are supported",
            ));
        }
    } else {
        (
            Source::Local(directory.to_owned()),
            local_revision(directory)?,
        )
    };
    Ok(Framework {
        version: package.version.clone(),
        revision,
        source,
    })
}

fn locate_manifest(explicit: Option<&Path>) -> Result<Option<PathBuf>> {
    if let Some(path) = explicit {
        return Ok(Some(path.to_owned()));
    }
    let cwd = env::current_dir().map_err(|_| error("Could not determine current directory"))?;
    Ok(cwd
        .ancestors()
        .map(|dir| dir.join("Cargo.toml"))
        .find(|path| path.is_file()))
}

fn resolve(manifest: &Path, selection: Option<&str>) -> Result<(Framework, PathBuf)> {
    let output = Command::new("cargo")
        .args([
            "metadata",
            "--locked",
            "--format-version",
            "1",
            "--manifest-path",
        ])
        .arg(manifest)
        .output()
        .map_err(|_| error("Dependency-aware setup requires Cargo on PATH"))?;
    if !output.status.success() {
        return Err(error(
            "Cargo could not resolve the locked application; run cargo metadata --locked to diagnose or create/update Cargo.lock explicitly",
        ));
    }
    let metadata: Metadata = serde_json::from_slice(&output.stdout)
        .map_err(|_| error("Invalid Cargo metadata response"))?;
    let root = if let Some(name) = selection {
        let mut matches = metadata.packages.iter().filter(|package| {
            metadata.workspace_members.contains(&package.id)
                && (package.name == name || package.id == name)
        });
        let selected = matches
            .next()
            .ok_or_else(|| error("Selected package was not found"))?;
        if matches.next().is_some() {
            return Err(error("Ambiguous package selection"));
        }
        selected
    } else {
        let at_workspace_root = fs::canonicalize(manifest)
            .map_err(|_| error("Could not resolve application manifest path"))?
            == metadata.workspace_root.join("Cargo.toml");
        let id = if at_workspace_root {
            if metadata.workspace_default_members.len() == 1 {
                metadata.workspace_default_members.first()
            } else {
                None
            }
        } else {
            metadata.resolve.root.as_ref()
        }
        .ok_or_else(|| {
            error("Workspace has multiple application packages; select one with --package")
        })?;
        metadata
            .packages
            .iter()
            .find(|package| package.id == *id)
            .ok_or_else(|| error("Could not identify application package"))?
    };
    let graph: BTreeMap<_, _> = metadata
        .resolve
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), &node.dependencies))
        .collect();
    let mut pending = vec![root.id.as_str()];
    let mut reachable = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if reachable.insert(id)
            && let Some(dependencies) = graph.get(id)
        {
            pending.extend(dependencies.iter().map(String::as_str));
        }
    }
    let mut candidates = metadata
        .packages
        .iter()
        .filter(|package| package.name == "rostfrei" && reachable.contains(package.id.as_str()));
    let dependency = candidates
        .next()
        .ok_or_else(|| error("Selected application has no resolved rostfrei dependency"))?;
    if candidates.next().is_some() {
        return Err(error(
            "Application resolves multiple rostfrei versions/revisions; select an unambiguous application package",
        ));
    }
    let destination = root
        .manifest_path
        .parent()
        .ok_or_else(|| error("Invalid application manifest path"))?
        .join(".agents/skills");
    Ok((framework(dependency)?, destination))
}

fn tools_root() -> Result<PathBuf> {
    if let Some(path) = env::var_os("ROSTFREI_TOOLS_DIR") {
        return Ok(PathBuf::from(path));
    }
    let cargo = env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".cargo")))
        .ok_or_else(|| {
            error("Set CARGO_HOME or ROSTFREI_TOOLS_DIR for the versioned tools cache")
        })?;
    Ok(cargo.join("rostfrei/tools"))
}

fn cache_root(version: &str, revision: &str) -> Result<PathBuf> {
    if !valid_revision(revision)
        || version.is_empty()
        || !version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b".-+".contains(&byte))
    {
        return Err(error(
            "Invalid framework version/revision for tool selection",
        ));
    }
    Ok(tools_root()?.join(format!(
        "{version}-{revision}-{}-{}",
        env::consts::OS,
        env::consts::ARCH
    )))
}

fn executable(root: &Path) -> PathBuf {
    root.join("bin")
        .join(format!("cargo-rostfrei{}", env::consts::EXE_SUFFIX))
}

fn release_cache(release: &Release) -> Result<PathBuf> {
    if !release.valid_identity() {
        return Err(error(
            "Unknown build identity; run skills update with fingerprint-capable tooling",
        ));
    }
    let mut base =
        cache_root(&release.framework_version, &release.source_revision)?.into_os_string();
    if let Some(fingerprint) = &release.source_fingerprint {
        base.push(format!("-local-{fingerprint}"));
    }
    Ok(PathBuf::from(base))
}

fn local_tool(framework: &Framework, directory: &Path) -> Result<PathBuf> {
    let cli = directory
        .parent()
        .ok_or_else(|| error("Unsupported local framework layout"))?
        .join("rostfrei-structure");
    if !cli.join("Cargo.toml").is_file() {
        return Err(error(
            "Local framework has no matching rostfrei-structure source",
        ));
    }
    let (before, _) = source_identity::source(directory)
        .map_err(|_| error("Could not fingerprint local framework source"))?;
    let tools = tools_root()?;
    fs::create_dir_all(&tools).map_err(|_| error("Could not create matching tool cache"))?;
    let staging = tempfile::Builder::new()
        .prefix(".local-build-")
        .tempdir_in(&tools)
        .map_err(|_| error("Could not stage local tool build"))?;
    let output = Command::new("cargo")
        .args(["install", "--locked", "--debug", "--path"])
        .arg(cli)
        .arg("--root")
        .arg(staging.path())
        .arg("rostfrei-structure")
        .env("ROSTFREI_SOURCE_STATE", &before)
        .output()
        .map_err(|_| error("Could not build matching local tool"))?;
    if !output.status.success() {
        return Err(error(
            "Could not build matching local tool; check Cargo access/toolchain",
        ));
    }
    let release = inspect_tool(&executable(staging.path()))?;
    let (after, _) = source_identity::source(directory)
        .map_err(|_| error("Could not verify local source after build"))?;
    if before != after {
        return Err(error(
            "Local source changed during build; no skills installed",
        ));
    }
    if release.framework_version != framework.version
        || release.source_revision != framework.revision
        || (release.source_tree_dirty && release.source_fingerprint.as_ref() != Some(&after))
    {
        return Err(error(
            "Local source/build identity changed or is unsupported; no skills installed",
        ));
    }
    let destination = release_cache(&release)?;
    if destination.exists() {
        if !release.matches(&inspect_tool(&executable(&destination))?) {
            return Err(error("Cached build identity mismatch; preserve this entry and move it aside before retrying").detail("path", destination.display().to_string()));
        }
    } else {
        fs::rename(staging.path(), &destination).map_err(|_| {
            error("Could not retain the matching local build; existing cache entries preserved")
        })?;
    }
    Ok(executable(&destination))
}

fn inspect_tool(path: &Path) -> Result<Release> {
    let output = Command::new(path)
        .args(["rostfrei", "skills", "info"])
        .output()
        .map_err(|_| error("Matching skills-enabled CLI is unavailable"))?;
    if !output.status.success() {
        return Err(error(
            "Resolved framework predates skills-enabled tooling; no compatible kit is available",
        ));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|_| error("Resolved tool has no supported release metadata"))
}

fn matching_tool(framework: &Framework) -> Result<Option<PathBuf>> {
    if let Source::Local(directory) = &framework.source {
        return local_tool(framework, directory).map(Some);
    }
    let current = Release::current();
    if framework.version == current.framework_version
        && framework.revision == current.source_revision
        && !current.source_tree_dirty
        && !matches!(framework.source, Source::Local(_))
    {
        return Ok(None);
    }
    let root = cache_root(&framework.version, &framework.revision)?;
    let binary = executable(&root);
    if !binary.exists() {
        fs::create_dir_all(&root).map_err(|_| error("Could not create matching tool cache"))?;
        let mut cargo = Command::new("cargo");
        cargo.args(["install", "--locked", "--root"]).arg(&root);
        match &framework.source {
            Source::Git { url, revision } => {
                cargo.args(["--git", url, "--rev", revision]);
            }
            Source::Registry => {
                cargo.args([
                    "--registry",
                    "crates-io",
                    "--version",
                    &format!("={}", framework.version),
                ]);
            }
            Source::Local(directory) => {
                return local_tool(framework, directory).map(Some);
            }
        }
        cargo.arg("rostfrei-structure");
        println!(
            "Preparing the CLI for resolved Rostfrei {} at {}",
            framework.version, framework.revision
        );
        // Cargo diagnostics can include credential-bearing registry configuration.
        // Keep raw output out of setup diagnostics; never change Cargo settings.
        let output = cargo
            .output()
            .map_err(|_| error("Could not invoke Cargo to obtain the matching setup tool"))?;
        if !output.status.success() {
            return Err(error(
                "Could not obtain the exact resolved rostfrei-structure tool; check Cargo access/toolchain or whether this pre-kit revision supports skills",
            ));
        }
    }
    let release = inspect_tool(&binary)?;
    if release.framework_version != framework.version
        || release.source_revision != framework.revision
        || (!matches!(framework.source, Source::Local(_)) && release.source_tree_dirty)
    {
        return Err(error(
            "Cached tool does not match the application's resolved pristine framework revision; move this cache entry aside and rerun setup",
        ).detail("path", root.display().to_string()));
    }
    Ok(Some(binary))
}

pub(super) fn cached_runner(release: &Release) -> Result<PathBuf> {
    let binary = executable(&release_cache(release)?);
    if !binary.is_file() {
        return Err(error(
            "Kit/CLI release or protocol mismatch; run explicit application skills install/update to prepare its matching tool",
        ));
    }
    let actual = inspect_tool(&binary)?;
    if !release.matches(&actual) {
        return Err(error("Cached runner release or protocol mismatch"));
    }
    Ok(binary)
}

fn apply_with_tool(
    binary: &Path,
    kit: Option<&Path>,
    destination: &Path,
    update: bool,
) -> Result<()> {
    let mut command = Command::new(binary);
    command
        .args(["rostfrei", "skills", "apply", "--destination"])
        .arg(destination);
    if let Some(kit) = kit {
        command.arg("--kit").arg(kit);
    }
    if update {
        command.arg("--update");
    }
    let output = command
        .output()
        .map_err(|_| error("Could not start matching setup tool"))?;
    if !output.status.success() {
        return Err(error("Matching tool could not install/update skills (unsupported pre-onboarding revision or preserved consumer conflict)")
            .detail("diagnostic", String::from_utf8_lossy(&output.stderr).trim()));
    }
    print!("{}", String::from_utf8_lossy(&output.stdout));
    Ok(())
}

pub fn setup_skills(options: SetupOptions) -> Result<()> {
    if let Some(kit) = &options.kit {
        let release = install::external(kit)?.release;
        let destination = options
            .destination
            .as_deref()
            .unwrap_or_else(|| Path::new(".agents/skills"));
        if release.check(Phase::Installation).is_ok() {
            return install::install_skills(Some(kit), destination, options.update);
        }
        return apply_with_tool(
            &cached_runner(&release)?,
            Some(kit),
            destination,
            options.update,
        );
    }
    let manifest = locate_manifest(options.manifest_path.as_deref())?;
    let Some(manifest) = manifest else {
        let destination = options.destination.ok_or_else(|| error("No Cargo application found; use --manifest-path or an explicit --destination for this CLI's bundled skills"))?;
        return install::install_skills(None, &destination, options.update);
    };
    let (framework, default) = resolve(&manifest, options.package.as_deref())?;
    let destination = options.destination.unwrap_or(default);
    if let Some(binary) = matching_tool(&framework)? {
        apply_with_tool(&binary, None, &destination, options.update)
    } else {
        install::install_skills(None, &destination, options.update)
    }
}

pub fn setup_scaffold(directory: &Path, version: &str, revision: &str) -> Result<()> {
    let framework = Framework {
        version: version.to_owned(),
        revision: revision.to_owned(),
        source: Source::Git {
            url: "https://github.com/elbLabs/rostfrei".to_owned(),
            revision: revision.to_owned(),
        },
    };
    let destination = directory.join(".agents/skills");
    matching_tool(&framework)?.map_or_else(
        || install::install_skills(None, &destination, false),
        |binary| apply_with_tool(&binary, None, &destination, false),
    )
}

pub(super) fn delegate_test(release: &Release) -> Result<u8> {
    let status = Command::new(cached_runner(release)?)
        .args(env::args_os().skip(1))
        .stdin(Stdio::null())
        .status()
        .map_err(|_| error("Could not execute cached matching runner"))?;
    match status.code() {
        Some(0) => Ok(0),
        Some(1) => Ok(1),
        Some(2) => Ok(2),
        _ => Err(error(
            "Matching runner terminated without a supported result",
        )),
    }
}
