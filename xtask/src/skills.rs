//! Native skill-format validation and reproducible release archives.
use crate::{candidate, cli, environment, files, hash, process, read_json, root, versions};
use anyhow::{Context, Result, ensure};
use flate2::{Compression, GzBuilder, read::GzDecoder};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
};
use unicode_normalization::UnicodeNormalization;
use yaml_rust2::{
    Yaml, YamlLoader,
    parser::{Event, MarkedEventReceiver, Parser},
    scanner::{Marker, Scanner, TScalarStyle, TokenType},
};

pub const NAMES: [&str; 2] = ["rostfrei-develop-feature", "rostfrei-tracer"];
pub const REFERENCE_REVISION: &str = "69ef37e9424c0a7ea9dd2293b559e43ec8176379";

#[derive(Default)]
struct StringScalars(YamlLoader);
impl MarkedEventReceiver for StringScalars {
    fn on_event(&mut self, event: Event, mark: Marker) {
        // StrictYAML's schema-free parser preserves scalars as strings, including
        // numeric-looking names, booleans, null and empty compatibility fields.
        let event = match event {
            Event::Scalar(value, _, anchor, tag) => {
                Event::Scalar(value, TScalarStyle::DoubleQuoted, anchor, tag)
            }
            event => event,
        };
        self.0.on_event(event, mark);
    }
}

// Rules follow agentskills/agentskills skills-ref at REFERENCE_REVISION. The
// conformance cases live in tests/skills.rs; validation needs no network access.
pub fn validate(directory: &Path) -> Result<()> {
    ensure!(
        directory.is_dir(),
        "not a skill directory: {}",
        directory.display()
    );
    let path = ["SKILL.md", "skill.md"]
        .into_iter()
        .map(|n| directory.join(n))
        .find(|p| p.is_file())
        .context("Missing required file: SKILL.md")?;
    validate_content(
        &fs::read_to_string(path)?,
        directory
            .file_name()
            .context("skill directory name")?
            .to_str()
            .context("UTF-8 directory name")?,
    )
}
pub fn validate_content(content: &str, directory_name: &str) -> Result<()> {
    ensure!(
        content.starts_with("---"),
        "SKILL.md must start with YAML frontmatter (---)"
    );
    let front = content
        .strip_prefix("---")
        .context("frontmatter")?
        .split_once("---")
        .context("SKILL.md frontmatter not properly closed with ---")?
        .0;
    // StrictYAML rejects flow collections, aliases, anchors and explicit tags.
    for token in Scanner::new(front.chars()) {
        ensure!(
            !matches!(
                token.1,
                TokenType::FlowSequenceStart
                    | TokenType::FlowMappingStart
                    | TokenType::Anchor(_)
                    | TokenType::Alias(_)
                    | TokenType::Tag(_, _)
            ),
            "unsupported strict YAML construct"
        );
    }
    let mut loader = StringScalars::default();
    Parser::new_from_str(front).load(&mut loader, true)?;
    let docs = loader.0.documents();
    ensure!(docs.len() == 1, "expected one YAML document");
    let metadata = docs[0]
        .as_hash()
        .context("SKILL.md frontmatter must be a YAML mapping")?;
    let allowed = [
        "name",
        "description",
        "license",
        "allowed-tools",
        "metadata",
        "compatibility",
    ];
    for key in metadata.keys() {
        ensure!(
            key.as_str().is_some_and(|k| allowed.contains(&k)),
            "Unexpected field in frontmatter: {key:?}"
        );
    }
    let scalar = |key: &str| -> Result<String> {
        match metadata
            .get(&Yaml::String(key.into()))
            .with_context(|| format!("Missing required field: {key}"))?
        {
            Yaml::String(v) | Yaml::Real(v) => Ok(v.clone()),
            Yaml::Integer(v) => Ok(v.to_string()),
            Yaml::Boolean(v) => Ok(v.to_string()),
            _ => anyhow::bail!("Field '{key}' must be a string"),
        }
    };
    let name: String = scalar("name")?.trim().nfkc().collect();
    ensure!(
        !name.is_empty() && name.chars().count() <= 64,
        "name must contain 1–64 characters"
    );
    ensure!(
        name == name.to_lowercase()
            && !name.starts_with('-')
            && !name.ends_with('-')
            && !name.contains("--")
            // Python's isalnum uses Letter/Number categories; Rust's
            // is_alphanumeric additionally includes some combining marks.
            && regex::Regex::new(r"^[\p{L}\p{N}-]+$")?.is_match(&name),
        "invalid skill name: {name}"
    );
    ensure!(
        name == directory_name.nfkc().collect::<String>(),
        "Directory name must match skill name"
    );
    let description = scalar("description")?;
    ensure!(
        !description.trim().is_empty() && description.chars().count() <= 1024,
        "description must contain 1–1024 characters"
    );
    if metadata.contains_key(&Yaml::String("compatibility".into())) {
        ensure!(
            scalar("compatibility")?.chars().count() <= 500,
            "compatibility exceeds 500 characters"
        );
    }
    Ok(())
}
pub fn package(output: &Path, require_clean: bool) -> Result<PathBuf> {
    let version = versions::check(&root(), None)?;
    let release: Value = serde_json::from_str(&process::run(
        &[&cli().display().to_string(), "rostfrei", "skills", "info"],
        &root(),
        &environment(),
    )?)?;
    let revision = process::run(&["git", "rev-parse", "HEAD"], &root(), &environment())?;
    ensure!(
        release["frameworkVersion"] == version && release["sourceRevision"] == revision.trim(),
        "CLI build is stale; rebuild rostfrei-structure"
    );
    if require_clean {
        candidate::verify_release(&release, &candidate::clean_revision(&root())?, &version)?;
    }
    let catalog = fs::read_to_string(root().join("crates/rostfrei-tracer/src/catalog.rs"))?;
    let catalog_version = regex::Regex::new(r"CATALOG_VERSION: u32 = (\d+)")?
        .captures(&catalog)
        .context("catalog version")?[1]
        .parse::<u64>()?;
    ensure!(
        release["contracts"]["catalog"] == catalog_version,
        "CLI catalog contract mismatch"
    );
    let temporary = tempfile::tempdir()?;
    let destination = temporary.path().join("skills");
    process::run(
        &[
            &cli().display().to_string(),
            "rostfrei",
            "skills",
            "install",
            "--destination",
            &destination.display().to_string(),
        ],
        temporary.path(),
        &environment(),
    )?;
    let mut resources = BTreeMap::new();
    let mut expected = BTreeMap::new();
    for name in NAMES {
        validate(&destination.join(name))?;
        for (path, bytes) in files(&destination.join(name))? {
            resources.insert(format!("{name}/{path}"), bytes);
        }
        let prefix = format!(".agents/skills/{name}");
        let tracked = process::run(
            &["git", "ls-files", "-z", "--", &prefix],
            &root(),
            &environment(),
        )?;
        for relative in tracked.split('\0').filter(|p| !p.is_empty()) {
            let path = root().join(relative);
            ensure!(
                !path.is_symlink(),
                "authoritative resources must be regular files"
            );
            if path.is_file()
                && path
                    .extension()
                    .is_some_and(|ext| ext == "md" || ext == "json")
            {
                expected.insert(
                    relative
                        .strip_prefix(".agents/skills/")
                        .context("resource path")?
                        .to_owned(),
                    fs::read(path)?,
                );
            }
        }
        expected.insert(format!("{name}/LICENSE"), fs::read(root().join("LICENSE"))?);
        let metadata = format!("{name}/references/release.json");
        ensure!(
            serde_json::from_slice::<Value>(&resources[&metadata])? == release,
            "bundled release differs from CLI"
        );
        expected.insert(metadata.clone(), resources[&metadata].clone());
    }
    expected.insert(
        "rostfrei-develop-feature/assets/behavioral-test-v2.schema.json".into(),
        fs::read(root().join("crates/rostfrei-tracer/schema/behavioral-test-v2.schema.json"))?,
    );
    ensure!(
        resources == expected,
        "bundled CLI resources differ from authoritative source; rebuild the CLI"
    );
    let mut archive_files = BTreeMap::from([
        (
            "README.md".into(),
            fs::read(root().join("docs/agent-skills-install.md"))?,
        ),
        ("LICENSE".into(), fs::read(root().join("LICENSE"))?),
        (
            "release.json".into(),
            resources["rostfrei-develop-feature/references/release.json"].clone(),
        ),
    ]);
    archive_files.extend(
        resources
            .into_iter()
            .map(|(n, b)| (format!("skills/{n}"), b)),
    );
    let checksums: BTreeMap<_, _> = archive_files
        .iter()
        .map(|(n, b)| (n.clone(), hash(b)))
        .collect();
    archive_files.insert(
        "manifest.json".into(),
        format!(
            "{}\n",
            serde_json::to_string_pretty(&json!({"sha256": checksums}))?
        )
        .into_bytes(),
    );
    fs::create_dir_all(output)?;
    let stem = format!("rostfrei-agent-skills-{version}");
    let path = output.join(format!("{stem}.tar.gz"));
    write_archive(&path, &stem, &archive_files)?;
    fs::write(
        path.with_extension("gz.sha256"),
        format!("{}  {stem}.tar.gz\n", crate::digest(&path)?),
    )?;
    Ok(path)
}
pub fn write_archive(path: &Path, stem: &str, files: &BTreeMap<String, Vec<u8>>) -> Result<()> {
    let gzip = GzBuilder::new()
        .mtime(0)
        .write(fs::File::create(path)?, Compression::default());
    let mut archive = tar::Builder::new(gzip);
    for (name, bytes) in files {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len().try_into()?);
        header.set_mode(0o644);
        header.set_mtime(0);
        header.set_uid(0);
        header.set_gid(0);
        header.set_cksum();
        archive.append_data(&mut header, format!("{stem}/{name}"), bytes.as_slice())?;
    }
    archive.into_inner()?.finish()?;
    Ok(())
}
pub fn extract(archive: &Path, destination: &Path) -> Result<PathBuf> {
    fs::create_dir_all(destination)?;
    for entry in tar::Archive::new(GzDecoder::new(fs::File::open(archive)?)).entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        ensure!(
            path.components().all(|c| matches!(c, Component::Normal(_))),
            "unsafe archive path"
        );
        ensure!(
            entry.header().entry_type().is_file() || entry.header().entry_type().is_dir(),
            "archive link or special file"
        );
        ensure!(
            entry.unpack_in(destination)?,
            "archive path escaped destination"
        );
    }
    crate::only_file(destination, "")
}
pub fn archive_release(archive: &Path) -> Result<Value> {
    for entry in tar::Archive::new(GzDecoder::new(fs::File::open(archive)?)).entries()? {
        let mut entry = entry?;
        if entry.path()?.components().count() == 2 && entry.path()?.ends_with("release.json") {
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes)?;
            return Ok(serde_json::from_slice(&bytes)?);
        }
    }
    anyhow::bail!("archive has no release metadata")
}
pub fn validate_links(skill: &Path) -> Result<()> {
    let links = regex::Regex::new(r"\]\(([^)]+)\)")?;
    for (path, bytes) in files(skill)? {
        let text = String::from_utf8(bytes)?;
        ensure!(
            !text.contains(&root().display().to_string()) && !text.contains("agent-harness/"),
            "source path leaked into kit"
        );
        if path.ends_with(".md") {
            for link in links.captures_iter(&text) {
                let href = &link[1];
                if href.contains("://") || href.starts_with('#') {
                    continue;
                }
                let target = skill
                    .join(&path)
                    .parent()
                    .context("link parent")?
                    .join(href.split('#').next().context("href")?)
                    .canonicalize()?;
                ensure!(
                    target.starts_with(skill.canonicalize()?) && target.is_file(),
                    "broken or escaping skill link: {href}"
                );
            }
        }
    }
    let _ = read_json(&skill.join("references/release.json"))?;
    Ok(())
}
