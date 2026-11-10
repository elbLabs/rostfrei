use super::{command, source};
use crate::{Environment, cli, environment, files, process, read_json, root, save_json, skills};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};

fn app(path: &Path, name: &str, dependency: &str) -> Result<()> {
    fs::create_dir_all(path.join("src"))?;
    fs::write(path.join("src/main.rs"), "fn main() {}\n")?;
    fs::write(
        path.join("Cargo.toml"),
        format!("[package]\nname={name:?}\nversion=\"0.1.0\"\nedition=\"2024\"\n{dependency}\n"),
    )?;
    Ok(())
}
fn lock(app: &Path, env: &Environment, offline: bool) -> Result<()> {
    let args = if offline {
        vec!["cargo", "generate-lockfile", "--offline"]
    } else {
        vec!["cargo", "generate-lockfile"]
    };
    command(&args, app, env)?.checked()?;
    Ok(())
}
fn setup(
    cli: &Path,
    app: &Path,
    env: &Environment,
    action: &str,
    args: &[&str],
) -> Result<process::Output> {
    let cli = cli.display().to_string();
    let mut command = vec![cli.as_str(), "rostfrei", "skills", action];
    command.extend_from_slice(args);
    super::command(&command, app, env)
}
fn tools(path: &Path) -> Result<Vec<PathBuf>> {
    Ok(walkdir::WalkDir::new(path)
        .into_iter()
        .collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|e| e.file_name() == "cargo-rostfrei")
        .map(|e| e.into_path())
        .collect())
}
fn tool_for(path: &Path, release: &Value, cwd: &Path, env: &Environment) -> Result<PathBuf> {
    let mut matches = Vec::new();
    for tool in tools(path)? {
        if source::info(&tool, cwd, env)? == *release {
            matches.push(tool);
        }
    }
    ensure!(matches.len() == 1, "expected one matching cached tool");
    Ok(matches.remove(0))
}
pub fn run() -> Result<()> {
    let temporary = tempfile::Builder::new()
        .prefix("rostfrei-onboarding-")
        .tempdir()?;
    let base = temporary.path();
    let mut env = environment();
    env.insert(
        "ROSTFREI_TOOLS_DIR".into(),
        base.join("tools").display().to_string(),
    );
    env.insert(
        "CARGO_TARGET_DIR".into(),
        root().join("target/onboarding-local").display().to_string(),
    );
    let dependency = format!(
        "[dependencies]\nrostfrei={{path={:?}}}",
        root().join("crates/rostfrei")
    );
    let renamed = base.join("renamed");
    fs::create_dir_all(&renamed)?;
    fs::write(
        renamed.join("Cargo.toml"),
        format!(
            "[workspace]\nresolver=\"3\"\nmembers=[\"application\"]\n[workspace.dependencies]\nframework={{package=\"rostfrei\",path={:?}}}\n",
            root().join("crates/rostfrei")
        ),
    )?;
    let consumer = renamed.join("application");
    app(
        &consumer,
        "consumer",
        "[dependencies]\nframework.workspace=true",
    )?;
    lock(&consumer, &env, true)?;
    setup(&cli(), &consumer, &env, "install", &[])?.checked()?;
    ensure!(
        !renamed.join(".agents").exists(),
        "installed at workspace root instead of selected app"
    );
    let info = source::info(&cli(), &consumer, &env)?;
    for name in skills::NAMES {
        ensure!(
            read_json(&consumer.join(format!(".agents/skills/{name}/references/release.json")))?
                == info,
            "renamed dependency release mismatch"
        );
    }
    setup(&cli(), &consumer, &env, "install", &[])?.checked()?;
    let plain = base.join("plain");
    app(&plain, "plain-app", "")?;
    lock(&plain, &env, true)?;
    let result = setup(&cli(), &plain, &env, "install", &[])?;
    ensure!(
        result.code != 0
            && result.stderr.contains("no resolved rostfrei dependency")
            && !plain.join(".agents").exists(),
        "missing dependency not rejected"
    );
    let workspace = base.join("workspace");
    fs::create_dir(&workspace)?;
    fs::write(
        workspace.join("Cargo.toml"),
        "[workspace]\nresolver=\"3\"\nmembers=[\"one\",\"two\"]\n",
    )?;
    for name in ["one", "two"] {
        app(&workspace.join(name), name, &dependency)?;
    }
    lock(&workspace, &env, true)?;
    let result = setup(&cli(), &workspace, &env, "install", &[])?;
    ensure!(
        result.code != 0 && result.stderr.contains("--package"),
        "ambiguous workspace accepted"
    );
    setup(
        &cli(),
        &workspace,
        &env,
        "install",
        &[
            "--manifest-path",
            &workspace.join("Cargo.toml").display().to_string(),
            "--package",
            "two",
            "--destination",
            "selected-skills",
        ],
    )?
    .checked()?;
    ensure!(
        workspace
            .join("selected-skills/rostfrei-tracer/SKILL.md")
            .is_file()
            && !workspace.join("one/.agents").exists()
            && !workspace.join("two/.agents").exists(),
        "workspace selection/destination mismatch"
    );
    let rooted = base.join("rooted");
    app(
        &rooted,
        "root-tool",
        "[workspace]\nresolver=\"3\"\nmembers=[\"app\",\"other\"]\ndefault-members=[\"app\"]",
    )?;
    app(&rooted.join("app"), "app", &dependency)?;
    app(&rooted.join("other"), "other", "")?;
    lock(&rooted, &env, true)?;
    setup(&cli(), &rooted, &env, "install", &[])?.checked()?;
    ensure!(
        rooted
            .join("app/.agents/skills/rostfrei-tracer/SKILL.md")
            .is_file()
            && !rooted.join(".agents").exists(),
        "default member not honored"
    );
    for (cwd, args) in [
        (rooted.join("other"), Vec::<String>::new()),
        (
            rooted.clone(),
            vec![
                "--manifest-path".into(),
                rooted.join("other/Cargo.toml").display().to_string(),
            ],
        ),
        (rooted.clone(), vec!["--package".into(), "root-tool".into()]),
    ] {
        let result = setup(
            &cli(),
            &cwd,
            &env,
            "install",
            &args.iter().map(String::as_str).collect::<Vec<_>>(),
        )?;
        ensure!(
            result.code != 0 && result.stderr.contains("no resolved rostfrei dependency"),
            "explicit package selection was ignored"
        );
    }
    let manifest = rooted.join("Cargo.toml");
    fs::write(
        &manifest,
        fs::read_to_string(&manifest)?.replace(
            "default-members=[\"app\"]",
            "default-members=[\"app\",\"other\"]",
        ),
    )?;
    let result = setup(&cli(), &rooted, &env, "install", &[])?;
    ensure!(
        result.code != 0 && result.stderr.contains("--package"),
        "ambiguous defaults accepted"
    );
    setup(&cli(), &rooted, &env, "install", &["--package", "app"])?.checked()?;
    let starter = base.join("published-starter");
    let result = command(
        &[
            &cli().display().to_string(),
            "rostfrei",
            "new",
            &starter.display().to_string(),
        ],
        base,
        &env,
    )?
    .checked()?;
    ensure!(
        result.stdout.contains("predates skills-enabled tooling"),
        "starter capability gap not reported"
    );
    lock(&starter, &env, false)?;
    env.insert("CARGO_PROFILE_RELEASE_OPT_LEVEL".into(), "0".into());
    env.insert("CARGO_PROFILE_RELEASE_DEBUG".into(), "0".into());
    let result = setup(&cli(), &starter, &env, "install", &[])?;
    ensure!(
        result.code != 0
            && result.stderr.contains("predates skills-enabled tooling")
            && !starter.join(".agents").exists(),
        "pre-kit starter incorrectly installed skills"
    );
    retained_runner(&base.join("retained"), &env)?;
    versioned(&base.join("versioned"), &env)?;
    println!(
        "Onboarding: real Cargo resolution, selection, retained runners, upgrades and atomic edit preservation passed"
    );
    Ok(())
}
fn retained_runner(base: &Path, parent_env: &Environment) -> Result<()> {
    fs::create_dir(base)?;
    let mut env = parent_env.clone();
    env.insert(
        "ROSTFREI_TOOLS_DIR".into(),
        base.join("tools").display().to_string(),
    );
    let source = base.join("framework");
    source::fresh(&source, &env)?;
    let target = root().join("target/onboarding-tools");
    let global = base.join("cargo-rostfrei");
    fs::copy(source::build(&source, &target, &env)?, &global)?;
    let first = source::info(&global, base, &env)?;
    ensure!(first["sourceTreeDirty"] == false, "fixture dirty");
    let application = base.join("app");
    app(
        &application,
        "retained-runner",
        &format!(
            "[dependencies]\nrostfrei={{git={:?},rev={}}}",
            format!("file://{}", source.display()),
            first["sourceRevision"]
        ),
    )?;
    lock(&application, &env, false)?;
    process::run(
        &["cargo", "metadata", "--locked", "--format-version", "1"],
        &application,
        &env,
    )?;
    let cargo = process::executable("cargo")?;
    let wrappers = base.join("metadata-only");
    fs::create_dir(&wrappers)?;
    let wrapper = wrappers.join("cargo");
    fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\n[ \"$1\" = metadata ] || exit 91\nexec {:?} \"$@\"\n",
            cargo
        ),
    )?;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755))?;
    let mut metadata_env = env.clone();
    metadata_env.insert(
        "PATH".into(),
        format!(
            "{}:{}",
            wrappers.display(),
            env.get("PATH").map_or("", String::as_str)
        ),
    );
    metadata_env.insert("CARGO_NET_OFFLINE".into(), "true".into());
    command(
        &["cargo", "metadata", "--locked", "--format-version", "1"],
        &application,
        &metadata_env,
    )?
    .checked()?;
    setup(&global, &application, &metadata_env, "install", &[])?.checked()?;
    let retained = tools(&base.join("tools"))?;
    ensure!(
        retained.len() == 1,
        "matching global setup must retain its runner"
    );
    let cached = &retained[0];
    ensure!(
        !cached.is_symlink()
            && fs::read(cached)? == fs::read(&global)?
            && fs::metadata(cached)?.mode() == fs::metadata(&global)?.mode(),
        "retained binary mismatch"
    );
    let stat = fs::metadata(cached)?;
    setup(&global, &application, &metadata_env, "install", &[])?.checked()?;
    let now = fs::metadata(cached)?;
    ensure!(
        stat.ino() == now.ino()
            && stat.mtime() == now.mtime()
            && stat.mtime_nsec() == now.mtime_nsec(),
        "valid cache replaced"
    );
    let saved = base.join("runner-a");
    fs::copy(&global, &saved)?;
    source::commit(&source, &env)?;
    fs::copy(source::build(&source, &target, &env)?, &global)?;
    ensure!(
        source::info(&global, base, &env)?["sourceRevision"] != first["sourceRevision"],
        "global upgrade unchanged"
    );
    fs::remove_dir_all(&source)?;
    let definition =
        application.join(".agents/skills/rostfrei-develop-feature/assets/add-first-bicycle.json");
    let mut runtime = env.clone();
    runtime.insert("PATH".into(), String::new());
    runtime.insert("CARGO_NET_OFFLINE".into(), "true".into());
    runtime.insert(
        "ROSTFREI_TRACER_URL".into(),
        "unsupported://retained-runner-sentinel".into(),
    );
    routed(&global, &application, &runtime, &definition, None, true)?;
    ensure!(
        source::info(cached, &application, &runtime)? == first,
        "retained runner changed after global upgrade"
    );
    fs::copy(&global, cached)?;
    let collision = fs::read(cached)?;
    let baseline = fs::read(application.join(".agents/skills/.rostfrei-managed.json"))?;
    let result = setup(&saved, &application, &metadata_env, "install", &[])?;
    ensure!(
        result.code != 0
            && result.stderr.contains("mismatch")
            && fs::read(cached)? == collision
            && fs::read(application.join(".agents/skills/.rostfrei-managed.json"))? == baseline,
        "cache collision changed executable or baseline"
    );
    Ok(())
}
fn routed(
    cli: &Path,
    app: &Path,
    env: &Environment,
    definition: &Path,
    release: Option<&Path>,
    configuration: bool,
) -> Result<()> {
    let output = app.join("never-written.json");
    let mut args = process::args(&[
        &cli.display().to_string(),
        "rostfrei",
        "test",
        &definition.display().to_string(),
        "--output",
        &output.display().to_string(),
    ]);
    if let Some(release) = release {
        args.extend(process::args(&[
            "--release",
            &release.display().to_string(),
        ]));
    }
    let result = process::capture(
        &args,
        app,
        env,
        std::time::Duration::from_secs(120),
        &[],
        false,
    )?;
    ensure!(
        result.code == 2,
        "routing should stop before submission: {}",
        result.stderr
    );
    let error: Value = serde_json::from_str(&result.stderr)?;
    ensure!(
        (error["error"]["phase"] == "configuration") == configuration && !output.exists(),
        "routing identity/configuration boundary mismatch: {}",
        result.stderr
    );
    Ok(())
}
fn versioned(base: &Path, parent_env: &Environment) -> Result<()> {
    fs::create_dir(base)?;
    let source = base.join("framework");
    let mut env = parent_env.clone();
    let cache = base.join("tools");
    env.insert("ROSTFREI_TOOLS_DIR".into(), cache.display().to_string());
    env.insert(
        "CARGO_TARGET_DIR".into(),
        root().join("target/onboarding-tools").display().to_string(),
    );
    source::fresh(&source, &env)?;
    let first = source::head(&source, &env)?;
    let application = base.join("app");
    app(
        &application,
        "versioned-consumer",
        &format!(
            "[dependencies]\nframework={{package=\"rostfrei\",git={:?},rev={first:?}}}",
            format!("file://{}", source.display())
        ),
    )?;
    lock(&application, &env, false)?;
    setup(&cli(), &application, &env, "install", &[])?.checked()?;
    let skills = application.join(".agents/skills");
    let develop = skills.join("rostfrei-develop-feature/references/worked-example.md");
    let tracer = skills.join("rostfrei-tracer/SKILL.md");
    let baseline = skills.join(".rostfrei-managed.json");
    let metadata = skills.join("rostfrei-tracer/references/release.json");
    let original_develop = fs::read(&develop)?;
    let original_tracer = fs::read(&tracer)?;
    let original_baseline = fs::read(&baseline)?;
    let release = read_json(&metadata)?;
    ensure!(
        release["sourceRevision"] == first
            && release["sourceRevision"]
                != source::info(&cli(), &application, &env)?["sourceRevision"],
        "versioned setup used global identity"
    );
    setup(&cli(), &application, &env, "install", &[])?.checked()?;
    fs::write(
        &tracer,
        [
            original_tracer.clone(),
            b"\nLocal consumer decision\n".to_vec(),
        ]
        .concat(),
    )?;
    let notes = skills.join("rostfrei-tracer/user-notes.md");
    fs::write(&notes, "keep my notes\n")?;
    let worked =
        source.join(".agents/skills/rostfrei-develop-feature/references/worked-example.md");
    fs::write(
        &worked,
        format!("{}\nUpstream revision B\n", fs::read_to_string(&worked)?),
    )?;
    let runtime = source.join(".agents/skills/rostfrei-tracer/references/runtime.md");
    fs::rename(&runtime, runtime.with_file_name("runtime-next.md"))?;
    let instructions = source.join(".agents/skills/rostfrei-tracer/SKILL.md");
    fs::write(
        &instructions,
        fs::read_to_string(&instructions)?
            .replace("references/runtime.md", "references/runtime-next.md"),
    )?;
    let link = source.join("crates/rostfrei-structure/resources/runtime.md");
    fs::remove_file(&link)?;
    std::os::unix::fs::symlink(
        "../../../.agents/skills/rostfrei-tracer/references/runtime-next.md",
        &link,
    )?;
    let inputs = source.join("crates/rostfrei-structure/src/agent_tools/bundle_files.rs");
    fs::write(
        &inputs,
        fs::read_to_string(&inputs)?.replace(
            "rostfrei-tracer/references/runtime.md",
            "rostfrei-tracer/references/runtime-next.md",
        ),
    )?;
    process::run(&["git", "add", "."], &source, &env)?;
    source::commit(&source, &env)?;
    let second = source::head(&source, &env)?;
    let manifest = application.join("Cargo.toml");
    fs::write(
        &manifest,
        fs::read_to_string(&manifest)?.replace(&first, &second),
    )?;
    lock(&application, &env, false)?;
    let result = setup(&cli(), &application, &env, "install", &[])?;
    ensure!(
        result.code != 0 && result.stderr.contains("skills update"),
        "upgrade silently installed"
    );
    env.insert("CARGO_NET_OFFLINE".into(), "true".into());
    let result = setup(&cli(), &application, &env, "update", &[])?;
    ensure!(
        result.code != 0
            && result.stderr.contains("Preserving")
            && fs::read(&develop)? == original_develop
            && fs::read(&baseline)? == original_baseline
            && fs::read_to_string(&tracer)?.ends_with("Local consumer decision\n")
            && skills
                .join("rostfrei-tracer/references/runtime.md")
                .exists(),
        "conflict changed managed resources"
    );
    fs::write(&tracer, &original_tracer)?;
    setup(&cli(), &application, &env, "update", &[])?.checked()?;
    ensure!(
        fs::read_to_string(&develop)?.ends_with("Upstream revision B\n")
            && !skills
                .join("rostfrei-tracer/references/runtime.md")
                .exists()
            && skills
                .join("rostfrei-tracer/references/runtime-next.md")
                .is_file()
            && fs::read_to_string(&notes)? == "keep my notes\n",
        "resolved update did not atomically rename resources"
    );
    setup(&cli(), &application, &env, "update", &[])?.checked()?;
    let immutable_release = read_json(&metadata)?;
    ensure!(
        immutable_release["sourceRevision"] == second,
        "updated revision mismatch"
    );
    let definition = skills.join("rostfrei-develop-feature/assets/add-first-bicycle.json");
    let mut runtime_env = env.clone();
    runtime_env.insert("PATH".into(), String::new());
    runtime_env.insert(
        "ROSTFREI_TRACER_URL".into(),
        "unsupported://routing-check".into(),
    );
    routed(&cli(), &application, &runtime_env, &definition, None, true)?;
    let selected = application.join("selected-release.json");
    let mut invalid = immutable_release.clone();
    invalid["skills"] = json!(["unexpected-skill"]);
    save_json(&selected, &invalid)?;
    routed(
        &cli(),
        &application,
        &runtime_env,
        &definition,
        Some(&selected),
        false,
    )?;
    let single = fs::read_to_string(&manifest)?;
    fs::write(
        &manifest,
        format!(
            "{single}other={{package=\"rostfrei\",git={:?},rev={first:?}}}\n",
            format!("file://{}", source.display())
        ),
    )?;
    lock(&application, &env, false)?;
    let result = setup(&cli(), &application, &env, "update", &[])?;
    ensure!(
        result.code != 0 && result.stderr.contains("multiple rostfrei"),
        "multiple revisions selected arbitrarily"
    );
    let immutable_tool = tool_for(&cache, &immutable_release, &application, &env)?;
    let immutable_binary = fs::read(&immutable_tool)?;
    fs::write(
        &worked,
        format!(
            "{}\nUncommitted local source edit\n",
            fs::read_to_string(&worked)?
        ),
    )?;
    app(
        &application,
        "versioned-consumer",
        &format!(
            "[dependencies]\nframework={{package=\"rostfrei\",path={:?}}}",
            source.join("crates/rostfrei")
        ),
    )?;
    lock(&application, &env, false)?;
    setup(&cli(), &application, &env, "update", &[])?.checked()?;
    let local_release = read_json(&metadata)?;
    ensure!(
        local_release["sourceTreeDirty"] == true
            && fs::read_to_string(&develop)?.ends_with("Uncommitted local source edit\n")
            && fs::read(&immutable_tool)? == immutable_binary,
        "mutable setup overwrote immutable tool"
    );
    let local_tool = tool_for(&cache, &local_release, &application, &env)?;
    let other_source = base.join("second-local-source");
    crate::copy_tree(&source, &other_source)?;
    let other_worked = other_source.join(worked.strip_prefix(&source)?);
    fs::write(
        &other_worked,
        format!(
            "{}\nDifferent local worktree edit\n",
            fs::read_to_string(&other_worked)?
        ),
    )?;
    fs::write(
        &manifest,
        fs::read_to_string(&manifest)?.replace(
            &source.display().to_string(),
            &other_source.display().to_string(),
        ),
    )?;
    lock(&application, &env, false)?;
    setup(&cli(), &application, &env, "update", &[])?.checked()?;
    let other_release = read_json(&metadata)?;
    ensure!(
        other_release != local_release
            && source::info(&local_tool, &application, &env)? == local_release
            && fs::read(&immutable_tool)? == immutable_binary,
        "dirty worktrees collided"
    );
    let isolated = base.join("isolated-tools");
    for expected in [&immutable_release, &other_release] {
        save_json(&selected, expected)?;
        let correct = tool_for(&cache, expected, &application, &env)?;
        let relative = correct.strip_prefix(&cache)?;
        let counterfeit = isolated.join(relative);
        fs::create_dir_all(counterfeit.parent().context("counterfeit parent")?)?;
        fs::copy(&local_tool, &counterfeit)?;
        let mut counterfeit_env = runtime_env.clone();
        counterfeit_env.insert("ROSTFREI_TOOLS_DIR".into(), isolated.display().to_string());
        for runner in [&local_tool, &cli()] {
            routed(
                runner,
                &application,
                &counterfeit_env,
                &definition,
                Some(&selected),
                false,
            )?;
        }
    }
    fs::write(&manifest, single)?;
    lock(&application, &env, false)?;
    setup(&cli(), &application, &env, "update", &[])?.checked()?;
    ensure!(
        read_json(&metadata)? == immutable_release,
        "immutable selection was not restored"
    );
    let _ = files(&skills)?;
    Ok(())
}
