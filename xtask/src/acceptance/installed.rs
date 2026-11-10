use super::command;
use crate::{cli, digest, environment, files, read_json, root, skills};
use anyhow::{Context, Result, ensure};
use std::{fs, path::Path};

pub fn run() -> Result<()> {
    let temporary = tempfile::Builder::new()
        .prefix("rostfrei-skills-consumer-")
        .tempdir()?;
    let consumer = temporary.path();
    ensure!(!consumer.starts_with(root()), "consumer must be external");
    let native = consumer.join("cargo-rostfrei");
    fs::copy(cli(), &native)?;
    let archive = skills::package(&consumer.join("build"), false)?;
    let checksum = fs::read_to_string(archive.with_extension("gz.sha256"))?;
    ensure!(
        checksum
            == format!(
                "{}  {}\n",
                digest(&archive)?,
                archive
                    .file_name()
                    .context("archive name")?
                    .to_string_lossy()
            ),
        "kit checksum mismatch"
    );
    let original_digest = digest(&archive)?;
    ensure!(
        digest(&skills::package(&consumer.join("again"), false)?)? == original_digest,
        "kit build is not reproducible"
    );
    let kit = skills::extract(&archive, &consumer.join("extracted"))?;
    let relocated = consumer.join("relocated kit");
    fs::rename(kit, &relocated)?;
    fs::remove_dir_all(consumer.join("build"))?;
    let mut env = environment();
    env.insert("PATH".into(), String::new());
    let install = |cwd: &Path, args: &[&str], success: bool| -> Result<crate::process::Output> {
        let native = native.display().to_string();
        let mut all = vec![native.as_str(), "rostfrei", "skills", "install"];
        all.extend_from_slice(args);
        let result = command(&all, cwd, &env)?;
        ensure!(
            (result.code == 0) == success,
            "unexpected install result: {}{}",
            result.stdout,
            result.stderr
        );
        Ok(result)
    };
    let destination = consumer.join("app/.agents/skills");
    let destination_text = destination.display().to_string();
    let kit_text = relocated.display().to_string();
    install(
        consumer,
        &["--kit", &kit_text, "--destination", &destination_text],
        true,
    )?;
    install(
        consumer,
        &["--kit", &kit_text, "--destination", &destination_text],
        true,
    )?;
    let release = read_json(&relocated.join("release.json"))?;
    let tutorial = "rostfrei-tracer/references/guided-tutorial.md";
    let expected_tutorial = fs::read(root().join(".agents/skills").join(tutorial))?;
    ensure!(
        read_json(&relocated.join("manifest.json"))?["sha256"][format!("skills/{tutorial}")]
            == crate::hash(&expected_tutorial),
        "tutorial missing from archive manifest"
    );
    for name in skills::NAMES {
        let skill = destination.join(name);
        skills::validate(&skill)?;
        skills::validate_links(&skill)?;
        ensure!(
            read_json(&skill.join("references/release.json"))? == release,
            "installed release mismatch"
        );
        ensure!(
            fs::read(skill.join("LICENSE"))? == fs::read(root().join("LICENSE"))?,
            "installed license mismatch"
        );
    }
    // Independent flags, including a default destination without Cargo or Git.
    let override_path = consumer.join("custom/skills");
    install(
        consumer,
        &["--destination", &override_path.display().to_string()],
        true,
    )?;
    install(consumer, &["--kit", &kit_text], true)?;
    for name in skills::NAMES {
        ensure!(
            files(&override_path.join(name))?
                == files(&consumer.join(".agents/skills").join(name))?,
            "independent override mismatch"
        );
    }
    let instructions = destination
        .parent()
        .context("skills parent")?
        .parent()
        .context("app parent")?
        .join("AGENTS.md");
    fs::write(&instructions, "Consumer instructions\n")?;
    let unrelated = destination.join("local-skill/SKILL.md");
    fs::create_dir_all(unrelated.parent().context("local skill parent")?)?;
    fs::write(&unrelated, "Local skill\n")?;
    let modified = destination.join("rostfrei-tracer/SKILL.md");
    let original = fs::read_to_string(&modified)?;
    fs::write(&modified, format!("{original}\nConsumer addition\n"))?;
    fs::remove_dir_all(destination.join(skills::NAMES[0]))?;
    let refused = install(
        consumer,
        &["--kit", &kit_text, "--destination", &destination_text],
        false,
    )?;
    ensure!(
        refused.stderr.to_lowercase().contains("preserv")
            && !destination.join(skills::NAMES[0]).exists()
            && fs::read_to_string(&modified)?.ends_with("Consumer addition\n"),
        "install did not preserve edits atomically"
    );
    ensure!(
        fs::read_to_string(&instructions)? == "Consumer instructions\n"
            && fs::read_to_string(&unrelated)? == "Local skill\n",
        "unrelated consumer work changed"
    );
    let user_work = consumer.join("user-work");
    fs::create_dir(&user_work)?;
    std::os::unix::fs::symlink(&user_work, destination.join(skills::NAMES[0]))?;
    install(
        consumer,
        &["--kit", &kit_text, "--destination", &destination_text],
        false,
    )?;
    ensure!(
        fs::read_dir(&user_work)?.next().is_none(),
        "symlink target changed"
    );
    fs::remove_file(destination.join(skills::NAMES[0]))?;
    let expected = files(&relocated.join("skills"))?;
    fs::write(
        relocated.join("skills/rostfrei-develop-feature/SKILL.md"),
        "corrupted",
    )?;
    let other = consumer.join("other");
    fs::create_dir(&other)?;
    install(&other, &["--kit", &kit_text], false)?;
    ensure!(
        !other.join(".agents").exists(),
        "corrupt kit partially installed"
    );
    fs::remove_dir_all(&relocated)?;
    let moved_app = consumer.join("moved application");
    fs::rename(consumer.join("app"), &moved_app)?;
    let moved_skills = moved_app.join(".agents/skills");
    ensure!(
        !moved_skills.join(tutorial).is_symlink()
            && fs::read(moved_skills.join(tutorial))? == expected_tutorial
            && fs::read_to_string(moved_skills.join("rostfrei-tracer/SKILL.md"))?
                .contains("](references/guided-tutorial.md)"),
        "guided tutorial did not survive application relocation"
    );
    skills::validate_links(&moved_skills.join("rostfrei-tracer"))?;
    // Bundled installation survives removal of the extracted archive and tools.
    let standalone = consumer.join("standalone");
    fs::create_dir(&standalone)?;
    fs::write(standalone.join("AGENTS.md"), "consumer instructions\n")?;
    let bundled = standalone.join(".agents/skills");
    let bundled_text = bundled.display().to_string();
    install(&standalone, &["--destination", &bundled_text], true)?;
    let mut actual = files(&bundled)?;
    actual.remove(".rostfrei-managed.json");
    ensure!(
        actual == expected,
        "bundled resources differ from released archive"
    );
    ensure!(
        !standalone.join("Cargo.toml").exists(),
        "standalone installation created a project"
    );
    install(&standalone, &["--destination", &bundled_text], true)?;
    fs::write(bundled.join("rostfrei-tracer/SKILL.md"), "consumer edits\n")?;
    fs::remove_dir_all(bundled.join(skills::NAMES[0]))?;
    install(&standalone, &["--destination", &bundled_text], false)?;
    ensure!(
        !bundled.join(skills::NAMES[0]).exists()
            && fs::read_to_string(standalone.join("AGENTS.md"))? == "consumer instructions\n",
        "bundled preflight changed consumer state"
    );
    // Untracked local resources remain local and untouched.
    let local = tempfile::Builder::new()
        .prefix(".package-regression-")
        .tempdir_in(root().join(".agents/skills/rostfrei-develop-feature"))?;
    fs::write(local.path().join("local.md"), "consumer work")?;
    let packaged = skills::package(&consumer.join("untracked"), false)?;
    let extracted = skills::extract(&packaged, &consumer.join("untracked-extracted"))?;
    let local_name = local
        .path()
        .file_name()
        .context("local name")?
        .to_string_lossy();
    ensure!(
        !files(&extracted)?
            .keys()
            .any(|p| p.contains(local_name.as_ref())),
        "untracked resources shipped"
    );
    ensure!(
        fs::read_to_string(local.path().join("local.md"))? == "consumer work",
        "local resource changed"
    );
    println!(
        "Installed kit: reproducibility, relocation, standalone setup, links and atomic preservation passed"
    );
    Ok(())
}
