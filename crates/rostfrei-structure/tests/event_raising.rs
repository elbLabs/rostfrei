use std::{fs, io, path::Path, path::PathBuf, process::Command};

use rostfrei_structure::{Diagnostic, DiagnosticCode, check_domain_root};

const ACTION: &str = "bike_rental/rental_fleet/rent_bicycle/execute.rs";
const HANDLER: &str = "bike_rental/rental_fleet/rent_bicycle/handler.rs";

struct Fixture {
    directory: tempfile::TempDir,
    domain: PathBuf,
}

impl Fixture {
    fn new() -> io::Result<Self> {
        let directory = tempfile::tempdir()?;
        let domain = directory.path().join("src/domain");
        copy_tree(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/valid_domain/src/domain"),
            &domain,
        )?;
        Ok(Self { directory, domain })
    }

    fn write(&self, path: &str, source: &str) -> io::Result<()> {
        fs::write(self.domain.join(path), source)
    }

    fn raising_diagnostics(&self) -> Vec<Diagnostic> {
        check_domain_root(&self.domain)
            .into_iter()
            .filter(|diagnostic| diagnostic.code == DiagnosticCode::EventRaisingOutsideAction)
            .collect()
    }
}

fn copy_tree(source: &Path, destination: &Path) -> io::Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn action_source(body: &str) -> String {
    format!(
        "impl RentBicycleContract for AggregateInstance<RentalFleetAggregate> {{
    fn rent_bicycle(&mut self, bicycle_id: BicycleId) -> Result<(), BicycleUnavailable> {{
        let event = BicycleRented {{ bicycle_id }};
        {body}
        Ok(())
    }}
}}"
    )
}

fn handler_source(body: &str) -> String {
    format!(
        "pub struct RentBicycleHandler;
#[async_trait]
impl CommandHandler<RentBicycle> for RentBicycleHandler {{
    type Rejection = BicycleUnavailable;
    async fn handle(&self, command: &RentBicycle, execution: &mut CommandExecution<'_>)
        -> CommandHandlingResult<Self::Rejection> {{
        let mut fleet = execution.load::<RentalFleetAggregate>(command.fleet_id.as_str()).await?;
        let event = BicycleRented {{ bicycle_id: command.bicycle_id.clone() }};
        {body}
        Ok(CommandDecision::Accepted)
    }}
}}"
    )
}

#[test]
fn validated_action_methods_allow_raising_and_local_closures() {
    let fixture = Fixture::new().unwrap();
    for body in [
        "self.raise(event);",
        "self.r#raise(event);",
        "AggregateInstance::<RentalFleetAggregate>::raise(self, event);",
        "rostfrei::AggregateInstance::raise(self, event);",
        "let emit = AggregateInstance::<RentalFleetAggregate>::raise; emit(self, event);",
        "let emit = || self.raise(event); emit();",
        "forward!({ self.raise(event) });",
    ] {
        fixture.write(ACTION, &action_source(body)).unwrap();
        let diagnostics = check_domain_root(&fixture.domain);
        assert!(diagnostics.is_empty(), "{body}: {diagnostics:#?}");
    }
}

#[test]
fn handlers_must_invoke_actions_instead_of_raising_events() {
    let fixture = Fixture::new().unwrap();
    for body in [
        "fleet.raise(event);",
        "fleet.r#raise(event);",
        "fleet.aggregate_mut().raise(event);",
        "AggregateInstance::<RentalFleetAggregate>::raise(&mut fleet, event);",
        "rostfrei::AggregateInstance::raise(&mut fleet, event);",
        "<AggregateInstance<RentalFleetAggregate>>::raise(&mut fleet, event);",
        "rostfrei::AggregateInstance::r#raise(&mut fleet, event);",
        "type Instance = AggregateInstance<RentalFleetAggregate>; Instance::raise(&mut fleet, event);",
        "use rostfrei::AggregateInstance as Instance; Instance::raise(&mut fleet, event);",
        "let emit = AggregateInstance::<RentalFleetAggregate>::raise; emit(&mut fleet, event);",
        "let emit = || fleet.raise(event); emit();",
        "forward!({ fleet.raise(event) });",
        "forward!({ fleet.r#raise(event) });",
        "forward!(fleet.raise::<BicycleRented>(event));",
        "forward!(fleet.raise::<Option<Result<BicycleRented, Error>>>(event));",
        "forward!(fleet.raise::<fn() -> BicycleRented>(event));",
        "forward!(fleet.raise::<fn() -> Option<BicycleRented>>(event));",
        "forward!(AggregateInstance::<RentalFleetAggregate>::raise(&mut fleet, event));",
        "macro_rules! emit { () => { fleet.raise(event) }; } emit!();",
        "macro_rules! emit { ($event:ty) => { fleet.raise::<$event>(event) }; } emit!(BicycleRented);",
    ] {
        fixture.write(HANDLER, &handler_source(body)).unwrap();
        let diagnostics = check_domain_root(&fixture.domain);
        assert_eq!(diagnostics.len(), 1, "{body}: {diagnostics:#?}");
        let diagnostic = &diagnostics[0];
        assert_eq!(diagnostic.code, DiagnosticCode::EventRaisingOutsideAction);
        assert_eq!(diagnostic.path, fixture.domain.join(HANDLER));
        assert_eq!(diagnostic.line, 9);
        assert!(diagnostic.message.contains("domain action implementations"));
    }
}

#[test]
fn query_execute_files_do_not_authorize_event_raising() {
    let fixture = Fixture::new().unwrap();
    fixture
        .write(
            "bike_rental/rental_fleet/bicycle_availability/execute.rs",
            "impl BicycleAvailabilityQuery for RentalFleet {
    fn bicycle_availability(&self, _input: BicycleId) -> BicycleAvailability {
        let mut instance = AggregateInstance::<RentalFleetAggregate>::new(stream_id());
        instance.raise(event());
        BicycleAvailability::Available
    }
}",
        )
        .unwrap();
    let diagnostics = check_domain_root(&fixture.domain);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    assert_eq!(
        diagnostics[0].code,
        DiagnosticCode::EventRaisingOutsideAction
    );
    assert_eq!(diagnostics[0].line, 4);
}

#[test]
fn raising_requires_the_declared_action_and_its_validated_owner() {
    let fixture = Fixture::new().unwrap();
    let action = action_source("self.raise(event);");
    for source in [
        action.replace("RentBicycleContract", "OtherAction"),
        action.replace("RentalFleetAggregate", "OtherAggregate"),
        action.replace(
            "impl RentBicycleContract",
            "impl super::RentBicycleContract",
        ),
        action.replace("AggregateInstance<RentalFleetAggregate>", "Alias"),
        format!(
            "use super::RentBicycleContract as Alias; {}",
            action.replace("impl RentBicycleContract", "impl Alias")
        ),
        format!("use super::*; {action}"),
        format!("{action}\n{action}"),
    ] {
        fixture.write(ACTION, &source).unwrap();
        let diagnostics = fixture.raising_diagnostics();
        assert!(
            !diagnostics.is_empty(),
            "raising unexpectedly authorized: {source}"
        );
    }
}

#[test]
fn an_unannotated_trait_does_not_authorize_event_raising() {
    let fixture = Fixture::new().unwrap();
    fixture.write(
        "bike_rental/rental_fleet/rent_bicycle/action.rs",
        "pub trait RentBicycleContract { fn rent_bicycle(&mut self, bicycle_id: BicycleId) -> Result<(), BicycleUnavailable>; }",
    ).unwrap();
    let diagnostics = fixture.raising_diagnostics();
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    assert_eq!(diagnostics[0].path, fixture.domain.join(ACTION));
}

#[test]
fn helpers_nested_items_and_other_impls_do_not_inherit_action_permission() {
    let fixture = Fixture::new().unwrap();
    for source in [
        format!(
            "{}\nfn helper(instance: &mut AggregateInstance<RentalFleetAggregate>) {{ instance.raise(event()); }}",
            action_source("self.raise(event);")
        ),
        action_source(
            "self.raise(event); fn helper(instance: &mut AggregateInstance<RentalFleetAggregate>) { instance.raise(event()); }",
        ),
        action_source(
            "self.raise(event); impl OtherAction for Other { fn execute(&mut self) { instance().raise(event()); } }",
        ),
        action_source(
            "self.raise(event); macro_rules! emit { ($instance:expr) => { $instance.raise(event()) }; }",
        ),
        format!(
            "{} impl OtherAction for Other {{ fn execute(&mut self) {{ instance().raise(event()); }} }}",
            action_source("self.raise(event);").replace('\n', " ")
        ),
    ] {
        fixture.write(ACTION, &source).unwrap();
        let diagnostics = fixture.raising_diagnostics();
        assert_eq!(diagnostics.len(), 1, "{source}: {diagnostics:#?}");
    }
}

#[test]
fn policies_apply_and_initialize_methods_cannot_raise_events() {
    let fixture = Fixture::new().unwrap();
    for (path, source) in [
        (
            "bike_rental/rental_fleet/rental_assessment/evaluate.rs",
            "impl RentalAssessment for RentalFleetAggregate { fn assess(&self) { instance().raise(event()); } }",
        ),
        (
            "bike_rental/rental_fleet/rent_bicycle/apply.rs",
            "impl Apply<BicycleRented> for RentalFleet { fn apply(&mut self, event: &BicycleRented) { instance().raise(event); } }",
        ),
        (
            "bike_rental/rental_fleet/initialize.rs",
            "impl Initialize<RentalFleetAggregate> for RentalFleet { fn initialize(stream: &StreamId) -> Self { instance().raise(event()); root() } }",
        ),
    ] {
        let file = fixture.domain.join(path);
        let original = fs::read_to_string(&file).ok();
        fixture.write(path, source).unwrap();
        let diagnostics = fixture.raising_diagnostics();
        assert_eq!(diagnostics.len(), 1, "{path}: {diagnostics:#?}");
        assert_eq!(diagnostics[0].path, file);
        if let Some(original) = original {
            fs::write(file, original).unwrap();
        } else {
            fs::remove_file(file).unwrap();
        }
    }
}

#[test]
fn ordinary_action_calls_and_strings_mentioning_raise_are_allowed() {
    let fixture = Fixture::new().unwrap();
    fixture
        .write(
            HANDLER,
            &handler_source(
                r#"let _message = "fleet.raise(event)";
        assert_eq!("AggregateInstance::raise", "AggregateInstance::raise");
        match fleet.rent_bicycle(command.bicycle_id.clone()) {
            Ok(()) => {},
            Err(rejection) => return Ok(CommandDecision::Rejected(rejection)),
        }"#,
            ),
        )
        .unwrap();
    let diagnostics = check_domain_root(&fixture.domain);
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
}

#[test]
fn attribute_macro_arguments_cannot_raise_events_outside_actions() {
    let fixture = Fixture::new().unwrap();
    for expression in [
        "instance.raise(event)",
        "instance.r#raise::<BicycleRented>(event)",
        "AggregateInstance::<RentalFleetAggregate>::raise(instance, event)",
    ] {
        let helper = format!(
            "#[tracing::instrument(skip_all, fields(emitted = {{ {expression}; true }}))]
fn helper(instance: &mut AggregateInstance<RentalFleetAggregate>, event: BicycleRented) {{}}"
        );
        let action = action_source("");
        for (path, source) in [
            (ACTION, format!("{action}\n{helper}")),
            (ACTION, action_source(&helper)),
            (HANDLER, handler_source(&helper)),
            (HANDLER, handler_source("").replace(
                "    async fn handle",
                &format!("    #[tracing::instrument(skip_all, fields(emitted = {{ {expression}; true }}))]\n    async fn handle"),
            )),
            (ACTION, format!("#[instrument_impl(emitted = {{ {expression}; true }})]\n{action}")),
        ] {
            fixture.write(path, &source).unwrap();
            let diagnostics = fixture.raising_diagnostics();
            assert_eq!(diagnostics.len(), 1, "{source}: {diagnostics:#?}");
            assert_eq!(diagnostics[0].path, fixture.domain.join(path));
            fixture.write(path, if path == ACTION { &action } else { "" }).unwrap();
        }
    }
}

#[test]
fn attributes_on_action_methods_retain_the_action_scope() {
    let fixture = Fixture::new().unwrap();
    fixture.write(ACTION, &action_source("").replace(
        "    fn rent_bicycle",
        "    #[tracing::instrument(skip_all, fields(emitted = { self.raise(BicycleRented { bicycle_id: bicycle_id.clone() }); true }))]\n    fn rent_bicycle",
    )).unwrap();
    let diagnostics = check_domain_root(&fixture.domain);
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
}

#[test]
fn fields_and_ranges_in_macro_inputs_are_not_event_raising() {
    let fixture = Fixture::new().unwrap();
    for expression in [
        "salary.raise",
        "salary.r#raise",
        "(salary.raise)(event)",
        "0..raise",
        "0..raise()",
        "0..raise::<u8>()",
        "0..=raise",
        "..raise",
    ] {
        for body in [
            format!("let _value = {expression};"),
            format!("assert_eq!({expression}, expected);"),
            format!("trace!(value = ?({expression}));"),
            format!(
                "#[tracing::instrument(skip_all, fields(value = ?({expression})))]\nfn helper() {{}}"
            ),
        ] {
            fixture.write(HANDLER, &handler_source(&body)).unwrap();
            let diagnostics = check_domain_root(&fixture.domain);
            assert!(diagnostics.is_empty(), "{body}: {diagnostics:#?}");
        }
    }
}

#[test]
fn mirrored_domain_tests_can_raise_events_for_fixtures() {
    let fixture = Fixture::new().unwrap();
    fixture
        .write(
            "tests/bike_rental/rental_fleet/rent_bicycle.rs",
            "#[test]\nfn fixture() { instance().raise(event()); }",
        )
        .unwrap();
    let diagnostics = check_domain_root(&fixture.domain);
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
}

#[test]
fn workspace_command_fails_with_rf011_for_handler_event_raising() {
    let fixture = Fixture::new().unwrap();
    fixture
        .write(HANDLER, &handler_source("fleet.raise(event);"))
        .unwrap();
    let root = fixture.directory.path();
    fs::write(
        root.join("Cargo.toml"),
        r#"[workspace]
[package]
name = "event-raising-fixture"
version = "0.0.0"
edition = "2024"
[package.metadata.rostfrei.structure]
version = 1
domain-root = "src/domain"
"#,
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "").unwrap();
    fs::create_dir_all(root.join("src/bin")).unwrap();
    fs::write(
        root.join("src/bin/rostfrei-domain-check.rs"),
        "fn main() {}",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-rostfrei"))
        .args(["rostfrei", "check", "--workspace", "--manifest-path"])
        .arg(root.join("Cargo.toml"))
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(stderr.contains("RF011"), "{stderr}");
    assert!(stderr.contains("handler.rs:9"), "{stderr}");
    assert!(
        stderr.contains("invoke the aggregate's domain action"),
        "{stderr}"
    );
    assert!(stderr.contains("failed with 1 diagnostic(s)"), "{stderr}");
}
