use domain::{Command, CommandDescriptor as ModeledCommandDescriptor};
use rostfrei_core::{Aggregate, AggregateInstance, CommandHandler, StreamId};
use rostfrei_registry::{CommandDefinition, DomainRegistry, RegistrationError};

struct FirstAggregate;
struct SecondAggregate;
struct EmptyAggregate;
struct QualifiedAggregate;
struct InvalidContextAggregate;
struct InvalidLocalAggregate;
struct NestedAggregate;

macro_rules! aggregate {
    ($aggregate:ty, $name:literal) => {
        impl Aggregate for $aggregate {
            type State = ();
            type Event = ();

            const AGGREGATE_TYPE: &'static str = $name;

            fn initial(_stream_id: &StreamId) -> Self::State {}

            fn apply(_state: &mut Self::State, _event: &Self::Event) {}
        }
    };
}

aggregate!(FirstAggregate, "first");
aggregate!(SecondAggregate, "second");
aggregate!(EmptyAggregate, "");
aggregate!(QualifiedAggregate, "bike-rental/rental-fleet");
aggregate!(InvalidContextAggregate, "BikeRental/rental-fleet");
aggregate!(InvalidLocalAggregate, "bike-rental/RentalFleet");
aggregate!(NestedAggregate, "bike-rental/group/rental-fleet");

struct Open;
struct Rename;
struct EmptyName;
struct ZeroVersion;
struct InvalidRoutedName;
struct LongName;

macro_rules! command {
    ($command:ty, $id:literal, $version:literal) => {
        impl Command for $command {
            const LOCAL_ID: &'static str = $id;
            const LABEL: &'static str = $id;
            const FIELDS: &'static [domain::FieldDescriptor] = &[];
            const SCHEMA_VERSION: u32 = $version;
        }
    };
}

command!(Open, "open", 1);
command!(Rename, "rename", 2);
command!(EmptyName, "", 1);
command!(ZeroVersion, "zero", 0);
command!(InvalidRoutedName, "Open", 1);
command!(
    LongName,
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    1
);

macro_rules! handler {
    ($aggregate:ty, $command:ty, $rejection:ty) => {
        impl CommandHandler<$command> for $aggregate {
            type Rejection = $rejection;

            fn handle(
                _command: &$command,
                _aggregate: &mut AggregateInstance<Self>,
            ) -> Result<(), Self::Rejection> {
                Ok(())
            }
        }
    };
}

handler!(FirstAggregate, Open, &'static str);
handler!(FirstAggregate, Rename, ());
handler!(FirstAggregate, EmptyName, ());
handler!(FirstAggregate, ZeroVersion, ());
handler!(SecondAggregate, Open, u8);
handler!(EmptyAggregate, Open, ());
handler!(QualifiedAggregate, Open, ());
handler!(InvalidContextAggregate, Open, ());
handler!(InvalidLocalAggregate, Open, ());
handler!(NestedAggregate, Open, ());
handler!(FirstAggregate, InvalidRoutedName, ());
handler!(FirstAggregate, LongName, ());

#[test]
fn paired_registration_builds_runtime_and_modeled_metadata() {
    let mut registry = DomainRegistry::new();
    registry.register_command::<FirstAggregate, Open>().unwrap();
    registry
        .register_command::<FirstAggregate, Rename>()
        .unwrap();

    let open = registry.command("first", "open", 1).unwrap();
    assert_eq!(open.aggregate_type, "first");
    assert_eq!(open.rust_command_type, std::any::type_name::<Open>());
    assert_eq!(
        open.rust_aggregate_type,
        std::any::type_name::<FirstAggregate>()
    );
    assert_eq!(
        open.modeled_command(),
        &ModeledCommandDescriptor {
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
    assert_eq!(registry.aggregates().collect::<Vec<_>>(), ["first"]);
    assert_eq!(
        registry
            .commands()
            .map(|command| command.command_name)
            .collect::<Vec<_>>(),
        ["open", "rename"]
    );
}

#[test]
fn one_owner_independent_command_can_be_registered_for_multiple_aggregates() {
    let mut registry = DomainRegistry::new();
    registry.register_command::<FirstAggregate, Open>().unwrap();
    registry
        .register_command::<SecondAggregate, Open>()
        .unwrap();

    assert!(registry.command("first", "open", 1).is_some());
    assert!(registry.command("second", "open", 1).is_some());
    assert_eq!(registry.commands().count(), 2);
    assert_eq!(
        registry
            .commands()
            .map(|command| command.aggregate_type.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );
}

#[test]
fn duplicate_pair_is_rejected_without_partial_mutation() {
    let mut registry = DomainRegistry::new();
    registry.register_command::<FirstAggregate, Open>().unwrap();

    assert_eq!(
        registry.register_command::<FirstAggregate, Open>(),
        Err(RegistrationError::DuplicateCommandIdentity {
            aggregate_type: "first".to_owned(),
            command_name: "open",
            schema_version: 1,
        })
    );
    assert_eq!(registry.commands().count(), 1);
}

#[test]
fn validates_command_and_aggregate_identity() {
    let mut registry = DomainRegistry::new();

    assert_eq!(
        registry.register_command::<FirstAggregate, EmptyName>(),
        Err(RegistrationError::EmptyCommandName {
            rust_command_type: std::any::type_name::<EmptyName>(),
        })
    );
    assert_eq!(
        registry.register_command::<FirstAggregate, ZeroVersion>(),
        Err(RegistrationError::ZeroSchemaVersion {
            command_name: "zero",
        })
    );
    assert_eq!(
        registry.register_command::<EmptyAggregate, Open>(),
        Err(RegistrationError::EmptyAggregateType {
            command_name: "open",
            schema_version: 1,
        })
    );
    assert_eq!(registry.commands().count(), 0);
}

#[test]
fn accepts_qualified_and_legacy_unqualified_aggregate_types() {
    let mut registry = DomainRegistry::new();

    registry.register_command::<FirstAggregate, Open>().unwrap();
    registry
        .register_command::<QualifiedAggregate, Open>()
        .unwrap();

    assert!(registry.command("first", "open", 1).is_some());
    assert!(
        registry
            .command("bike-rental/rental-fleet", "open", 1)
            .is_some()
    );
}

#[test]
fn rejects_unroutable_command_names_without_partial_mutation() {
    let mut registry = DomainRegistry::new();

    assert_eq!(
        registry.register_command::<FirstAggregate, InvalidRoutedName>(),
        Err(RegistrationError::InvalidCommandIdentity {
            command_name: "Open",
            schema_version: 1,
            reason: "address name has an invalid format".to_owned(),
        })
    );
    assert_eq!(registry.commands().count(), 0);
    assert_eq!(registry.aggregates().count(), 0);

    assert_eq!(
        registry.register_command::<FirstAggregate, LongName>(),
        Err(RegistrationError::InvalidCommandIdentity {
            command_name: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            schema_version: 1,
            reason: "address name is too long".to_owned(),
        })
    );
    assert_eq!(registry.commands().count(), 0);
    assert_eq!(registry.aggregates().count(), 0);
}

#[test]
fn rejects_unroutable_qualified_aggregate_types_without_partial_mutation() {
    macro_rules! assert_invalid_routing {
        ($aggregate:ty, $reason:literal) => {
            let mut registry = DomainRegistry::new();
            assert_eq!(
                registry.register_command::<$aggregate, Open>(),
                Err(RegistrationError::InvalidCommandIdentity {
                    command_name: "open",
                    schema_version: 1,
                    reason: $reason.to_owned(),
                })
            );
            assert_eq!(registry.commands().count(), 0);
            assert_eq!(registry.aggregates().count(), 0);
        };
    }

    assert_invalid_routing!(
        InvalidContextAggregate,
        "address context has an invalid format"
    );
    assert_invalid_routing!(InvalidLocalAggregate, "address name has an invalid format");
    assert_invalid_routing!(
        NestedAggregate,
        "aggregate type must contain at most one context separator"
    );
}

#[test]
fn blanket_definition_is_contextual_to_the_handler_aggregate() {
    let first = <Open as CommandDefinition<FirstAggregate>>::descriptor();
    let second = <Open as CommandDefinition<SecondAggregate>>::descriptor();

    assert_eq!(first.command_name, second.command_name);
    assert_ne!(first.aggregate_type, second.aggregate_type);
}
