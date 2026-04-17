use rostfrei_core::{Aggregate, AggregateInstance, CommandHandler, StreamId};
use rostfrei_registry::{
    CommandDefinition, CommandDescriptor, DomainModule, DomainRegistry, ModuleDescriptor,
    RegistrationError,
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
        domain_command: None,
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
