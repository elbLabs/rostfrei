use crate::{
    Environment, digest, environment, process, read_json, root, save_json, skills, text, versions,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{fs, path::Path};

pub fn clean_revision(source: &Path) -> Result<String> {
    let env = environment();
    let revision = process::run(&["git", "rev-parse", "HEAD"], source, &env)?
        .trim()
        .to_owned();
    ensure!(
        process::run(
            &["git", "status", "--porcelain", "--untracked-files=all"],
            source,
            &env
        )?
        .is_empty(),
        "Candidate source must be clean; put evidence/build outputs outside the checkout"
    );
    Ok(revision)
}
pub fn verify_release(release: &Value, revision: &str, version: &str) -> Result<()> {
    let expected = json!({"kitVersion":1,"frameworkVersion":version,"sourceRevision":revision,"sourceTreeDirty":false,
        "contracts":{"catalog":1,"behavioralTest":2,"messageSeries":2,"fixture":1},"skills":skills::NAMES});
    ensure!(
        release == &expected
            && revision.len() == 40
            && revision
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "CLI/kit candidate revision, clean build identity, version or protocol mismatch"
    );
    Ok(())
}
pub fn verify_crates(
    metadata: &Value,
    source: &Path,
    revision: &str,
    version: &str,
) -> Result<Value> {
    let mut records = Vec::new();
    for package in crate::array(&metadata["packages"])? {
        if !text(&package["name"])?.starts_with("rostfrei") {
            continue;
        }
        let origin = &package["source"];
        let path = crate::absolute(Path::new(text(&package["manifest_path"])?))?;
        let valid_origin = if origin.is_null() {
            path.starts_with(crate::absolute(source)?)
        } else {
            let origin = text(origin)?;
            origin.starts_with("git+") && origin.rsplit('#').next() == Some(revision)
        };
        ensure!(
            package["version"] == version && valid_origin,
            "Mixed candidate crate: {}: {origin}",
            package["name"]
        );
        records.push(json!({"id":package["id"],"name":package["name"],"version":package["version"],"source":origin,"manifest_path":package["manifest_path"]}));
    }
    ensure!(!records.is_empty(), "No resolved Rostfrei crates");
    Ok(json!(records))
}
pub fn verify_rustls(lock: &Path) -> Result<Value> {
    let lock = versions::read(lock)?;
    let versions = lock["package"]
        .as_array()
        .context("lock packages")?
        .iter()
        .filter(|p| p["name"].as_str() == Some("rustls"))
        .map(|p| p["version"].as_str().context("rustls version"))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        !versions.is_empty(),
        "missing rustls; report to lockfile owner"
    );
    for version in &versions {
        ensure!(
            semver::Version::parse(version)? >= semver::Version::new(0, 23, 45),
            "rustls must be patched >=0.23.45; report to lockfile owner"
        );
    }
    Ok(json!(versions))
}
pub fn immutable_checkout(source: &Path, destination: &Path) -> Result<()> {
    let revision = clean_revision(source)?;
    process::run(
        &[
            "git",
            "clone",
            "--quiet",
            "--no-hardlinks",
            "--no-checkout",
            &source.display().to_string(),
            &destination.display().to_string(),
        ],
        source,
        &environment(),
    )?;
    process::run(
        &[
            "git",
            "-c",
            "maintenance.auto=false",
            "checkout",
            "--quiet",
            "--detach",
            &revision,
        ],
        destination,
        &environment(),
    )?;
    ensure!(
        clean_revision(destination)? == revision,
        "Source-checkout revision changed"
    );
    Ok(())
}
pub fn prepare_bike_rental(source: &Path, destination: &Path, revision: &str) -> Result<()> {
    ensure!(
        !destination.exists()
            && !crate::absolute(destination)?.starts_with(crate::absolute(source)?),
        "Choose a new external app directory"
    );
    let workspace = versions::read(&source.join("Cargo.toml"))?["workspace"].clone();
    let example = source.join("examples/bike-rental");
    let tracked = process::run(
        &["git", "ls-files", "-z", "--", "examples/bike-rental"],
        source,
        &environment(),
    )?;
    for name in tracked.split('\0').filter(|p| !p.is_empty()) {
        let original = source.join(name);
        if !original.is_file() {
            continue;
        }
        let target = destination.join(original.strip_prefix(&example)?);
        fs::create_dir_all(target.parent().context("target parent")?)?;
        fs::copy(original, target)?;
    }
    let manifest_path = destination.join("Cargo.toml");
    let mut manifest = versions::read(&manifest_path)?;
    for (name, value) in manifest["package"].as_table_mut().context("package")? {
        if value.get("workspace").and_then(toml::Value::as_bool) == Some(true) {
            *value = workspace["package"][name].clone();
        }
    }
    for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(table) = manifest
            .get_mut(section)
            .and_then(toml::Value::as_table_mut)
        {
            for (name, specification) in table {
                if specification
                    .get("workspace")
                    .and_then(toml::Value::as_bool)
                    != Some(true)
                {
                    continue;
                }
                let inherited = &workspace["dependencies"][name];
                let mut dependency = inherited.as_table().cloned().unwrap_or_else(|| {
                    toml::Table::from_iter([("version".into(), inherited.clone())])
                });
                let mut features = dependency
                    .get("features")
                    .and_then(toml::Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                features.extend(
                    specification
                        .get("features")
                        .and_then(toml::Value::as_array)
                        .cloned()
                        .unwrap_or_default(),
                );
                for (key, value) in specification.as_table().context("dependency")? {
                    if key != "workspace" && key != "features" {
                        dependency.insert(key.clone(), value.clone());
                    }
                }
                if !features.is_empty() {
                    features.sort_by_key(|v| v.to_string());
                    features.dedup();
                    dependency.insert("features".into(), toml::Value::Array(features));
                }
                if dependency
                    .get("package")
                    .and_then(toml::Value::as_str)
                    .unwrap_or(name)
                    .starts_with("rostfrei")
                {
                    dependency.remove("path");
                    dependency.insert(
                        "git".into(),
                        toml::Value::String(
                            reqwest::Url::from_directory_path(source.canonicalize()?)
                                .map_err(|()| anyhow::anyhow!("source URL"))?
                                .as_str()
                                .trim_end_matches('/')
                                .to_owned(),
                        ),
                    );
                    dependency.insert("rev".into(), toml::Value::String(revision.into()));
                }
                *specification = toml::Value::Table(dependency);
            }
        }
    }
    manifest["lints"] = workspace["lints"].clone();
    fs::write(manifest_path, toml::to_string_pretty(&manifest)?)?;
    fs::copy(source.join("Cargo.lock"), destination.join("Cargo.lock"))?;
    fs::copy(
        source.join("rust-toolchain.toml"),
        destination.join("rust-toolchain.toml"),
    )?;
    Ok(())
}
pub struct Recorder<'a> {
    pub output: &'a Path,
}
impl Recorder<'_> {
    pub fn run(
        &self,
        command: &[String],
        cwd: &Path,
        env: &Environment,
        expected: i32,
    ) -> Result<String> {
        let ledger = self.output.join("commands.json");
        let mut commands = if ledger.exists() {
            read_json(&ledger)?
        } else {
            json!([])
        };
        let result = process::capture(
            command,
            cwd,
            env,
            std::time::Duration::from_secs(1800),
            &[],
            false,
        )?;
        let log = format!("command-{:02}.log", crate::array(&commands)?.len());
        fs::write(
            self.output.join(&log),
            format!("{}{}", result.stdout, result.stderr),
        )?;
        commands.as_array_mut().context("commands array")?.push(
            json!({"argv":command,"cwd":cwd,"exit":result.code,"expectedExit":expected,"log":log}),
        );
        save_json(&ledger, &commands)?;
        ensure!(
            result.code == expected,
            "Expected exit {expected}, observed {}; inspect {}",
            result.code,
            self.output.join(log).display()
        );
        Ok(result.stdout)
    }
    pub fn metadata(
        &self,
        label: &str,
        source: &Path,
        revision: &str,
        version: &str,
        cwd: &Path,
        env: &Environment,
    ) -> Result<Value> {
        let metadata: Value = serde_json::from_str(&self.run(
            &process::args(&["cargo", "metadata", "--locked", "--format-version", "1"]),
            cwd,
            env,
            0,
        )?)?;
        save_json(
            &self.output.join(format!("{label}-metadata.json")),
            &metadata,
        )?;
        fs::copy(
            cwd.join("Cargo.lock"),
            self.output.join(format!("{label}-Cargo.lock")),
        )?;
        save_json(
            &self.output.join(format!("{label}-rostfrei-crates.json")),
            &verify_crates(&metadata, source, revision, version)?,
        )?;
        Ok(metadata)
    }
    pub fn host_build(
        &self,
        label: &str,
        source: &Path,
        revision: &str,
        cwd: &Path,
        target: &Path,
        env: &Environment,
    ) -> Result<()> {
        let command = process::args(&[
            "cargo",
            "build",
            "--locked",
            "-p",
            "bike-rental",
            "--bin",
            "bike-rental-api",
            "--target-dir",
            &target.display().to_string(),
        ]);
        self.run(&command, cwd, env, 0)?;
        let executable = target.join("debug/bike-rental-api");
        save_json(
            &self.output.join(format!("{label}-host-build.json")),
            &json!({"sourceRevision":revision,"sourceTreeDirty":false,"source":source,
            "manifest":cwd.join("Cargo.toml"),"lockSha256":digest(&cwd.join("Cargo.lock"))?,"command":command,"executable":executable,
            "executableSha256":digest(&executable)?,"attestation":"local build command and binary hash; HTTP catalog does not attest Git SHA"}),
        )
    }
}
pub fn qualify(output: &Path) -> Result<()> {
    ensure!(
        !output.exists() && !output.starts_with(root()),
        "Choose a new output directory outside the source checkout"
    );
    let revision = clean_revision(&root())?;
    let version = versions::check(&root(), None)?;
    fs::create_dir_all(output)?;
    let record = Recorder { output };
    save_json(
        &output.join("candidate.json"),
        &json!({"revision":revision,"version":version,"source":root(),"liveQualification":"pending","freshSessionTrial":"parent-owned; unexecuted","humanFeedback":"pending"}),
    )?;
    let mut env = environment();
    env.insert(
        "ROSTFREI_TOOLS_DIR".into(),
        output.join("tools").display().to_string(),
    );
    record.metadata("workspace", &root(), &revision, &version, &root(), &env)?;
    save_json(
        &output.join("workspace-rustls.json"),
        &verify_rustls(&root().join("Cargo.lock"))?,
    )?;
    let target = output.join("target");
    record.run(
        &process::args(&[
            "cargo",
            "package",
            "--locked",
            "-p",
            "rostfrei-structure",
            "--target-dir",
            &target.display().to_string(),
        ]),
        &root(),
        &env,
        0,
    )?;
    let extraction = output.join("package-source");
    let packaged = skills::extract(
        &target.join(format!("package/rostfrei-structure-{version}.crate")),
        &extraction,
    )?;
    let vcs = read_json(&packaged.join(".cargo_vcs_info.json"))?;
    ensure!(
        vcs["git"]["sha1"] == revision && vcs["git"]["dirty"] != true,
        "Packaged CLI provenance mismatch"
    );
    save_json(&output.join("packaged-cli-vcs.json"), &vcs)?;
    let metadata: Value = serde_json::from_str(&record.run(
        &process::args(&["cargo", "metadata", "--locked", "--format-version", "1"]),
        &packaged,
        &env,
        0,
    )?)?;
    save_json(&output.join("packaged-cli-metadata.json"), &metadata)?;
    save_json(
        &output.join("packaged-cli-rostfrei-crates.json"),
        &json!(
            crate::array(&metadata["packages"])?
                .iter()
                .filter(|p| p["name"]
                    .as_str()
                    .is_some_and(|n| n.starts_with("rostfrei")))
                .collect::<Vec<_>>()
        ),
    )?;
    record.run(
        &process::args(&["cargo", "tree", "--locked", "-i", "rustls"]),
        &packaged,
        &env,
        0,
    )?;
    fs::copy(
        packaged.join("Cargo.lock"),
        output.join("packaged-cli-Cargo.lock"),
    )?;
    save_json(
        &output.join("packaged-cli-rustls.json"),
        &verify_rustls(&packaged.join("Cargo.lock"))?,
    )?;
    record.run(
        &process::args(&[
            "cargo",
            "install",
            "--locked",
            "--path",
            &packaged.display().to_string(),
            "--debug",
            "--root",
            &output.join("cli").display().to_string(),
            "--target-dir",
            &output.join("package-target").display().to_string(),
        ]),
        &root(),
        &env,
        0,
    )?;
    let cli = output.join("cli/bin/cargo-rostfrei");
    fs::remove_dir_all(&extraction)?;
    let mut no_tools = env.clone();
    no_tools.insert("PATH".into(), String::new());
    let release: Value = serde_json::from_str(&record.run(
        &process::args(&[&cli.display().to_string(), "rostfrei", "skills", "info"]),
        &root(),
        &no_tools,
        0,
    )?)?;
    verify_release(&release, &revision, &version)?;
    save_json(&output.join("cli-release.json"), &release)?;
    env.insert("ROSTFREI_CLI".into(), cli.display().to_string());
    record.run(
        &process::args(&[
            &std::env::current_exe()?.display().to_string(),
            "skills",
            "package",
            "--require-clean",
            "--output",
            &output.join("kit").display().to_string(),
        ]),
        &root(),
        &env,
        0,
    )?;
    let archive = crate::only_file(&output.join("kit"), ".tar.gz")?;
    let checksum = fs::read_to_string(archive.with_extension("gz.sha256"))?;
    let expected = format!(
        "{}  {}\n",
        digest(&archive)?,
        archive
            .file_name()
            .context("archive name")?
            .to_string_lossy()
    );
    ensure!(checksum == expected, "Kit checksum mismatch");
    let kit_release = skills::archive_release(&archive)?;
    verify_release(&kit_release, &revision, &version)?;
    record.host_build("workspace", &root(), &revision, &root(), &target, &env)?;
    let app = output.join("external-bike-rental");
    prepare_bike_rental(&root(), &app, &revision)?;
    record.run(
        &process::args(&["cargo", "metadata", "--format-version", "1"]),
        &app,
        &env,
        0,
    )?;
    record.metadata("external", &root(), &revision, &version, &app, &env)?;
    save_json(
        &output.join("external-rustls.json"),
        &verify_rustls(&app.join("Cargo.lock"))?,
    )?;
    for action in ["install", "update"] {
        record.run(
            &process::args(&[&cli.display().to_string(), "rostfrei", "skills", action]),
            &app,
            &env,
            0,
        )?;
    }
    for name in skills::NAMES {
        ensure!(
            read_json(&app.join(format!(".agents/skills/{name}/references/release.json")))?
                == kit_release,
            "Installed kit identity mismatch"
        );
    }
    let cached = walkdir::WalkDir::new(output.join("tools"))
        .into_iter()
        .collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|e| e.file_name() == "cargo-rostfrei")
        .map(|e| e.into_path())
        .collect::<Vec<_>>();
    ensure!(cached.len() == 1, "expected one retained runner");
    let cached_release: Value = serde_json::from_str(&record.run(
        &process::args(&[
            &cached[0].display().to_string(),
            "rostfrei",
            "skills",
            "info",
        ]),
        &app,
        &no_tools,
        0,
    )?)?;
    verify_release(&cached_release, &revision, &version)?;
    save_json(
        &output.join("retained-runner.json"),
        &json!({"path":cached[0],"sha256":digest(&cached[0])?,"release":cached_release,"cliSha256":digest(&cli)?,"packageSourceRemoved":!extraction.exists()}),
    )?;
    record.host_build(
        "external",
        &root(),
        &revision,
        &app,
        &output.join("external-target"),
        &env,
    )?;
    ensure!(
        clean_revision(&root())? == revision,
        "Candidate changed during qualification"
    );
    save_json(
        &output.join("artifacts.json"),
        &json!({"status":"passed","revision":revision,"release":release,"archive":archive,"sha256":digest(&archive)?,"liveQualification":"pending","humanFeedback":"pending"}),
    )
}
