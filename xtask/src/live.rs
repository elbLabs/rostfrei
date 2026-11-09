use crate::{
    Environment, array,
    candidate::{self, Recorder},
    capabilities::Capabilities,
    cli, digest, environment, nats, nonce, process, read_json, root, save_json, skills, text,
    versions,
};
use anyhow::{Context, Result, ensure};
use clap::Args;
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

#[derive(Args)]
pub struct Options {
    #[arg(long)]
    pub output: PathBuf,
    #[arg(long, conflicts_with_all=["external_app","host_command"])]
    pub source_checkout: bool,
    #[arg(long)]
    pub require_handler_simulation: bool,
    #[arg(long)]
    pub external_app: Option<PathBuf>,
    #[arg(long)]
    pub host_build_provenance: Option<PathBuf>,
    #[arg(long, num_args=1.., allow_hyphen_values=true, requires="host_build_provenance")]
    pub host_command: Vec<String>,
}
pub fn host_environment(prefix: &str) -> Result<Environment> {
    let port = nats::port()?;
    let mut env = environment();
    for (key, value) in [
        ("ROSTFREI_APPLICATION", format!("{prefix}-{}", nonce())),
        (
            "ROSTFREI_COMPLETED_OUTCOME_EPOCH",
            format!("{prefix}-{}", nonce()),
        ),
        ("ROSTFREI_API_ADDR", format!("127.0.0.1:{port}")),
        ("ROSTFREI_TRACER_URL", format!("http://127.0.0.1:{port}")),
        ("ROSTFREI_API_TOKEN", nonce()),
        ("ROSTFREI_DISPATCH_TOKEN", nonce()),
        ("ROSTFREI_INSPECTION_TOKEN", nonce()),
    ] {
        env.insert(key.into(), value);
    }
    Ok(env)
}
pub fn ready(
    host: &mut process::OwnedChild,
    env: &Environment,
    timeout: Duration,
) -> Result<Value> {
    let deadline = Instant::now() + timeout;
    let client = nats::client(Duration::from_secs(1))?;
    loop {
        process::cancelled()?;
        ensure!(
            host.child.try_wait()?.is_none(),
            "Application host exited during startup; inspect host.log"
        );
        if let Ok(response) = client
            .get(format!("{}/catalog", env["ROSTFREI_TRACER_URL"]))
            .bearer_auth(&env["ROSTFREI_API_TOKEN"])
            .send()
            && response.status().is_success()
            && let Ok(body) = response.text()
            && let Ok(catalog) = serde_json::from_str(&body)
        {
            return Ok(catalog);
        }
        ensure!(
            Instant::now() < deadline,
            "Application startup exceeded {timeout:?}"
        );
        thread::sleep(Duration::from_millis(100));
    }
}
pub fn qualify(options: Options) -> Result<()> {
    ensure!(
        std::env::var("ROSTFREI_NATS_URL").is_ok_and(|s| !s.is_empty()),
        "Run through cargo xtask test with a disposable NATS broker"
    );
    let output = crate::absolute(&options.output)?;
    ensure!(
        !output.starts_with(root()),
        "Evidence must be outside the immutable source checkout"
    );
    ensure!(
        !output.exists() || fs::read_dir(&output)?.next().is_none(),
        "Preserving existing evidence: choose a new or empty directory"
    );
    let revision = candidate::clean_revision(&root())?;
    let version = versions::check(&root(), None)?;
    fs::create_dir_all(&output)?;
    let record = Recorder { output: &output };
    let temporary = tempfile::Builder::new()
        .prefix("rostfrei-skills-live-")
        .tempdir()?;
    let consumer = temporary.path();
    let native = consumer.join("cargo-rostfrei");
    let no_tools = consumer.join("no-tools");
    fs::create_dir(&no_tools)?;
    let mut host_source = options
        .external_app
        .as_ref()
        .map(|p| crate::absolute(p))
        .transpose()?
        .unwrap_or_else(root);
    let skill;
    if options.source_checkout {
        host_source = consumer.join("checkout");
        candidate::immutable_checkout(&root(), &host_source)?;
        skill = host_source.join(".agents/skills/rostfrei-develop-feature");
        ensure!(
            !skill.join("references/release.json").exists(),
            "Source regression must start without generated resources"
        );
        let target = root().join("target/skills-source");
        record.run(
            &process::args(&[
                "cargo",
                "build",
                "--locked",
                "--offline",
                "--manifest-path",
                &host_source
                    .join("crates/rostfrei-structure/Cargo.toml")
                    .display()
                    .to_string(),
                "--bin",
                "cargo-rostfrei",
                "--target-dir",
                &target.display().to_string(),
            ]),
            &host_source,
            &environment(),
            0,
        )?;
        fs::copy(target.join("debug/cargo-rostfrei"), &native)?;
    } else if options.external_app.is_some() {
        skill = host_source.join(".agents/skills/rostfrei-develop-feature");
        fs::copy(cli(), &native)?;
        candidate::verify_release(
            &read_json(&skill.join("references/release.json"))?,
            &revision,
            &version,
        )?;
    } else {
        let artifacts = consumer.join("artifacts");
        let archive = skills::package(&artifacts, true)?;
        let kit = skills::extract(&archive, &consumer.join("extracted"))?;
        let destination = consumer.join("application/.agents/skills");
        fs::copy(cli(), &native)?;
        let mut env = environment();
        env.insert("PATH".into(), no_tools.display().to_string());
        record.run(
            &process::args(&[
                &native.display().to_string(),
                "rostfrei",
                "skills",
                "install",
                "--kit",
                &kit.display().to_string(),
                "--destination",
                &destination.display().to_string(),
            ]),
            consumer,
            &env,
            0,
        )?;
        skill = destination.join("rostfrei-develop-feature");
        fs::remove_dir_all(kit)?;
        fs::remove_dir_all(artifacts)?;
    }
    let cli_release: Value = serde_json::from_str(&record.run(
        &process::args(&[&native.display().to_string(), "rostfrei", "skills", "info"]),
        consumer,
        &environment(),
        0,
    )?)?;
    candidate::verify_release(&cli_release, &revision, &version)?;
    let framework_root = root();
    let metadata = record.metadata(
        "host",
        if options.external_app.is_some() {
            &framework_root
        } else {
            &host_source
        },
        &revision,
        &version,
        &host_source,
        &environment(),
    )?;
    let mut provenance = if !options.host_command.is_empty() {
        let provenance = read_json(
            options
                .host_build_provenance
                .as_deref()
                .context("explicit host command requires build provenance")?,
        )?;
        let executable = crate::absolute(Path::new(&options.host_command[0]))?;
        ensure!(
            provenance["sourceRevision"] == revision
                && provenance["sourceTreeDirty"] == false
                && executable == crate::absolute(Path::new(text(&provenance["executable"])?))?
                && digest(&executable)? == text(&provenance["executableSha256"])?
                && digest(&host_source.join("Cargo.lock"))? == text(&provenance["lockSha256"])?,
            "Explicit host binary/build provenance differs from candidate"
        );
        provenance
    } else {
        json!({"sourceRevision":revision,"sourceTreeDirty":false,"source":host_source,"lockSha256":digest(&host_source.join("Cargo.lock"))?,"attestation":"local standard cargo run from recorded source; HTTP client does not attest Git SHA"})
    };
    let env = host_environment("skills")?;
    let mut client_env = env.clone();
    client_env.insert("PATH".into(), no_tools.display().to_string());
    let command = if options.host_command.is_empty() {
        process::args(&[
            "cargo",
            "run",
            "--locked",
            "-p",
            "bike-rental",
            "--bin",
            "bike-rental-api",
            "--",
            "--tracer",
        ])
    } else {
        options.host_command.clone()
    };
    let mut host = process::background(&command, &host_source, &env, &output.join("host.log"))?;
    let result = (|| {
        let catalog = ready(&mut host, &env, Duration::from_secs(300))?;
        let definition = skill.join("assets/add-first-bicycle.json");
        let original = fs::read(&definition)?;
        let original_json: Value = serde_json::from_slice(&original)?;
        let mut capabilities = Capabilities::new(&env, &output)?;
        capabilities.discover()?;
        capabilities.seed_normal(&original_json)?;
        let runner =
            |definition: &Path, name: &str, expected: i32, release: bool| -> Result<Value> {
                let evidence = output.join(name);
                let mut args = process::args(&[
                    &native.display().to_string(),
                    "rostfrei",
                    "test",
                    &definition.display().to_string(),
                    "--output",
                    &evidence.display().to_string(),
                ]);
                if release && !options.source_checkout {
                    args.extend(process::args(&[
                        "--release",
                        &skill.join("references/release.json").display().to_string(),
                    ]));
                }
                record.run(&args, consumer, &client_env, expected)?;
                read_json(&evidence)
            };
        let passed = runner(&definition, "example-passed.json", 0, false)?;
        let schema = if options.source_checkout {
            root().join("crates/rostfrei-tracer/schema/behavioral-test-v2.schema.json")
        } else {
            skill.join("assets/behavioral-test-v2.schema.json")
        };
        ensure!(
            passed["schema"] == read_json(&schema)?,
            "Packaged schema differs from live schema"
        );
        validate_capture(&passed, "passed")?;
        let observed = capabilities.observed_run(&passed, "run")?;
        capabilities.normal_preserved("first-suite")?;
        let mut wrong = original_json.clone();
        wrong["expected"]["graphs"][0]["nodes"][1]["payload"]["bicycle_id"] =
            json!("negative-control-wrong-id");
        let negative = consumer.join("negative-control.json");
        save_json(&negative, &wrong)?;
        let failed = runner(&negative, "negative-control-failed.json", 1, true)?;
        validate_capture(&failed, "failed")?;
        capabilities.after_reset(&observed, "second-suite")?;
        capabilities.normal_preserved("second-suite")?;
        let second = capabilities.observed_run(&failed, "second-suite")?;
        ensure!(
            fs::read(&definition)? == original,
            "Passing definition changed"
        );
        validate_capture(
            &runner(&definition, "example-rerun-passed.json", 0, false)?,
            "passed",
        )?;
        capabilities.after_reset(&second, "third-suite")?;
        capabilities.normal_preserved("third-suite")?;
        capabilities.verify_normal_history()?;
        let simulation =
            capabilities.simulate_handler(&original_json, options.require_handler_simulation)?;
        ensure!(
            candidate::clean_revision(&root())? == revision,
            "Candidate changed during qualification"
        );
        if options.source_checkout {
            ensure!(
                candidate::clean_revision(&host_source)? == revision,
                "Source snapshot changed during qualification"
            );
        }
        if options.host_command.is_empty() {
            let executable =
                Path::new(text(&metadata["target_directory"])?).join("debug/bike-rental-api");
            provenance["executable"] = json!(executable);
            provenance["executableSha256"] = json!(digest(&executable)?);
            provenance["command"] = json!(command);
        }
        save_json(&output.join("host-build-provenance.json"), &provenance)?;
        let legacy = array(&catalog["contexts"])?.iter().any(|c| {
            c["commands"].as_array().is_some_and(|cs| {
                cs.iter().any(|c| {
                    c["versions"].as_array().is_some_and(|vs| {
                        vs.iter().any(|v| v.get("simulateHrefTemplate").is_some())
                    })
                })
            })
        });
        save_json(
            &output.join("summary.json"),
            &json!({"release":passed["release"],"catalogVersion":catalog["catalogVersion"],"application":env["ROSTFREI_APPLICATION"],
            "applicationCommand":command,"hostSourceRevision":revision,"hostBuildProvenance":"host-build-provenance.json","cliRelease":cli_release,"cliSha256":digest(&native)?,
            "completedOutcomeEpoch":env["ROSTFREI_COMPLETED_OUTCOME_EPOCH"],"normalPreservation":"observed history retained through three suite resets; new duplicate command rejected",
            "requiredHandlerSimulation":options.require_handler_simulation,"sourceCheckout":options.source_checkout,"fixture":{"id":passed["fixture"]["id"],"revision":passed["fixture"]["revision"]},
            "example":"passed","negativeComparisonControl":"failed-as-expected","exampleAfterReset":"passed","capture":passed["messageSeries"]["capture"],
            "freshSessionTrial":"unexecuted","humanFeedback":"not-collected","brokerCleanup":"owned by cargo xtask test caller","consumerPath":"empty tools directory; build tools unavailable",
            "extractedKitRemovedBeforeExecution":!options.source_checkout && options.external_app.is_none(),"externalApplication":options.external_app,
            "capabilities":["scoped catalogs","Test/Production observation lists and bounded initial SSE snapshots","Test observed event detail and explicit causation","Test observation expiry after reset","Production observation generation retained","scoped quarantine lists"],
            "simulationAdvertised":legacy,"handlerSimulation":simulation}),
        )?;
        Ok::<_, anyhow::Error>(())
    })();
    let stopped = host.stop(Duration::from_secs(30));
    result?;
    ensure!(stopped? == 0, "host shutdown failed");
    println!("Live qualification passed; evidence: {}", output.display());
    Ok(())
}
fn validate_capture(evidence: &Value, status: &str) -> Result<()> {
    ensure!(
        evidence["report"]["status"] == status
            && evidence["captureState"] == "complete"
            && evidence["messageSeries"]["capture"]["settled"] == true
            && evidence["messageSeries"]["capture"]["fidelity"] == "exact",
        "Expected a complete, settled, exact real-NATS {status} capture"
    );
    Ok(())
}
