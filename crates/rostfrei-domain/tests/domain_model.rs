#![allow(dead_code)]

use domain::{
    Aggregate, BoundedContext, DomainError, DomainEvent, DomainIdentity, DomainService, Entity,
    ValueObject, domain_model,
};
use serde_json::json;

#[derive(BoundedContext)]
#[domain(id = "inbox", label = "Inbox")]
pub struct Inbox;

#[derive(DomainIdentity)]
pub struct MailboxId(u64);

#[derive(Entity)]
#[domain(id = "mailbox-root", label = "Mailbox")]
pub struct MailboxRoot {
    id: MailboxId,
    address: Option<Vec<EmailAddress>>,
}

impl domain::EntityDefinition for MailboxRoot {
    type Owner = Mailbox;
    type Identity = MailboxId;

    fn identity(&self) -> &Self::Identity {
        &self.id
    }
}

#[derive(Aggregate)]
#[domain(id = "mailbox", label = "Mailbox")]
pub struct Mailbox;

impl domain::AggregateDefinition for Mailbox {
    type Context = Inbox;
    type Root = MailboxRoot;
    type Event = MailboxEvents;
}

#[derive(domain::AggregateEvents)]
pub enum MailboxEvents {
    Event0(MailboxOpened),
}

#[derive(DomainIdentity)]
pub struct ThreadId(u64);

#[derive(Entity)]
#[domain(id = "thread-root", label = "Thread")]
pub struct ThreadRoot {
    id: ThreadId,
}

impl domain::EntityDefinition for ThreadRoot {
    type Owner = Thread;
    type Identity = ThreadId;

    fn identity(&self) -> &Self::Identity {
        &self.id
    }
}

#[derive(Aggregate)]
#[domain(id = "thread", label = "Thread")]
pub struct Thread;

impl domain::AggregateDefinition for Thread {
    type Context = Inbox;
    type Root = ThreadRoot;
    type Event = domain::NoDomainEvents;
    const ANCHOR: bool = true;
}

#[derive(ValueObject)]
#[domain(id = "email-address", label = "Email address")]
struct EmailAddress(String);

#[derive(DomainService)]
#[domain(id = "mail-transfer", label = "Mail transfer")]
struct MailTransfer;

impl domain::DomainServiceDefinition for MailTransfer {
    type Context = Inbox;
}

#[derive(DomainEvent)]
#[domain(id = "mailbox-opened", label = "Mailbox opened")]
pub struct MailboxOpened;

#[derive(DomainError)]
#[domain(
    id = "transfer-denied",
    label = "Transfer denied",
    code = "TRANSFER_DENIED",
    message = "Mail transfer was denied."
)]
struct TransferDenied;

#[test]
#[allow(clippy::too_many_lines)]
fn compiles_explicit_domain_model_to_json() {
    let model = domain_model! {
        contexts: [Inbox],
        aggregates: [Mailbox],
        entities: [MailboxRoot],
        value_objects: [EmailAddress],
        services: [MailTransfer],
        errors: [TransferDenied],
    }
    .expect("explicit domain model should be valid");

    assert_eq!(
        model,
        json!({
            "boundedContexts": [{
                "id": "inbox",
                "label": "Inbox",
            }],
            "aggregates": [{
                "id": {
                    "context": "inbox",
                    "local": "mailbox",
                },
                "label": "Mailbox",
                "root": {
                    "aggregate": {
                        "context": "inbox",
                        "local": "mailbox",
                    },
                    "local": "mailbox-root",
                },
            }],
            "entities": [{
                "id": {
                    "aggregate": {
                        "context": "inbox",
                        "local": "mailbox",
                    },
                    "local": "mailbox-root",
                },
                "label": "Mailbox",
                "identity": {
                    "owner": {
                        "aggregate": { "context": "inbox", "local": "mailbox" },
                        "local": "mailbox-root",
                    },
                },
                "fields": [{
                    "name": "id",
                    "value": {
                        "kind": "opaque",
                    },
                }, {
                    "name": "address",
                    "value": {
                        "kind": "optional",
                        "value": {
                            "kind": "list",
                            "element": {
                                "kind": "opaque",
                            },
                        },
                    },
                }],
            }],
            "valueObjects": [{
                "id": "email-address",
                "label": "Email address",
            }],
            "domainServices": [{
                "id": {
                    "context": "inbox",
                    "local": "mail-transfer",
                },
                "label": "Mail transfer",
            }],
            "domainIdentities": [{
                "id": {
                    "owner": {
                        "aggregate": { "context": "inbox", "local": "mailbox" },
                        "local": "mailbox-root",
                    },
                },
            }],
            "domainEvents": [{
                "id": {
                    "aggregate": {
                        "context": "inbox",
                        "local": "mailbox",
                    },
                    "local": "mailbox-opened",
                },
                "label": "Mailbox opened",
                "schemaVersion": 1,
                "fields": [],
            }],
            "domainErrors": [{
                "id": "transfer-denied",
                "label": "Transfer denied",
                "code": "TRANSFER_DENIED",
                "message": "Mail transfer was denied.",
                "fields": [],
            }],
            "actions": [],
            "policies": [],
            "queries": [],
            "invariants": [],
        })
    );

    let root = MailboxRoot {
        id: MailboxId(1),
        address: None,
    };
    let address = EmailAddress("team@example.com".to_owned());
    assert_eq!(root.id.0, 1);
    assert_eq!(address.0, "team@example.com");
}

#[test]
fn projects_the_anchor_role_only_for_declared_anchors() {
    let model = domain_model! {
        contexts: [Inbox],
        aggregates: [Mailbox, Thread],
        entities: [MailboxRoot, ThreadRoot],
        value_objects: [EmailAddress],
        services: [],
        errors: [],
    }
    .expect("domain model should be valid");

    let aggregates = model["aggregates"].as_array().unwrap();
    assert_eq!(aggregates[0]["id"]["local"], "mailbox");
    assert!(aggregates[0].get("anchor").is_none());
    assert_eq!(aggregates[1]["id"]["local"], "thread");
    assert_eq!(aggregates[1]["anchor"], true);
}

#[test]
fn supports_empty_declaration_lists() {
    let model = domain_model! {
        contexts: [],
        aggregates: [],
        entities: [],
        value_objects: [],
        services: [],
        errors: [],
    }
    .expect("empty domain model should be valid");

    assert!(
        model
            .as_object()
            .unwrap()
            .values()
            .all(|value| value.as_array().is_some_and(Vec::is_empty))
    );
}
rostfrei_domain_macros::__install_test_macro_support!();
