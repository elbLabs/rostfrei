use super::command;
use crate::{Environment, cli, environment, process, read_json, root};
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};

pub(super) fn commit(source: &Path, env: &Environment) -> Result<()> {
    process::run(
        &[
            "git",
            "-c",
            "user.name=Skills regression",
            "-c",
            "user.email=skills@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "--allow-empty",
            "-m",
            "Source fixture",
        ],
        source,
        env,
    )?;
    Ok(())
}
pub(super) fn fresh(destination: &Path, env: &Environment) -> Result<()> {
    let paths = process::run(&["git", "ls-files", "-z"], &root(), env)?;
    for name in paths.split('\0').filter(|s| !s.is_empty()) {
        let source = root().join(name);
        let target = destination.join(name);
        if !source.is_file() && !source.is_symlink() {
            continue;
        }
        fs::create_dir_all(target.parent().context("source fixture parent")?)?;
        if source.is_symlink() {
            std::os::unix::fs::symlink(fs::read_link(source)?, target)?;
        } else {
            fs::copy(source, target)?;
        }
    }
    // Include the new repository tool during pre-commit acceptance too.
    crate::copy_tree(&root().join("xtask"), &destination.join("xtask"))?;
    process::run(
        &["git", "init", "--quiet", &destination.display().to_string()],
        &root(),
        env,
    )?;
    for (key, value) in [("maintenance.auto", "false"), ("gc.auto", "0")] {
        process::run(&["git", "config", "--local", key, value], destination, env)?;
    }
    process::run(&["git", "add", "."], destination, env)?;
    commit(destination, env)
}
pub(super) fn build(source: &Path, target: &Path, env: &Environment) -> Result<PathBuf> {
    process::run(
        &[
            "cargo",
            "build",
            "--locked",
            "--offline",
            "--manifest-path",
            &source
                .join("crates/rostfrei-structure/Cargo.toml")
                .display()
                .to_string(),
            "--bin",
            "cargo-rostfrei",
            "--target-dir",
            &target.display().to_string(),
        ],
        source,
        env,
    )?;
    Ok(target.join("debug/cargo-rostfrei"))
}
pub(super) fn info(cli: &Path, cwd: &Path, env: &Environment) -> Result<Value> {
    Ok(serde_json::from_str(&process::run(
        &[&cli.display().to_string(), "rostfrei", "skills", "info"],
        cwd,
        env,
    )?)?)
}
pub(super) fn head(source: &Path, env: &Environment) -> Result<String> {
    Ok(process::run(&["git", "rev-parse", "HEAD"], source, env)?
        .trim()
        .into())
}
pub fn run() -> Result<()> {
    let temporary = tempfile::Builder::new()
        .prefix("rostfrei-source-skills-")
        .tempdir()?;
    let parent = temporary.path();
    let config = parent.join("gitconfig");
    fs::write(
        &config,
        "[gc]\n auto=1\n autoDetach=true\n[maintenance]\n auto=true\n",
    )?;
    let trace = parent.join("git-trace.jsonl");
    let mut env = environment();
    env.insert("GIT_CONFIG_GLOBAL".into(), config.display().to_string());
    env.insert("GIT_TRACE2_EVENT".into(), trace.display().to_string());
    let source = parent.join("checkout");
    fresh(&source, &env)?;
    commit(&source, &env)?;
    for line in fs::read_to_string(&trace)?.lines() {
        let event: Value = serde_json::from_str(line)?;
        if event["event"] == "child_start" {
            ensure!(
                !event["argv"].as_array().into_iter().flatten().any(|v| v
                    .as_str()
                    .is_some_and(|s| ["gc", "repack", "pack-objects"].contains(&s))),
                "fixture launched background Git writer"
            );
        }
    }
    let skill = source.join(".agents/skills/rostfrei-develop-feature");
    let release = skill.join("references/release.json");
    ensure!(!release.exists(), "source has generated metadata");
    let target = root().join("target/skills-source");
    let native = build(&source, &target, &env)?;
    let mut empty = env.clone();
    empty.insert("PATH".into(), String::new());
    let first = info(&native, &source, &empty)?;
    let first_head = head(&source, &env)?;
    ensure!(
        first["sourceRevision"] == first_head
            && first["sourceTreeDirty"] == false
            && !release.exists()
            && !skill.join("assets/behavioral-test-v2.schema.json").exists(),
        "source discovery generated or mismatched metadata"
    );
    ensure!(
        first["contracts"]
            == serde_json::json!({"catalog":1,"behavioralTest":2,"messageSeries":2,"fixture":1}),
        "source contracts mismatch"
    );
    ensure!(
        process::run(&["git", "status", "--porcelain"], &source, &env)?.is_empty(),
        "source discovery dirtied checkout"
    );
    commit(&source, &env)?;
    let second_head = head(&source, &env)?;
    build(&source, &target, &env)?;
    ensure!(
        first_head != second_head
            && info(&native, &source, &empty)?["sourceRevision"] == second_head
            && !release.exists(),
        "HEAD change left stale build metadata"
    );
    fs::write(source.join(".cargo-ok"), "{}")?;
    build(&source, &target, &env)?;
    ensure!(
        info(&native, &source, &empty)?["sourceTreeDirty"] == false,
        "Cargo marker made source dirty"
    );
    let instructions = skill.join("SKILL.md");
    fs::write(
        &instructions,
        format!(
            "{}\nSource freshness regression\n",
            fs::read_to_string(&instructions)?
        ),
    )?;
    build(&source, &target, &env)?;
    let installed = parent.join("installed");
    command(
        &[
            &native.display().to_string(),
            "rostfrei",
            "skills",
            "install",
            "--destination",
            &installed.display().to_string(),
        ],
        parent,
        &empty,
    )?
    .checked()?;
    ensure!(
        fs::read_to_string(installed.join("rostfrei-develop-feature/SKILL.md"))?
            .ends_with("Source freshness regression\n")
            && read_json(&installed.join("rostfrei-develop-feature/references/release.json"))?["sourceTreeDirty"]
                == true
            && !release.exists(),
        "source edit was not reflected by native rebuild"
    );
    let main = source.join("crates/rostfrei-structure/src/main.rs");
    fs::write(
        &main,
        format!("{}\n// consumer modification\n", fs::read_to_string(&main)?),
    )?;
    build(&source, &target, &env)?;
    ensure!(
        info(&native, &source, &empty)?["sourceTreeDirty"] == true,
        "actual source modification hidden"
    );
    let mut metadata = info(&cli(), &source, &empty)?;
    metadata["sourceRevision"] = serde_json::json!("0".repeat(40));
    let edited = serde_json::to_vec(&metadata)?;
    fs::write(&release, &edited)?;
    empty.insert(
        "ROSTFREI_TRACER_URL".into(),
        "unsupported://source-regression".into(),
    );
    let result = command(
        &[
            &cli().display().to_string(),
            "rostfrei",
            "test",
            &skill
                .join("assets/add-first-bicycle.json")
                .display()
                .to_string(),
            "--output",
            &parent.join("unused.json").display().to_string(),
        ],
        &source,
        &empty,
    )?;
    ensure!(
        result.code == 2
            && result.stderr.contains("release or protocol mismatch")
            && fs::read(&release)? == edited,
        "stale legacy metadata was not preserved and rejected"
    );
    println!(
        "Source skills: fresh discovery, revision changes, dirty edits, Cargo marker and Git maintenance isolation passed"
    );
    Ok(())
}
