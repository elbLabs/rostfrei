use crate::{
    array, capabilities::Capabilities, environment, live, nonce, process, root, target, text,
    versions,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{fs, time::Duration};

pub fn new_project() -> Result<()> {
    let mut env = environment();
    env.insert(
        "CARGO_TARGET_DIR".into(),
        crate::absolute(&target())?.display().to_string(),
    );
    let temporary = tempfile::Builder::new()
        .prefix("rostfrei-starter-")
        .tempdir()?;
    ensure!(
        !temporary.path().starts_with(root()),
        "TMPDIR must be outside the Rostfrei checkout"
    );
    let project = temporary.path().join("review-starter");
    process::run(
        &[
            "cargo",
            "run",
            "--locked",
            "-p",
            "rostfrei-structure",
            "--bin",
            "cargo-rostfrei",
            "--",
            "rostfrei",
            "new",
            &project.display().to_string(),
        ],
        &root(),
        &env,
    )?;
    let manifest_path = project.join("Cargo.toml");
    let original = fs::read(&manifest_path)?;
    let manifest = versions::read(&manifest_path)?;
    let dependency = &manifest["dependencies"]["rostfrei"];
    ensure!(
        dependency == &manifest["dependencies"]["rostfrei-nats"],
        "Both Rostfrei dependencies must use the same release pin"
    );
    let git = dependency["git"].as_str().context("public git pin")?;
    let revision = dependency["rev"].as_str().context("public revision")?;
    let expected = format!("git+{git}?rev={revision}#{revision}");
    for args in [&["cargo", "check"][..], &["cargo", "test", "--locked"][..]] {
        process::run(args, &project, &env)?;
    }
    let metadata: Value = serde_json::from_str(&process::run(
        &["cargo", "metadata", "--locked", "--format-version", "1"],
        &project,
        &env,
    )?)?;
    let packages = array(&metadata["packages"])?
        .iter()
        .filter(|p| {
            p["name"]
                .as_str()
                .is_some_and(|n| n.starts_with("rostfrei"))
        })
        .collect::<Vec<_>>();
    for required in ["rostfrei", "rostfrei-nats"] {
        ensure!(
            packages.iter().any(|p| p["name"] == required),
            "missing generated dependency {required}"
        );
    }
    for package in packages {
        ensure!(
            package["source"] == expected,
            "{} resolved outside the public release pin",
            package["name"]
        );
    }
    ensure!(
        fs::read(manifest_path)? == original,
        "Generated manifest changed during acceptance"
    );
    println!("External starter passed check and test using {expected}");
    Ok(())
}
pub fn tracer_host(working_tree: bool) -> Result<()> {
    let revision = process::run(&["git", "rev-parse", "HEAD"], &root(), &environment())?
        .trim()
        .to_owned();
    let temporary = tempfile::Builder::new()
        .prefix("rostfrei-host-")
        .tempdir()?;
    let consumer = temporary.path().join("consumer");
    crate::copy_tree(
        &root().join("crates/rostfrei-tracer/tests/consumer"),
        &consumer,
    )?;
    let path = consumer.join("Cargo.toml");
    let dependency = if working_tree {
        format!("path = {:?}", root().join("crates/rostfrei"))
    } else {
        format!(
            "git = {:?}, rev = {revision:?}",
            format!("file://{}", root().display())
        )
    };
    let manifest = fs::read_to_string(&path)?.replace("path = \"../../../rostfrei\"", &dependency);
    fs::write(path, manifest)?;
    let mut env = live::host_environment("entry")?;
    let target = root().join("target/host-consumer");
    env.insert("CARGO_TARGET_DIR".into(), target.display().to_string());
    process::run(&["cargo", "test", "--", "--nocapture"], &consumer, &env)?;
    process::run(
        &[
            "cargo",
            "build",
            "--locked",
            "--bin",
            "tracer-host-consumer",
        ],
        &consumer,
        &env,
    )?;
    let mut host = process::background(
        &[target
            .join("debug/tracer-host-consumer")
            .display()
            .to_string()],
        &consumer,
        &env,
        &temporary.path().join("host.log"),
    )?;
    let result = (|| {
        let catalog = live::ready(&mut host, &env, Duration::from_secs(30))?;
        let client = Capabilities::new(&env, temporary.path())?;
        let definitions =
            client.get(text(&catalog["behavioralTest"]["definitionsHref"])?, "test")?;
        let definition = array(&definitions["items"])?
            .first()
            .context("standalone definition")?;
        ensure!(
            client
                .request(
                    text(&definition["runHref"])?,
                    "test",
                    Some(&json!({})),
                    200,
                    None,
                    None
                )?
                .0["status"]
                == "passed",
            "standalone suite failed"
        );
        Ok::<_, anyhow::Error>(())
    })();
    let stopped = host.stop(Duration::from_secs(20));
    result?;
    ensure!(stopped? == 0, "standalone shutdown failed");
    if !working_tree {
        let metadata: Value = serde_json::from_str(&process::run(
            &["cargo", "metadata", "--locked", "--format-version", "1"],
            &consumer,
            &env,
        )?)?;
        for package in array(&metadata["packages"])? {
            if text(&package["name"])?.starts_with("rostfrei") {
                ensure!(
                    package["source"]
                        .as_str()
                        .is_some_and(|s| s.ends_with(&format!("#{revision}"))),
                    "{} did not resolve from committed framework",
                    package["name"]
                );
            }
        }
    }
    println!("Standalone host: external tests, discovery, suite and shutdown passed");
    Ok(())
}
pub fn bike_host(studio: bool) -> Result<()> {
    ensure!(
        std::env::var("ROSTFREI_NATS_URL").is_ok_and(|s| !s.is_empty()),
        "Use cargo xtask test to supply a disposable broker"
    );
    process::run(
        &[
            "cargo",
            "build",
            "--locked",
            "-p",
            "bike-rental",
            "--bin",
            "bike-rental-api",
        ],
        &root(),
        &environment(),
    )?;
    let temporary = tempfile::tempdir()?;
    let env = live::host_environment("host")?;
    let mut host = process::background(
        &process::args(&[
            &target().join("debug/bike-rental-api").display().to_string(),
            "--tracer",
        ]),
        &root(),
        &env,
        &temporary.path().join("host.log"),
    )?;
    let result = (|| {
        let catalog = live::ready(&mut host, &env, Duration::from_secs(45))?;
        let mut client = Capabilities::new(&env, temporary.path())?;
        client.discover()?;
        let dispatch = |name: &str, key: &str, expected: &str| -> Result<()> {
            let command = array(&catalog["contexts"])?
                .iter()
                .flat_map(|c| c["commands"].as_array().into_iter().flatten())
                .find(|c| c["id"] == name)
                .context("bike command")?;
            let versions = array(&command["versions"])?;
            ensure!(versions.len() == 1, "command versions");
            client.normal_command(&json!({"context":"bike-rental","name":name,"schemaVersion":versions[0]["schemaVersion"],"payload":{"fleet_id":"city-fleet","bicycle_id":"bike-42"}}),key,expected)?;
            Ok(())
        };
        dispatch("rent-bicycle", "normal-before-test-reset", "accepted")?;
        let definitions =
            client.get(text(&catalog["behavioralTest"]["definitionsHref"])?, "test")?;
        ensure!(
            array(&definitions["items"])?.len() == 5,
            "expected five host definitions"
        );
        for definition in array(&definitions["items"])? {
            let report = client
                .request(
                    text(&definition["runHref"])?,
                    "test",
                    Some(&json!({})),
                    200,
                    None,
                    None,
                )?
                .0;
            ensure!(
                report["status"] == "passed" && report["comparison"]["status"] == "passed",
                "HTTP suite failed: {report}"
            );
            println!("HTTP passed: {}", definition["id"]);
        }
        dispatch("rent-bicycle", "normal-after-test-reset", "rejected")?;
        dispatch("return-bicycle", "normal-return", "accepted")?;
        if studio {
            let mut browser = env.clone();
            browser.insert(
                "VITE_TRACER_TARGET".into(),
                env["ROSTFREI_TRACER_URL"].clone(),
            );
            browser.insert(
                "VITE_TRACER_TOKEN".into(),
                env["ROSTFREI_API_TOKEN"].clone(),
            );
            process::run(
                &[
                    "node",
                    &root()
                        .join("scripts/check_tracer_studio.mjs")
                        .display()
                        .to_string(),
                ],
                &root().join("studio"),
                &browser,
            )?;
        }
        Ok::<_, anyhow::Error>(())
    })();
    let stopped = host.stop(Duration::from_secs(50));
    if result.is_err() {
        eprintln!(
            "{}",
            fs::read_to_string(temporary.path().join("host.log")).unwrap_or_default()
        );
    }
    result?;
    ensure!(stopped? == 0, "host shutdown failed");
    println!(
        "Application-linked host suite, Normal preservation and shutdown passed ({})",
        nonce()
    );
    Ok(())
}
