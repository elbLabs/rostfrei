#![allow(dead_code)]

use rostfrei_domain::{
    Aggregate, BoundedContext, DomainError, DomainEvent, DomainIdentity, DomainService, Entity,
    ValueObject, domain_actions, domain_model,
};
use serde_json::json;

#[derive(BoundedContext)]
#[domain(id = "inbox", label = "Inbox")]
pub struct Inbox;

#[derive(DomainIdentity)]
#[domain(owner = MailboxRoot)]
pub struct MailboxId(u64);

#[derive(Entity)]
#[domain(id = "mailbox-root", label = "Mailbox", owner = Mailbox)]
pub struct MailboxRoot {
    #[domain(identity)]
    id: MailboxId,
    #[domain(value_object)]
    address: Option<Vec<EmailAddress>>,
}

#[derive(Aggregate)]
#[domain(
    id = "mailbox",
    label = "Mailbox",
    context = Inbox,
    root = MailboxRoot,
    actions = [MailboxClosingActions, MailboxOpeningActions]
)]
pub struct Mailbox;

#[domain_actions(aggregate)]
pub trait MailboxOpeningActions {
    #[action(id = "open", label = "Open mailbox")]
    fn open(root: &mut MailboxRoot);
}

impl MailboxOpeningActions for Mailbox {
    fn open(root: &mut MailboxRoot) {
        let _ = root;
    }
}

#[domain_actions(aggregate)]
pub trait MailboxClosingActions {
    #[action(id = "close", label = "Close mailbox")]
    fn close(root: &mut MailboxRoot);
}

impl MailboxClosingActions for Mailbox {
    fn close(root: &mut MailboxRoot) {
        let _ = root;
    }
}

#[derive(ValueObject)]
#[domain(id = "email-address", label = "Email address", owner = Inbox)]
struct EmailAddress(String);

#[derive(DomainService)]
#[domain(id = "mail-transfer", label = "Mail transfer", context = Inbox)]
struct MailTransfer;

#[derive(DomainEvent)]
#[domain(id = "mailbox-opened", label = "Mailbox opened", owner = Mailbox)]
struct MailboxOpened;

#[derive(DomainError)]
#[domain(id = "transfer-denied", label = "Transfer denied", owner = MailTransfer, code = "TRANSFER_DENIED", message = "Mail transfer was denied.")]
struct TransferDenied;

#[test]
fn compiles_explicit_domain_model_to_json() {
    let model = domain_model! {
        contexts: [Inbox],
        aggregates: [Mailbox],
        entities: [MailboxRoot],
        identities: [MailboxId],
        value_objects: [EmailAddress],
        services: [MailTransfer],
        commands: [],
        events: [MailboxOpened],
        errors: [TransferDenied],
        query_groups: [],
    };

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
                    "field": "id",
                    "id": {
                        "owner": {
                            "aggregate": { "context": "inbox", "local": "mailbox" },
                            "local": "mailbox-root",
                        },
                    },
                },
                "fields": [{
                    "name": "id",
                    "value": {
                        "kind": "identity",
                        "id": {
                            "owner": {
                                "aggregate": { "context": "inbox", "local": "mailbox" },
                                "local": "mailbox-root",
                            },
                        },
                    },
                }, {
                    "name": "address",
                    "value": {
                        "kind": "optional",
                        "value": {
                            "kind": "list",
                            "element": {
                                "kind": "valueObject",
                                "id": {
                                    "owner": { "kind": "boundedContext", "id": "inbox" },
                                    "local": "email-address",
                                },
                            },
                        },
                    },
                }],
            }],
            "valueObjects": [{
                "id": {
                    "owner": {
                        "kind": "boundedContext",
                        "id": "inbox",
                    },
                    "local": "email-address",
                },
                "label": "Email address",
                "fields": [{
                    "name": "0",
                    "value": { "kind": "scalar", "scalar": "string" },
                }],
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
                "scalar": "u64",
            }],
            "domainCommands": [],
            "domainEvents": [{
                "id": {
                    "aggregate": {
                        "context": "inbox",
                        "local": "mailbox",
                    },
                    "local": "mailbox-opened",
                },
                "label": "Mailbox opened",
                "fields": [],
            }],
            "domainErrors": [{
                "id": {
                    "owner": {
                        "kind": "domainService",
                        "id": {
                            "context": "inbox",
                            "local": "mail-transfer",
                        },
                    },
                    "local": "transfer-denied",
                },
                "label": "Transfer denied",
                "code": "TRANSFER_DENIED",
                "message": "Mail transfer was denied.",
                "fields": [],
            }],
            "actions": [{
                "id": {
                    "owner": {
                        "kind": "aggregate",
                        "id": {
                            "context": "inbox",
                            "local": "mailbox",
                        },
                    },
                    "local": "close",
                },
                "label": "Close mailbox",
                "input": null,
                "output": null,
                "error": null,
            }, {
                "id": {
                    "owner": {
                        "kind": "aggregate",
                        "id": {
                            "context": "inbox",
                            "local": "mailbox",
                        },
                    },
                    "local": "open",
                },
                "label": "Open mailbox",
                "input": null,
                "output": null,
                "error": null,
            }],
            "decisions": [],
            "queries": [],
            "invariants": [],
        })
    );

    let mut root = MailboxRoot {
        id: MailboxId(1),
        address: None,
    };
    Mailbox::open(&mut root);
    let address = EmailAddress("team@example.com".to_owned());
    assert_eq!(root.id.0, 1);
    assert_eq!(address.0, "team@example.com");
}

#[test]
fn supports_empty_declaration_lists() {
    let model = domain_model! {
        contexts: [],
        aggregates: [],
        entities: [],
        identities: [],
        value_objects: [],
        services: [],
        commands: [],
        events: [],
        errors: [],
        query_groups: [],
    };

    assert!(
        model
            .as_object()
            .unwrap()
            .values()
            .all(|value| value.as_array().is_some_and(Vec::is_empty))
    );
}
