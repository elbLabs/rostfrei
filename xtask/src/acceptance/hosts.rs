use crate::{
    array, capabilities::Capabilities, environment, live, nonce, process, root, target, text,
    versions,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{fs, path::Path, time::Duration};

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
    let version = dependency["version"]
        .as_str()
        .and_then(|s| s.strip_prefix('='))
        .context("expected exact registry release")?;
    for args in [
        &["cargo", "check", "--all-targets"][..],
        &["cargo", "test", "--locked"][..],
        &["cargo", "fmt", "--check"][..],
    ] {
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
            package["source"] == "registry+https://github.com/rust-lang/crates.io-index",
            "{} resolved outside the public release pin",
            package["name"]
        );
        if package["name"] == "rostfrei" {
            ensure!(package["version"] == version, "wrong facade version");
            let crate_dir = Path::new(text(&package["manifest_path"])?)
                .parent()
                .context("registry crate directory")?;
            let vcs = crate::read_json(&crate_dir.join(".cargo_vcs_info.json"))?;
            let revision = text(&vcs["git"]["sha1"])?;
            for skill in ["rostfrei-tracer", "rostfrei-develop-feature"] {
                let file = project
                    .join(".agents/skills")
                    .join(skill)
                    .join("references/release.json");
                let identity = crate::read_json(&file)?;
                ensure!(
                    identity["sourceRevision"] == revision
                        && identity["frameworkVersion"] == version
                        && identity["sourceTreeDirty"] == false,
                    "{skill} does not match the published framework"
                );
            }
        }
    }
    ensure!(
        fs::read(manifest_path)? == original,
        "Generated manifest changed during acceptance"
    );
    let second = temporary.path().join("demo");
    process::run(
        &[
            &crate::absolute(&target())?
                .join("debug/cargo-rostfrei")
                .display()
                .to_string(),
            "rostfrei",
            "new",
            "--no-skills",
            &second.display().to_string(),
        ],
        &root(),
        &env,
    )?;
    process::run(&["cargo", "fmt", "--check"], &second, &env)?;
    process::run(&["cargo", "generate-lockfile", "--offline"], &second, &env)?;
    let broker = crate::nats::Broker::start(false, Duration::from_secs(45))?;
    env.insert(
        "ROSTFREI_NATS_URL".into(),
        broker
            .env
            .get("ROSTFREI_NATS_URL")
            .context("missing disposable broker URL")?
            .clone(),
    );
    // A second project's first rental must not replay the first project's decision.
    for (application, origin) in [
        (&project, "committed"),
        (&second, "committed"),
        (&project, "replayed"),
    ] {
        let output = process::run(
            &["cargo", "run", "--locked", "--example", "first_rental"],
            application,
            &env,
        )?;
        ensure!(
            output.lines().any(
                |line| line == format!("rental-1: {origin}; retry: replayed; stored events: 1")
            ),
            "expected {origin} rental and a single persisted event"
        );
    }
    broker.shutdown()?;
    println!(
        "External starter: matched {version} skills, built from crates.io, committed and replayed on NATS"
    );
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
        let contexts = array(&catalog["contexts"])?;
        ensure!(
            contexts.len() == 3,
            "expected three bounded contexts: {contexts:?}"
        );
        let context = |id: &str| {
            contexts
                .iter()
                .find(|context| context["id"] == id)
                .with_context(|| format!("missing bounded context {id}"))
        };
        for (id, expected) in [
            ("finance", &["finance/rental-payment"][..]),
            (
                "rental",
                &["rental/rental-fleet", "rental/rental-tariff"][..],
            ),
            ("workshop", &["workshop/repair-job"][..]),
        ] {
            let mut actual = array(&context(id)?["aggregates"])?
                .iter()
                .map(|aggregate| text(&aggregate["aggregateType"]))
                .collect::<Result<Vec<_>>>()?;
            actual.sort_unstable();
            ensure!(
                actual == expected,
                "{id} aggregates: expected {expected:?}, got {actual:?}"
            );
        }
        let mut client = Capabilities::new(&env, temporary.path())?;
        client.discover()?;
        let dispatch = |name: &str, key: &str, expected: &str| -> Result<()> {
            let command = array(&context("rental")?["commands"])?
                .iter()
                .find(|c| c["id"] == name)
                .context("bike command")?;
            let versions = array(&command["versions"])?;
            ensure!(versions.len() == 1, "command versions");
            client.normal_command(
                &json!({
                    "context": "rental", "name": name, "schemaVersion": versions[0]["schemaVersion"],
                    "payload": {"fleet_id":"city-fleet", "bicycle_id":"normal-registration", "condition":"serviceable"}
                }),
                key,
                expected,
            )?;
            Ok(())
        };
        ensure!(
            !contexts
                .iter()
                .flat_map(|c| c["commands"].as_array().into_iter().flatten())
                .any(|c| matches!(
                    c["id"].as_str(),
                    Some("rent-bicycle" | "process-rental-payment" | "settle-rental-deposit")
                )),
            "obsolete commands must not be advertised"
        );
        dispatch("register-bicycle", "normal-before-test-reset", "accepted")?;
        let definitions =
            client.get(text(&catalog["behavioralTest"]["definitionsHref"])?, "test")?;
        let definitions = array(&definitions["items"])?.as_slice();
        ensure!(definitions.len() == 7, "expected seven host definitions");
        let selected = std::env::var("ROSTFREI_BIKE_HOST_SCENARIO")
            .ok()
            .map(|id| {
                definitions
                    .iter()
                    .find(|definition| definition["id"] == id)
                    .with_context(|| format!("unknown bike-rental host scenario: {id}"))
            })
            .transpose()?;
        for definition in selected.map_or(definitions, std::slice::from_ref) {
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
            println!(
                "HTTP scenario passed: {} (command: {})",
                definition["id"], report["operation"]["command"]
            );
        }
        dispatch("register-bicycle", "normal-after-test-reset", "rejected")?;
        if studio {
            let mut browser = env.clone();
            if let Some(definition) = selected {
                browser.insert(
                    "ROSTFREI_BIKE_HOST_STUDIO_SCENARIO_NAME".into(),
                    text(&definition["name"])?.into(),
                );
            }
            browser.insert(
                "VITE_TRACER_TARGET".into(),
                env["ROSTFREI_TRACER_URL"].clone(),
            );
            browser.insert(
                "VITE_TRACER_TOKEN".into(),
                env["ROSTFREI_API_TOKEN"].clone(),
            );
            let output = process::run(
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
            print!("{output}");
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
