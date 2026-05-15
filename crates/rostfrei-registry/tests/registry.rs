use async_trait::async_trait;
use domain::{
    BoundedContextDescriptor, BoundedContextId, BoundedContextType, Command,
    CommandDescriptor as ModeledCommandDescriptor,
};
use rostfrei_core::{
    Aggregate, CommandDecision, CommandExecution, CommandHandler, CommandHandlingResult, StreamId,
};
use rostfrei_registry::{CommandDefinition, DomainRegistry, RegistrationError};

struct FirstContext;
struct SecondContext;
struct EmptyContext;
struct InvalidContext;

macro_rules! bounded_context {
    ($context:ty, $id:literal) => {
        impl BoundedContextType for $context {
            const DESCRIPTOR: BoundedContextDescriptor = BoundedContextDescriptor {
                id: BoundedContextId($id),
                label: $id,
            };
        }
    };
}

bounded_context!(FirstContext, "first");
bounded_context!(SecondContext, "second");
bounded_context!(EmptyContext, "");
bounded_context!(InvalidContext, "InvalidContext");

struct FirstAggregate;
struct SecondAggregate;
struct DuplicateFirstAggregate;
struct EmptyAggregate;
struct UnqualifiedAggregate;
struct InvalidContextAggregate;
struct InvalidNameAggregate;
struct NestedAggregate;

macro_rules! aggregate {
    ($aggregate:ty, $aggregate_type:literal) => {
        impl Aggregate for $aggregate {
            type State = ();
            type Event = ();

            const BOUNDED_CONTEXT: &'static str = "registry-test";
            const AGGREGATE_TYPE: &'static str = $aggregate_type;

            fn initial(_stream_id: &StreamId) -> Self::State {}

            fn apply(_state: &mut Self::State, _event: &Self::Event) {}
        }
    };
}

aggregate!(FirstAggregate, "first/items");
aggregate!(SecondAggregate, "second/items");
aggregate!(DuplicateFirstAggregate, "first/items");
aggregate!(EmptyAggregate, "");
aggregate!(UnqualifiedAggregate, "items");
aggregate!(InvalidContextAggregate, "Invalid/items");
aggregate!(InvalidNameAggregate, "first/Items");
aggregate!(NestedAggregate, "first/group/items");

struct Open;
struct OpenSecond;
struct Rename;
struct EmptyName;
struct ZeroVersion;
struct EmptyContextCommand;
struct InvalidContextCommand;
struct InvalidRoutedName;
struct LongName;

macro_rules! command {
    ($command:ty, $context:ty, $id:literal, $version:literal) => {
        impl Command for $command {
            type Context = $context;

            const LOCAL_ID: &'static str = $id;
            const LABEL: &'static str = $id;
            const FIELDS: &'static [domain::FieldDescriptor] = &[];
            const SCHEMA_VERSION: u32 = $version;
        }
    };
}

command!(Open, FirstContext, "open", 1);
command!(OpenSecond, SecondContext, "open", 1);
command!(Rename, FirstContext, "rename", 2);
command!(EmptyName, FirstContext, "", 1);
command!(ZeroVersion, FirstContext, "zero", 0);
command!(EmptyContextCommand, EmptyContext, "open", 1);
command!(InvalidContextCommand, InvalidContext, "open", 1);
command!(InvalidRoutedName, FirstContext, "Open", 1);
command!(
    LongName,
    FirstContext,
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    1
);

struct OpenHandler;
struct AlternateOpenHandler;
struct SecondOpenHandler;
struct RenameHandler;
struct EmptyNameHandler;
struct ZeroVersionHandler;
struct EmptyContextHandler;
struct InvalidContextHandler;
struct InvalidRoutedNameHandler;
struct LongNameHandler;

macro_rules! handler {
    ($handler:ty, $command:ty, $rejection:ty) => {
        #[async_trait]
        impl CommandHandler<$command> for $handler {
            type Rejection = $rejection;

            async fn handle(
                &self,
                _command: &$command,
                _unit_of_work: &mut CommandExecution<'_>,
            ) -> CommandHandlingResult<Self::Rejection> {
                Ok(CommandDecision::Accepted)
            }
        }
    };
}

handler!(OpenHandler, Open, &'static str);
handler!(AlternateOpenHandler, Open, ());
handler!(SecondOpenHandler, OpenSecond, u8);
handler!(RenameHandler, Rename, ());
handler!(EmptyNameHandler, EmptyName, ());
handler!(ZeroVersionHandler, ZeroVersion, ());
handler!(EmptyContextHandler, EmptyContextCommand, ());
handler!(InvalidContextHandler, InvalidContextCommand, ());
handler!(InvalidRoutedNameHandler, InvalidRoutedName, ());
handler!(LongNameHandler, LongName, ());

#[test]
fn explicitly_registers_aggregate_inventory_in_stable_order() {
    let mut registry = DomainRegistry::new();
    registry.register_aggregate::<SecondAggregate>().unwrap();
    registry.register_aggregate::<FirstAggregate>().unwrap();

    assert_eq!(
        registry.aggregates().collect::<Vec<_>>(),
        ["first/items", "second/items"]
    );
}

#[test]
fn repeated_registration_of_the_same_aggregate_is_idempotent() {
    let mut registry = DomainRegistry::new();
    registry.register_aggregate::<FirstAggregate>().unwrap();
    registry.register_aggregate::<FirstAggregate>().unwrap();

    assert_eq!(registry.aggregates().collect::<Vec<_>>(), ["first/items"]);
}

#[test]
fn rejects_a_different_aggregate_with_the_same_stream_identity() {
    let mut registry = DomainRegistry::new();
    registry.register_aggregate::<FirstAggregate>().unwrap();

    assert_eq!(
        registry.register_aggregate::<DuplicateFirstAggregate>(),
        Err(RegistrationError::DuplicateAggregateType {
            aggregate_type: "first/items".to_owned(),
            existing_rust_aggregate_type: std::any::type_name::<FirstAggregate>(),
            attempted_rust_aggregate_type: std::any::type_name::<DuplicateFirstAggregate>(),
        })
    );
    assert_eq!(registry.aggregates().collect::<Vec<_>>(), ["first/items"]);
}

#[test]
fn validates_explicit_aggregate_routing_identity_without_mutation() {
    let mut registry = DomainRegistry::new();

    assert_eq!(
        registry.register_aggregate::<EmptyAggregate>(),
        Err(RegistrationError::EmptyAggregateType {
            rust_aggregate_type: std::any::type_name::<EmptyAggregate>(),
        })
    );
    assert_eq!(
        registry.register_aggregate::<UnqualifiedAggregate>(),
        Err(RegistrationError::InvalidAggregateType {
            aggregate_type: "items".to_owned(),
            rust_aggregate_type: std::any::type_name::<UnqualifiedAggregate>(),
            reason: "aggregate type must be bounded-context-qualified with exactly one context separator"
                .to_owned(),
        })
    );
    assert_eq!(
        registry.register_aggregate::<InvalidContextAggregate>(),
        Err(RegistrationError::InvalidAggregateType {
            aggregate_type: "Invalid/items".to_owned(),
            rust_aggregate_type: std::any::type_name::<InvalidContextAggregate>(),
            reason: "address context has an invalid format".to_owned(),
        })
    );
    assert_eq!(
        registry.register_aggregate::<InvalidNameAggregate>(),
        Err(RegistrationError::InvalidAggregateType {
            aggregate_type: "first/Items".to_owned(),
            rust_aggregate_type: std::any::type_name::<InvalidNameAggregate>(),
            reason: "address name has an invalid format".to_owned(),
        })
    );
    assert_eq!(
        registry.register_aggregate::<NestedAggregate>(),
        Err(RegistrationError::InvalidAggregateType {
            aggregate_type: "first/group/items".to_owned(),
            rust_aggregate_type: std::any::type_name::<NestedAggregate>(),
            reason: "aggregate type must be bounded-context-qualified with exactly one context separator"
                .to_owned(),
        })
    );
    assert_eq!(registry.aggregates().count(), 0);
}

#[test]
fn every_registered_aggregate_has_tracer_catalog_coordinates() {
    let mut registry = DomainRegistry::new();
    registry.register_aggregate::<FirstAggregate>().unwrap();
    registry.register_aggregate::<SecondAggregate>().unwrap();

    for aggregate_type in registry.aggregates() {
        let (context, aggregate) = aggregate_type
            .split_once('/')
            .expect("successful registration must be context-qualified");
        assert!(!context.is_empty());
        assert!(!aggregate.is_empty());
        assert!(!aggregate.contains('/'));
    }
}

#[test]
fn command_registration_does_not_infer_aggregate_inventory() {
    let mut registry = DomainRegistry::new();
    registry.register_command::<Open, OpenHandler>().unwrap();

    assert_eq!(registry.commands().count(), 1);
    assert_eq!(registry.aggregates().count(), 0);
}

#[test]
fn handler_registration_builds_contextual_runtime_and_modeled_metadata() {
    let mut registry = DomainRegistry::new();
    registry.register_command::<Open, OpenHandler>().unwrap();
    registry
        .register_command::<Rename, RenameHandler>()
        .unwrap();

    let open = registry.command("first", "open", 1).unwrap();
    assert_eq!(open.bounded_context, "first");
    assert_eq!(open.rust_command_type, std::any::type_name::<Open>());
    assert_eq!(open.rust_handler_type, std::any::type_name::<OpenHandler>());
    assert_eq!(
        open.modeled_command(),
        &ModeledCommandDescriptor {
            bounded_context: FirstContext::DESCRIPTOR,
            local_id: "open",
            label: "open",
            fields: &[],
            schema_version: 1,
        }
    );
    assert_eq!(
        registry
            .command("first", "rename", 2)
            .unwrap()
            .schema_version,
        2
    );
    assert_eq!(
        registry
            .commands_for_context("first")
            .map(|command| command.command_name)
            .collect::<Vec<_>>(),
        ["open", "rename"]
    );
}

#[test]
fn the_same_local_command_identity_can_exist_in_different_contexts() {
    let mut registry = DomainRegistry::new();
    registry.register_command::<Open, OpenHandler>().unwrap();
    registry
        .register_command::<OpenSecond, SecondOpenHandler>()
        .unwrap();

    assert!(registry.command("first", "open", 1).is_some());
    assert!(registry.command("second", "open", 1).is_some());
    assert_eq!(registry.commands().count(), 2);
}

#[test]
fn duplicate_context_command_identity_is_rejected_without_partial_mutation() {
    let mut registry = DomainRegistry::new();
    registry.register_command::<Open, OpenHandler>().unwrap();

    assert_eq!(
        registry.register_command::<Open, AlternateOpenHandler>(),
        Err(RegistrationError::DuplicateCommandIdentity {
            bounded_context: "first",
            command_name: "open",
            schema_version: 1,
        })
    );
    assert_eq!(registry.commands().count(), 1);
}

#[test]
fn validates_command_and_context_identity() {
    let mut registry = DomainRegistry::new();

    assert_eq!(
        registry.register_command::<EmptyName, EmptyNameHandler>(),
        Err(RegistrationError::EmptyCommandName {
            rust_command_type: std::any::type_name::<EmptyName>(),
        })
    );
    assert_eq!(
        registry.register_command::<ZeroVersion, ZeroVersionHandler>(),
        Err(RegistrationError::ZeroSchemaVersion {
            command_name: "zero",
        })
    );
    assert_eq!(
        registry.register_command::<EmptyContextCommand, EmptyContextHandler>(),
        Err(RegistrationError::EmptyCommandBoundedContext {
            command_name: "open",
            schema_version: 1,
        })
    );
    assert_eq!(registry.commands().count(), 0);
}

#[test]
fn rejects_unroutable_contexts_and_command_names_without_partial_mutation() {
    let mut registry = DomainRegistry::new();

    assert_eq!(
        registry.register_command::<InvalidContextCommand, InvalidContextHandler>(),
        Err(RegistrationError::InvalidCommandIdentity {
            command_name: "open",
            schema_version: 1,
            reason: "address context has an invalid format".to_owned(),
        })
    );
    assert_eq!(
        registry.register_command::<InvalidRoutedName, InvalidRoutedNameHandler>(),
        Err(RegistrationError::InvalidCommandIdentity {
            command_name: "Open",
            schema_version: 1,
            reason: "address name has an invalid format".to_owned(),
        })
    );
    assert_eq!(
        registry.register_command::<LongName, LongNameHandler>(),
        Err(RegistrationError::InvalidCommandIdentity {
            command_name: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            schema_version: 1,
            reason: "address name is too long".to_owned(),
        })
    );
    assert_eq!(registry.commands().count(), 0);
}

#[test]
fn blanket_definition_is_contextual_to_the_command_and_handler() {
    let first = <Open as CommandDefinition<OpenHandler>>::descriptor();
    let second = <OpenSecond as CommandDefinition<SecondOpenHandler>>::descriptor();

    assert_eq!(first.command_name, second.command_name);
    assert_ne!(first.bounded_context, second.bounded_context);
    assert_eq!(
        first.rust_handler_type,
        std::any::type_name::<OpenHandler>()
    );
}
