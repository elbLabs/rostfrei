use rostfrei_core::{Aggregate, AggregateInstance, CommandHandler, StreamId};
use rostfrei_registry::{
    CommandDefinition, CommandDescriptor, DomainModule, DomainRegistry, ModuleDescriptor,
    QueryDefinition, QueryDescriptor, RegistrationError,
};

fn command(
    command_name: &'static str,
    schema_version: u32,
    aggregate_type: &'static str,
) -> CommandDescriptor {
    CommandDescriptor {
        command_name,
        schema_version,
        aggregate_type: aggregate_type.to_owned(),
        rust_command_type: "test::Command",
        rust_aggregate_type: "test::Aggregate",
        modeled_command: None,
    }
}

const fn query(
    bounded_context: &'static str,
    query_name: &'static str,
    schema_version: u32,
) -> QueryDescriptor {
    QueryDescriptor {
        bounded_context,
        query_name,
        schema_version,
        rust_request_type: "test::Query",
        rust_response_type: "test::Response",
        modeled_query: None,
    }
}

struct Banking;

impl DomainModule for Banking {
    const MODULE_NAME: &'static str = "banking";

    fn descriptor() -> ModuleDescriptor {
        ModuleDescriptor {
            module_name: Self::MODULE_NAME,
            commands: vec![
                command("withdraw-money", 1, "bank-account"),
                command("deposit-money", 1, "bank-account"),
            ],
            queries: Vec::new(),
        }
    }
}

struct Lending;

impl DomainModule for Lending {
    const MODULE_NAME: &'static str = "lending";

    fn descriptor() -> ModuleDescriptor {
        ModuleDescriptor {
            module_name: Self::MODULE_NAME,
            commands: vec![command("open-loan", 2, "loan")],
            queries: Vec::new(),
        }
    }
}

#[test]
fn registering_one_module_exposes_its_metadata() {
    let mut registry = DomainRegistry::new();

    registry.register_module::<Banking>().unwrap();

    assert_eq!(registry.modules().count(), 1);
    assert_eq!(registry.commands().count(), 2);
    assert_eq!(registry.aggregates().collect::<Vec<_>>(), ["bank-account"]);
    assert_eq!(
        registry.command("bank-account", "deposit-money", 1),
        Some(&command("deposit-money", 1, "bank-account"))
    );
    assert_eq!(registry.module("banking"), Some(&Banking::descriptor()));
}

#[test]
fn iteration_is_deterministic() {
    let mut registry = DomainRegistry::new();
    registry.register_module::<Lending>().unwrap();
    registry.register_module::<Banking>().unwrap();

    let modules = registry
        .modules()
        .map(|module| module.module_name)
        .collect::<Vec<_>>();
    let commands = registry
        .commands()
        .map(|command| (command.command_name, command.schema_version))
        .collect::<Vec<_>>();
    let aggregates = registry.aggregates().collect::<Vec<_>>();

    assert_eq!(modules, ["banking", "lending"]);
    assert_eq!(
        commands,
        [
            ("deposit-money", 1),
            ("open-loan", 2),
            ("withdraw-money", 1)
        ]
    );
    assert_eq!(aggregates, ["bank-account", "loan"]);
}

#[test]
fn commands_can_be_queried_by_aggregate_type() {
    let mut registry = DomainRegistry::new();
    registry.register_module::<Lending>().unwrap();
    registry.register_module::<Banking>().unwrap();

    let commands = registry
        .commands_for_aggregate("bank-account")
        .map(|command| command.command_name)
        .collect::<Vec<_>>();

    assert_eq!(commands, ["deposit-money", "withdraw-money"]);
    assert_eq!(registry.commands_for_aggregate("unknown").count(), 0);
}

struct DuplicateBanking;

impl DomainModule for DuplicateBanking {
    const MODULE_NAME: &'static str = "banking";

    fn descriptor() -> ModuleDescriptor {
        ModuleDescriptor {
            module_name: Self::MODULE_NAME,
            commands: vec![command("another-command", 1, "bank-account")],
            queries: Vec::new(),
        }
    }
}

#[test]
fn duplicate_module_registration_fails() {
    let mut registry = DomainRegistry::new();
    registry.register_module::<Banking>().unwrap();

    let error = registry.register_module::<DuplicateBanking>().unwrap_err();

    assert_eq!(
        error,
        RegistrationError::DuplicateModuleName {
            module_name: "banking"
        }
    );
    assert_eq!(registry.modules().count(), 1);
    assert_eq!(registry.commands().count(), 2);
}

struct InternallyDuplicated;

impl DomainModule for InternallyDuplicated {
    const MODULE_NAME: &'static str = "duplicated";

    fn descriptor() -> ModuleDescriptor {
        ModuleDescriptor {
            module_name: Self::MODULE_NAME,
            commands: vec![
                command("unique-command", 1, "account"),
                command("same-command", 1, "account"),
                command("same-command", 1, "account"),
            ],
            queries: Vec::new(),
        }
    }
}

#[test]
fn duplicate_command_inside_a_module_fails_atomically() {
    let mut registry = DomainRegistry::new();

    let error = registry
        .register_module::<InternallyDuplicated>()
        .unwrap_err();

    assert_eq!(
        error,
        RegistrationError::DuplicateCommandIdentityInModule {
            module_name: "duplicated",
            command_name: "same-command",
            schema_version: 1,
        }
    );
    assert_registry_is_empty(&registry);
}

struct OverlappingModule;

impl DomainModule for OverlappingModule {
    const MODULE_NAME: &'static str = "overlapping";

    fn descriptor() -> ModuleDescriptor {
        ModuleDescriptor {
            module_name: Self::MODULE_NAME,
            commands: vec![
                command("new-command", 1, "new-aggregate"),
                command("deposit-money", 1, "bank-account"),
            ],
            queries: Vec::new(),
        }
    }
}

struct SameLocalCommandForAnotherAggregate;

impl DomainModule for SameLocalCommandForAnotherAggregate {
    const MODULE_NAME: &'static str = "another-context";

    fn descriptor() -> ModuleDescriptor {
        ModuleDescriptor {
            module_name: Self::MODULE_NAME,
            commands: vec![command("deposit-money", 1, "another-aggregate")],
            queries: Vec::new(),
        }
    }
}

#[test]
fn command_identity_is_scoped_to_the_aggregate() {
    let mut registry = DomainRegistry::new();
    registry.register_module::<Banking>().unwrap();
    registry
        .register_module::<SameLocalCommandForAnotherAggregate>()
        .unwrap();

    assert_eq!(
        registry.command("bank-account", "deposit-money", 1),
        Some(&command("deposit-money", 1, "bank-account"))
    );
    assert_eq!(
        registry.command("another-aggregate", "deposit-money", 1),
        Some(&command("deposit-money", 1, "another-aggregate"))
    );
    assert_ne!(
        command("deposit-money", 1, "bank-account").identity(),
        command("deposit-money", 1, "another-aggregate").identity()
    );
}

#[test]
fn duplicate_command_across_modules_fails_atomically() {
    let mut registry = DomainRegistry::new();
    registry.register_module::<Banking>().unwrap();

    let error = registry.register_module::<OverlappingModule>().unwrap_err();

    assert_eq!(
        error,
        RegistrationError::DuplicateCommandIdentityAcrossModules {
            command_name: "deposit-money",
            schema_version: 1,
            existing_module_name: "banking",
            attempted_module_name: "overlapping",
        }
    );
    assert!(registry.module("overlapping").is_none());
    assert!(
        registry
            .command("new-aggregate", "new-command", 1)
            .is_none()
    );
    assert_eq!(registry.modules().count(), 1);
    assert_eq!(registry.commands().count(), 2);
    assert_eq!(registry.aggregates().collect::<Vec<_>>(), ["bank-account"]);
}

struct EmptyModuleName;

impl DomainModule for EmptyModuleName {
    const MODULE_NAME: &'static str = "";

    fn descriptor() -> ModuleDescriptor {
        ModuleDescriptor {
            module_name: Self::MODULE_NAME,
            commands: vec![command("valid-command", 1, "account")],
            queries: Vec::new(),
        }
    }
}

struct EmptyCommandName;

impl DomainModule for EmptyCommandName {
    const MODULE_NAME: &'static str = "empty-command";

    fn descriptor() -> ModuleDescriptor {
        ModuleDescriptor {
            module_name: Self::MODULE_NAME,
            commands: vec![
                command("valid-command", 1, "account"),
                command("", 1, "account"),
            ],
            queries: Vec::new(),
        }
    }
}

struct ZeroSchemaVersion;

impl DomainModule for ZeroSchemaVersion {
    const MODULE_NAME: &'static str = "zero-version";

    fn descriptor() -> ModuleDescriptor {
        ModuleDescriptor {
            module_name: Self::MODULE_NAME,
            commands: vec![
                command("valid-command", 1, "account"),
                command("invalid-command", 0, "account"),
            ],
            queries: Vec::new(),
        }
    }
}

struct EmptyAggregateType;

impl DomainModule for EmptyAggregateType {
    const MODULE_NAME: &'static str = "empty-aggregate";

    fn descriptor() -> ModuleDescriptor {
        ModuleDescriptor {
            module_name: Self::MODULE_NAME,
            commands: vec![
                command("valid-command", 1, "account"),
                command("invalid-command", 1, ""),
            ],
            queries: Vec::new(),
        }
    }
}

#[test]
fn invalid_descriptors_fail_without_partial_mutation() {
    assert_invalid::<EmptyModuleName>(RegistrationError::EmptyModuleName);
    assert_invalid::<EmptyCommandName>(RegistrationError::EmptyCommandName {
        module_name: "empty-command",
        rust_command_type: "test::Command",
    });
    assert_invalid::<ZeroSchemaVersion>(RegistrationError::ZeroSchemaVersion {
        module_name: "zero-version",
        command_name: "invalid-command",
    });
    assert_invalid::<EmptyAggregateType>(RegistrationError::EmptyAggregateType {
        module_name: "empty-aggregate",
        command_name: "invalid-command",
        schema_version: 1,
    });
}

fn assert_invalid<M: DomainModule>(expected: RegistrationError) {
    let mut registry = DomainRegistry::new();
    assert_eq!(registry.register_module::<M>(), Err(expected));
    assert_registry_is_empty(&registry);
}

fn assert_registry_is_empty(registry: &DomainRegistry) {
    assert_eq!(registry.modules().count(), 0);
    assert_eq!(registry.commands().count(), 0);
    assert_eq!(registry.queries().count(), 0);
    assert_eq!(registry.aggregates().count(), 0);
}

struct DirectAggregate;

impl Aggregate for DirectAggregate {
    type State = ();
    type Event = ();

    const AGGREGATE_TYPE: &'static str = "direct-aggregate";

    fn initial(_stream_id: &StreamId) -> Self::State {}

    fn apply(_state: &mut Self::State, _event: &Self::Event) {}
}

struct DirectCommand;

impl CommandHandler<DirectCommand> for DirectAggregate {
    type Rejection = ();

    fn handle(
        _command: &DirectCommand,
        _aggregate: &mut AggregateInstance<Self>,
    ) -> Result<(), Self::Rejection> {
        Ok(())
    }
}

impl CommandDefinition for DirectCommand {
    type Aggregate = DirectAggregate;

    const COMMAND_NAME: &'static str = "direct-command";
    const SCHEMA_VERSION: u32 = 1;
}

#[test]
fn commands_can_be_registered_without_a_module() {
    let mut registry = DomainRegistry::new();

    registry.register_command::<DirectCommand>().unwrap();

    assert_eq!(registry.modules().count(), 0);
    assert_eq!(
        registry.aggregates().collect::<Vec<_>>(),
        ["direct-aggregate"]
    );
    assert_eq!(
        registry.command("direct-aggregate", "direct-command", 1),
        Some(&DirectCommand::descriptor())
    );
}

struct FindAccount;

impl QueryDefinition for FindAccount {
    type Response = String;

    const BOUNDED_CONTEXT: &'static str = "banking";
    const QUERY_NAME: &'static str = "find-account";
    const SCHEMA_VERSION: u32 = 1;
}

struct InvalidRoutedQuery;

impl QueryDefinition for InvalidRoutedQuery {
    type Response = String;

    const BOUNDED_CONTEXT: &'static str = "Banking";
    const QUERY_NAME: &'static str = "find-account";
    const SCHEMA_VERSION: u32 = 1;
}

struct Reporting;

impl DomainModule for Reporting {
    const MODULE_NAME: &'static str = "reporting";

    fn descriptor() -> ModuleDescriptor {
        ModuleDescriptor {
            module_name: Self::MODULE_NAME,
            commands: Vec::new(),
            queries: vec![
                query("banking", "account-balance", 1),
                query("banking", "account-history", 2),
            ],
        }
    }
}

#[test]
fn queries_register_directly_and_through_modules() {
    let mut registry = DomainRegistry::new();
    registry.register_query::<FindAccount>().unwrap();
    registry.register_module::<Reporting>().unwrap();

    assert_eq!(registry.queries().count(), 3);
    assert_eq!(
        registry.query("banking", "find-account", 1),
        Some(&FindAccount::descriptor())
    );
    assert_eq!(
        registry
            .queries_for_context("banking")
            .map(|query| (query.query_name, query.schema_version))
            .collect::<Vec<_>>(),
        [
            ("account-balance", 1),
            ("account-history", 2),
            ("find-account", 1),
        ]
    );
}

#[test]
fn duplicate_query_registration_fails_without_partial_mutation() {
    let mut registry = DomainRegistry::new();
    registry.register_query::<FindAccount>().unwrap();

    let error = registry.register_query::<FindAccount>().unwrap_err();

    assert_eq!(
        error,
        RegistrationError::DuplicateQueryIdentityAcrossModules {
            query_name: "find-account",
            schema_version: 1,
            existing_module_name: "<direct query registration>",
            attempted_module_name: "<direct query registration>",
        }
    );
    assert_eq!(registry.queries().count(), 1);
}

#[test]
fn unroutable_query_definitions_fail_registration() {
    let mut registry = DomainRegistry::new();

    let error = registry.register_query::<InvalidRoutedQuery>().unwrap_err();

    assert_eq!(
        error,
        RegistrationError::InvalidQueryIdentity {
            module_name: "<direct query registration>",
            query_name: "find-account",
            schema_version: 1,
            reason: "address context has an invalid format".to_owned(),
        }
    );
    assert_registry_is_empty(&registry);
}
