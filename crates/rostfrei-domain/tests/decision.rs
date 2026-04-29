#![allow(dead_code)]

use domain::DecisionOutcome;
use domain::{
    Aggregate, AggregateId, AggregateType, BoundedContext, BoundedContextId, DecisionGroupType,
    DecisionOutcomeType, DecisionOwnerId, DecisionOwnerType, DomainIdentity, Entity, EntityId,
    EntityType, domain_decisions,
};

struct AffordabilityDecisions;

mod routing {
    #[allow(clippy::redundant_pub_crate)]
    pub(super) struct RoutingDecisions;
}

struct RootDecisions;

#[derive(BoundedContext)]
#[domain(id = "lending", label = "Lending")]
struct Lending;

#[derive(DomainIdentity)]
struct ApplicationId(u64);

#[derive(Aggregate)]
#[domain(id = "loan-application", label = "Loan application")]
struct LoanApplication;

impl domain::AggregateDefinition for LoanApplication {
    type Context = Lending;
    type Root = ApplicationRoot;
    type Event = domain::NoDomainEvents;
}

#[derive(Entity)]
#[domain(id = "application-root", label = "Application root")]
struct ApplicationRoot {
    #[domain(identity)]
    id: ApplicationId,
}

impl domain::EntityDefinition for ApplicationRoot {
    type Owner = LoanApplication;
    type Identity = ApplicationId;
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ApprovalEvidence {
    rationale: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DecisionDenial {
    Unaffordable,
    IdentityNotVerified,
}

struct Classification(String);

#[derive(DecisionOutcome, Debug, Eq, PartialEq)]
enum ApplicationOutcome {
    #[outcome(id = "ready", label = "Ready")]
    Ready,
    #[outcome(id = "approved", label = "Approved")]
    Approved(ApprovalEvidence, bool, u16),
    #[outcome(id = "declined", label = "Declined")]
    Declined {
        denial: DecisionDenial,
        retryable: bool,
        rank: u8,
    },
}

#[derive(DecisionOutcome)]
enum CfgFieldOutcome {
    #[outcome(id = "tuple", label = "Tuple")]
    Tuple(u8, #[cfg(any())] MissingTupleFieldType, #[cfg(test)] bool),
    #[outcome(id = "struct", label = "Struct")]
    Struct {
        first: u16,
        #[cfg(any())]
        missing: MissingNamedFieldType,
        #[cfg(test)]
        last: bool,
    },
}

#[domain_decisions(aggregate, group = AffordabilityDecisions)]
impl LoanApplication {
    #[decision(id = "assess-affordability", label = "Assess affordability")]
    fn assess_affordability(annual_income: u64, monthly_obligations: u64) -> ApplicationOutcome {
        if annual_income >= monthly_obligations.saturating_mul(36) {
            ApplicationOutcome::Approved(
                ApprovalEvidence {
                    rationale: "Income supports the debt burden".to_owned(),
                },
                true,
                100,
            )
        } else {
            ApplicationOutcome::Declined {
                denial: DecisionDenial::Unaffordable,
                retryable: true,
                rank: 1,
            }
        }
    }

    #[decision(id = "system-ready", label = "System ready")]
    const fn system_ready() -> ApplicationOutcome {
        ApplicationOutcome::Ready
    }
}

#[domain_decisions(aggregate, group = routing::RoutingDecisions)]
impl LoanApplication {
    #[decision(id = "classify-owned", label = "Classify owned")]
    fn classify_owned(r#type: Classification) -> ApplicationOutcome {
        if r#type.0.into_bytes().is_empty() {
            ApplicationOutcome::Declined {
                denial: DecisionDenial::Unaffordable,
                retryable: true,
                rank: 2,
            }
        } else {
            ApplicationOutcome::Ready
        }
    }

    #[decision(id = "classify-borrowed", label = "Classify borrowed")]
    const fn classify_borrowed(r#type: &Classification) -> ApplicationOutcome {
        if r#type.0.is_empty() {
            ApplicationOutcome::Declined {
                denial: DecisionDenial::Unaffordable,
                retryable: true,
                rank: 2,
            }
        } else {
            ApplicationOutcome::Ready
        }
    }
}

#[domain_decisions(entity, group = RootDecisions)]
impl ApplicationRoot {
    #[decision(id = "verify-identity", label = "Verify identity")]
    const fn verify_identity(verified: bool) -> ApplicationOutcome {
        if verified {
            ApplicationOutcome::Ready
        } else {
            ApplicationOutcome::Declined {
                denial: DecisionDenial::IdentityNotVerified,
                retryable: false,
                rank: 3,
            }
        }
    }
}

#[test]
fn ordinary_inherent_calls_return_typed_outcomes() {
    assert_eq!(
        LoanApplication::assess_affordability(120_000, 2_000),
        ApplicationOutcome::Approved(
            ApprovalEvidence {
                rationale: "Income supports the debt burden".to_owned(),
            },
            true,
            100,
        )
    );
    assert_eq!(
        LoanApplication::assess_affordability(36_000, 1_500),
        ApplicationOutcome::Declined {
            denial: DecisionDenial::Unaffordable,
            retryable: true,
            rank: 1,
        }
    );
    assert_eq!(LoanApplication::system_ready(), ApplicationOutcome::Ready);
    assert_eq!(
        LoanApplication::classify_owned(Classification("standard".to_owned())),
        ApplicationOutcome::Ready
    );
    assert_eq!(
        LoanApplication::classify_borrowed(&Classification("standard".to_owned())),
        ApplicationOutcome::Ready
    );
    assert_eq!(
        ApplicationRoot::verify_identity(false),
        ApplicationOutcome::Declined {
            denial: DecisionDenial::IdentityNotVerified,
            retryable: false,
            rank: 3,
        }
    );
}

#[test]
fn decision_owner_ids_match_aggregate_and_entity_descriptors() {
    assert_eq!(
        LoanApplication::DESCRIPTOR.id,
        AggregateId {
            context: BoundedContextId("lending"),
            local: "loan-application",
        }
    );
    assert_eq!(
        ApplicationRoot::DESCRIPTOR.id,
        EntityId {
            aggregate: LoanApplication::DESCRIPTOR.id,
            local: "application-root",
        }
    );
    assert_eq!(
        LoanApplication::DECISION_OWNER_ID,
        DecisionOwnerId::Aggregate(LoanApplication::DESCRIPTOR.id)
    );
    assert_eq!(
        ApplicationRoot::DECISION_OWNER_ID,
        DecisionOwnerId::Entity(ApplicationRoot::DESCRIPTOR.id)
    );
}

#[test]
fn generated_descriptors_preserve_group_method_and_outcome_order() {
    let aggregate = [
        <AffordabilityDecisions as DecisionGroupType>::DECISIONS,
        <routing::RoutingDecisions as DecisionGroupType>::DECISIONS,
    ];
    let entity = [<RootDecisions as DecisionGroupType>::DECISIONS];

    assert_eq!(aggregate.len(), 2);
    assert_eq!(
        aggregate[0]
            .iter()
            .map(|decision| decision.id.local)
            .collect::<Vec<_>>(),
        ["assess-affordability", "system-ready"]
    );
    assert_eq!(
        aggregate[1]
            .iter()
            .map(|decision| decision.id.local)
            .collect::<Vec<_>>(),
        ["classify-owned", "classify-borrowed"]
    );
    assert_eq!(entity[0][0].id.local, "verify-identity");

    let outcomes = aggregate[0][0].outcomes;
    assert_eq!(
        outcomes
            .iter()
            .map(|outcome| outcome.local_id)
            .collect::<Vec<_>>(),
        ["ready", "approved", "declined"]
    );
    assert_eq!(
        outcomes
            .iter()
            .map(|outcome| outcome.label)
            .collect::<Vec<_>>(),
        ["Ready", "Approved", "Declined"]
    );
}

#[test]
fn ordinary_owned_and_borrowed_inputs_do_not_add_descriptor_metadata() {
    let routing = <routing::RoutingDecisions as DecisionGroupType>::DECISIONS;
    assert_eq!(routing[0].id.local, "classify-owned");
    assert_eq!(routing[1].id.local, "classify-borrowed");
}

#[test]
fn field_cfg_does_not_affect_semantic_outcome_metadata() {
    let outcomes = <CfgFieldOutcome as DecisionOutcomeType>::OUTCOMES;

    assert_eq!(outcomes.len(), 2);
    assert_eq!(
        outcomes
            .iter()
            .map(|outcome| outcome.local_id)
            .collect::<Vec<_>>(),
        ["tuple", "struct"]
    );
}
