use std::{
    fs::{File, OpenOptions},
    io::{Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use serde::Serialize;
use serde_json::Value;

use super::{ErrorKind, Phase, Release, Result, ToolError, field, http::Tracer, json_file};

pub struct TestOptions {
    pub definition: PathBuf,
    pub output: PathBuf,
    pub release: Option<PathBuf>,
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
enum RunState {
    NotSubmitted,
    SubmissionOutcomeUnknown,
    ResponseReceived,
    Reported,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Evidence {
    release: Release,
    cli_release: Release,
    validation: Value,
    fixture: Value,
    catalog_version: u32,
    schema: Value,
    definition: Value,
    run_state: RunState,
    #[serde(skip_serializing_if = "Option::is_none")]
    report: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    capture_state: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message_series: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<ToolError>,
}

fn setup(message: &str) -> ToolError {
    ToolError::new(ErrorKind::SetupFailed, Phase::Preparation, message)
}

fn selected_release(options: &TestOptions) -> Result<Release> {
    // Installed example assets have adjacent release metadata. Source skills do
    // not need generated files: the compiled CLI identifies its own release.
    let adjacent = options
        .definition
        .parent()
        .and_then(Path::parent)
        .map(|path| path.join("references/release.json"))
        .filter(|path| path.exists());
    let Some(path) = options.release.as_ref().or(adjacent.as_ref()) else {
        return Ok(Release::current());
    };
    let release: Release = serde_json::from_value(json_file(path, Phase::Preparation)?)
        .map_err(|_| setup("Invalid skill release metadata"))?;
    Ok(release)
}

fn discover(tracer: &Tracer, release: Release, definition: Value) -> Result<(Evidence, String)> {
    let catalog = tracer.request("/catalog", None, Phase::Catalog)?;
    if catalog.get("catalogVersion").and_then(Value::as_u64)
        != Some(u64::from(release.contracts.catalog))
    {
        return Err(ToolError::new(
            ErrorKind::InvalidResponse,
            Phase::Catalog,
            "Catalog version differs from this CLI/kit",
        ));
    }
    let schema = tracer.request(
        field(&catalog, "/behavioralTest/schemaHref", Phase::Schema)?,
        None,
        Phase::Schema,
    )?;
    let validation = tracer.request(
        field(&catalog, "/behavioralTest/validateHref", Phase::Validation)?,
        Some(&definition),
        Phase::Validation,
    )?;
    if validation.get("valid") != Some(&Value::Bool(true)) {
        return Err(ToolError::new(
            ErrorKind::InvalidResponse,
            Phase::Validation,
            "Definition did not pass runtime validation",
        ));
    }
    let fixture_id = definition
        .pointer("/setup/fixture")
        .and_then(Value::as_str)
        .ok_or_else(|| setup("Definition has no fixture ID"))?;
    let fixtures = tracer.request(
        field(&catalog, "/testScenario/fixturesHref", Phase::Fixture)?,
        None,
        Phase::Fixture,
    )?;
    let fixture = fixtures
        .get("items")
        .and_then(Value::as_array)
        .and_then(|items| {
            items
                .iter()
                .find(|item| item.get("id").and_then(Value::as_str) == Some(fixture_id))
        })
        .ok_or_else(|| {
            ToolError::new(
                ErrorKind::InvalidResponse,
                Phase::Fixture,
                "Registered fixture was not found",
            )
        })?;
    let fixture = tracer.request(
        field(fixture, "/fixtureHref", Phase::Fixture)?,
        None,
        Phase::Fixture,
    )?;
    let run_href = field(&catalog, "/behavioralTest/runHref", Phase::Catalog)?.to_owned();
    let evidence = Evidence {
        catalog_version: release.contracts.catalog,
        release,
        cli_release: Release::current(),
        validation,
        fixture,
        schema,
        definition,
        run_state: RunState::NotSubmitted,
        report: None,
        capture_state: None,
        message_series: None,
        error: None,
    };
    Ok((evidence, run_href))
}

fn persist(file: &mut File, evidence: &Evidence) -> Result<()> {
    let failure = || {
        ToolError::new(
            ErrorKind::SetupFailed,
            Phase::EvidenceFile,
            "Could not retain execution evidence",
        )
    };
    let bytes = serde_json::to_vec_pretty(evidence).map_err(|_| failure())?;
    file.seek(SeekFrom::Start(0))
        .and_then(|_| file.write_all(&bytes))
        .and_then(|()| file.set_len(u64::try_from(bytes.len()).map_err(std::io::Error::other)?))
        .and_then(|()| file.flush())
        .map_err(|_| failure())
}

fn execute(
    tracer: &Tracer,
    run_href: &str,
    evidence: &mut Evidence,
    file: &mut File,
) -> Result<bool> {
    persist(file, evidence)?;
    evidence.run_state = RunState::SubmissionOutcomeUnknown;
    persist(file, evidence)?;
    println!(
        "Running isolated Test definition; resets the host's entire configured Test scenario."
    );
    let report = tracer.request(run_href, Some(&evidence.definition), Phase::Run)?;
    let passed = match report.get("status").and_then(Value::as_str) {
        Some("passed") => Some(true),
        Some("failed") => Some(false),
        _ => None,
    };
    evidence.report = Some(report);
    evidence.run_state = RunState::ResponseReceived;
    let passed = passed.ok_or_else(|| {
        ToolError::new(
            ErrorKind::InvalidResponse,
            Phase::Run,
            "Run response has no supported behavioral verdict",
        )
    })?;
    evidence.run_state = RunState::Reported;
    evidence.capture_state = Some("incomplete");
    persist(file, evidence)?;
    let href = evidence
        .report
        .as_ref()
        .and_then(|report| report.pointer("/operation/messageSeriesHref"))
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ToolError::new(
                ErrorKind::InvalidResponse,
                Phase::Capture,
                "Report has no valid message-series link",
            )
        })?;
    let separator = if href.contains('?') { '&' } else { '?' };
    let series = tracer.request(
        &format!("{href}{separator}within=10s&settleFor=500ms"),
        None,
        Phase::Capture,
    )?;
    evidence.message_series = Some(series);
    evidence.capture_state = Some("complete");
    persist(file, evidence)?;
    Ok(passed)
}

pub fn run_test(options: &TestOptions) -> Result<u8> {
    let definition = json_file(&options.definition, Phase::Preparation)?;
    if definition.get("schemaVersion").and_then(Value::as_u64) != Some(2) {
        return Err(setup("Expected a canonical JSON v2 behavior definition"));
    }
    let release = selected_release(options)?;
    if release.check(Phase::Preparation).is_err() {
        return super::setup::delegate_test(&release);
    }
    let tracer = Tracer::from_env()?;
    let (mut evidence, href) = discover(&tracer, release, definition)?;
    let mut opening = OpenOptions::new();
    opening.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        opening.mode(0o600);
    }
    let mut file = opening.open(&options.output).map_err(|_| {
        ToolError::new(
            ErrorKind::SetupFailed,
            Phase::EvidenceFile,
            "Could not create new evidence file; existing evidence is preserved",
        )
    })?;
    let result = execute(&tracer, &href, &mut evidence, &mut file);
    match result {
        Ok(passed) => {
            println!(
                "Behavioral status: {}; evidence: {}",
                if passed { "passed" } else { "failed" },
                options.output.display()
            );
            Ok(u8::from(!passed))
        }
        Err(mut error) => {
            if matches!(evidence.run_state, RunState::Reported)
                && let Some(verdict) = evidence
                    .report
                    .as_ref()
                    .and_then(|report| report.get("status"))
            {
                error = error.detail("reportedVerdict", verdict.clone());
            }
            evidence.error = Some(error.clone());
            if persist(&mut file, &evidence).is_err() {
                error = error.detail("evidenceWriteFailed", true);
            }
            Err(error)
        }
    }
}
