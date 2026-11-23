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
    let command = process::args(&[
        &target().join("debug/bike-rental-api").display().to_string(),
        "--tracer",
    ]);
    check_default_broker_inventory(&command, temporary.path())?;
    let mut env = live::host_environment("host")?;
    env.insert("ROSTFREI_INSPECT_BROKER".into(), "true".into());
    let mut host =
        process::background(&command, &root(), &env, &temporary.path().join("host.log"))?;
    let result = (|| {
        let catalog = live::ready(&mut host, &env, Duration::from_secs(45))?;
        let mut client = Capabilities::new(&env, temporary.path())?;
        client.discover()?;
        let inspection = &client.catalogs["production"];
        let infrastructure_href = text(&inspection["infrastructure"]["snapshotHref"])
            .context("inspection catalog must advertise opted-in broker inventory")?;
        let domain_href = text(&inspection["domainInventory"]["snapshotHref"])
            .context("inspection catalog must advertise opted-in domain inventory")?;
        ensure!(
            infrastructure_href == "/inventory/infrastructure"
                && domain_href == "/inventory/domain",
            "unexpected inventory routes in the inspection catalog"
        );
        let before_dispatch = normal_inventory_counts(&client, infrastructure_href)?;
        let dispatch = |name: &str, key: &str, expected: &str| -> Result<()> {
            let command = array(&catalog["contexts"])?
                .iter()
                .flat_map(|c| c["commands"].as_array().into_iter().flatten())
                .find(|c| c["id"] == name)
                .context("bike command")?;
            let versions = array(&command["versions"])?;
            ensure!(versions.len() == 1, "command versions");
            client.normal_command(&json!({"context":"bike-rental","name":name,"schemaVersion":versions[0]["schemaVersion"],"payload":{"fleet_id":"city-fleet","bicycle_id":"normal-registration","condition":"serviceable"}}),key,expected)?;
            Ok(())
        };
        ensure!(
            !array(&catalog["contexts"])?
                .iter()
                .flat_map(|c| c["commands"].as_array().into_iter().flatten())
                .any(|c| c["id"] == "rent-bicycle"),
            "direct rental must not be advertised"
        );
        dispatch("register-bicycle", "normal-before-test-reset", "accepted")?;
        let before_inventory = normal_inventory_counts(&client, infrastructure_href)?;
        ensure!(
            before_inventory[0] > before_dispatch[0],
            "production journal sample did not reflect the committed registration"
        );
        check_domain_inventory(&client, domain_href)?;
        for (capability, href) in [
            ("infrastructure", infrastructure_href),
            ("domainInventory", domain_href),
        ] {
            ensure!(
                catalog.get(capability).is_none(),
                "Test catalog leaked {capability}"
            );
            for scope in ["test", "dispatch"] {
                client.request(href, scope, None, 403, None, None)?;
            }
        }
        ensure!(
            normal_inventory_counts(&client, infrastructure_href)? == before_inventory,
            "inventory reads changed production journal or command results"
        );
        let definitions =
            client.get(text(&catalog["behavioralTest"]["definitionsHref"])?, "test")?;
        ensure!(
            array(&definitions["items"])?.len() == 7,
            "expected seven host definitions"
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
            println!(
                "HTTP scenario passed: {} (command: {})",
                definition["id"], report["operation"]["command"]
            );
        }
        dispatch("register-bicycle", "normal-after-test-reset", "rejected")?;
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
            browser.insert(
                "VITE_TRACER_INSPECTION_TOKEN".into(),
                env["ROSTFREI_INSPECTION_TOKEN"].clone(),
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
        "Application-linked host suite, inventory inspection, Normal preservation and shutdown passed ({})",
        nonce()
    );
    Ok(())
}

fn check_default_broker_inventory(command: &[String], temporary: &Path) -> Result<()> {
    let mut env = live::host_environment("host-default")?;
    env.remove("ROSTFREI_INSPECT_BROKER");
    env.remove("ROSTFREI_DISPATCH_TOKEN");
    let log = temporary.join("host-default.log");
    let mut host = process::background(command, &root(), &env, &log)?;
    let result = (|| {
        let control = live::ready(&mut host, &env, Duration::from_secs(45))?;
        ensure!(
            !serde_json::to_string(&control)?.contains("\"dispatchHrefTemplate\""),
            "Dispatch was advertised without an installed credential"
        );
        let client = Capabilities::new(&env, temporary)?;
        let catalog = client.get("/catalog", "production")?;
        ensure!(
            catalog.get("infrastructure").is_none(),
            "bike-rental advertised broker inventory without explicit opt-in"
        );
        client.request(
            "/inventory/infrastructure",
            "production",
            None,
            501,
            None,
            None,
        )?;
        Ok::<_, anyhow::Error>(())
    })();
    let stopped = host.stop(Duration::from_secs(50));
    if result.is_err() {
        eprintln!("{}", fs::read_to_string(log).unwrap_or_default());
    }
    result?;
    ensure!(stopped? == 0, "default host shutdown failed");
    Ok(())
}

fn normal_inventory_counts(client: &Capabilities, href: &str) -> Result<[u64; 2]> {
    let snapshot = client.get(href, "production")?;
    // Use the unique application name from this host, not arbitrary streams in
    // the sampled broker account. Normal has no scope segment in its stream names.
    let prefix = format!(
        "{}__BIKE_RENTAL",
        client.application.replace('-', "_").to_ascii_uppercase()
    );
    let journal = format!("{prefix}__JOURNAL");
    let results = format!("{prefix}__COMMAND_RESULTS");
    for source in array(&snapshot["sources"])? {
        if source["connection"] != "connected" || source["streams"]["status"] != "complete" {
            continue;
        }
        let streams = array(&source["streams"]["items"])?;
        let find = |name: &str| streams.iter().find(|stream| stream["name"] == name);
        if let (Some(journal), Some(results)) = (find(&journal), find(&results)) {
            return Ok([
                journal["messages"]
                    .as_u64()
                    .context("production journal message count")?,
                results["messages"]
                    .as_u64()
                    .context("production command results message count")?,
            ]);
        }
    }
    anyhow::bail!("inventory did not sample {journal} and {results} from the connected broker")
}

fn check_domain_inventory(client: &Capabilities, href: &str) -> Result<()> {
    let snapshot = client.get(href, "production")?;
    let fleet_id = json!({"context":"bike-rental","local":"rental-fleet"});
    ensure!(
        snapshot["compiledModelAvailable"] == true
            && array(&snapshot["aggregates"])?
                .iter()
                .any(|aggregate| aggregate["id"] == fleet_id
                    && aggregate["aggregateType"] == "bike-rental/rental-fleet"
                    && aggregate["root"]["local"] == "rental-fleet-root")
            && array(&snapshot["entities"])?
                .iter()
                .any(|entity| entity["id"]["aggregate"] == fleet_id
                    && entity["id"]["local"] == "bicycle")
            && array(&snapshot["registeredCommands"])?
                .iter()
                .any(|command| command["context"] == "bike-rental"
                    && command["name"] == "register-bicycle"),
        "domain inventory missing the compiled fleet graph or its runtime registrations"
    );
    for (group, id, field, declared_type) in [
        ("entities", "bicycle", "category", "Option<BicycleCategory>"),
        (
            "domainEvents",
            "bicycle-collection-requested",
            "payment_id",
            "PaymentId",
        ),
        (
            "registeredCommands",
            "register-bicycle",
            "bicycle_id",
            "BicycleId",
        ),
    ] {
        let declaration = array(&snapshot[group])?
            .iter()
            .find(|entry| {
                (entry["id"]["aggregate"] == fleet_id && entry["id"]["local"] == id)
                    || (entry["context"] == "bike-rental"
                        && entry["name"] == id
                        && entry["schemaVersion"] == 2)
            })
            .with_context(|| format!("missing declaration for {id}"))?;
        ensure!(
            array(&declaration["fields"])?
                .iter()
                .any(|row| row["name"] == field && row["declaredType"] == declared_type),
            "missing declared type {declared_type} for {id}.{field}"
        );
    }
    Ok(())
}
