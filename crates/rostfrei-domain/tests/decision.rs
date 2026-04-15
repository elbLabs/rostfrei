#![allow(dead_code)]

use domain::{
    Aggregate, AggregateId, AggregateType, BoundedContext, BoundedContextId,
    DecisionInputDescriptor, DecisionOutputDescriptor, DecisionOwnerId, DecisionOwnerType,
    DomainIdentity, Entity, EntityId, EntityType, ScalarType, ValueObject, ValueObjectType,
    domain_decisions,
};

#[derive(BoundedContext)]
#[domain(id = "lending", label = "Lending")]
struct Lending;

#[derive(DomainIdentity)]
#[domain(owner = ApplicationRoot)]
struct ApplicationId(u64);

#[derive(Aggregate)]
#[domain(
    id = "loan-application",
    label = "Loan application",
    context = Lending,
    root = ApplicationRoot,
    decisions
)]
struct LoanApplication;

#[derive(Entity)]
#[domain(
    id = "application-root",
    label = "Application root",
    owner = LoanApplication,
    decisions
)]
struct ApplicationRoot {
    #[domain(identity)]
    id: ApplicationId,
}

#[derive(ValueObject, Clone, Debug, Eq, PartialEq)]
#[domain(id = "decision-outcome", label = "Decision outcome", owner = LoanApplication)]
struct DecisionOutcome {
    rationale: String,
}

#[derive(ValueObject, Clone, Copy, Debug, Eq, PartialEq)]
#[domain(id = "decision-denial", label = "Decision denial", owner = LoanApplication)]
enum DecisionDenial {
    Unaffordable,
    IdentityNotVerified,
}

#[domain_decisions(aggregate)]
impl LoanApplication {
    #[decision(id = "assess-affordability", label = "Assess affordability")]
    fn assess_affordability(
        annual_income: u64,
        monthly_obligations: u64,
    ) -> Result<DecisionOutcome, DecisionDenial> {
        if annual_income >= monthly_obligations.saturating_mul(36) {
            Ok(DecisionOutcome {
                rationale: "Income supports the debt burden".to_owned(),
            })
        } else {
            Err(DecisionDenial::Unaffordable)
        }
    }

    #[decision(id = "route-automatically", label = "Route automatically")]
    fn route_automatically(requested_amount: u64) -> Result<DecisionOutcome, DecisionDenial> {
        if requested_amount == 0 {
            return Err(DecisionDenial::Unaffordable);
        }
        Ok(DecisionOutcome {
            rationale: if requested_amount <= 250_000 {
                "Automatic review"
            } else {
                "Manual review"
            }
            .to_owned(),
        })
    }

    #[decision(id = "system-ready", label = "System ready")]
    #[allow(clippy::unnecessary_wraps)]
    const fn system_ready() -> Result<bool, u8> {
        Ok(true)
    }

    #[decision(id = "classify", label = "Classify")]
    fn classify(r#type: String) -> Result<bool, u8> {
        if r#type.into_bytes().is_empty() {
            Err(0)
        } else {
            Ok(true)
        }
    }
}

#[domain_decisions(entity)]
impl ApplicationRoot {
    #[decision(id = "verify-identity", label = "Verify identity")]
    fn verify_identity(verified: bool) -> Result<(), DecisionDenial> {
        verified
            .then_some(())
            .ok_or(DecisionDenial::IdentityNotVerified)
    }
}

#[test]
fn ordinary_inherent_calls_return_typed_results() {
    assert_eq!(
        LoanApplication::assess_affordability(120_000, 2_000),
        Ok(DecisionOutcome {
            rationale: "Income supports the debt burden".to_owned(),
        })
    );
    assert_eq!(
        LoanApplication::assess_affordability(36_000, 1_500),
        Err(DecisionDenial::Unaffordable)
    );
    assert_eq!(ApplicationRoot::verify_identity(true), Ok(()));
    assert_eq!(
        ApplicationRoot::verify_identity(false),
        Err(DecisionDenial::IdentityNotVerified)
    );
    assert_eq!(LoanApplication::system_ready(), Ok(true));
    assert_eq!(LoanApplication::classify("standard".to_owned()), Ok(true));
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
fn generated_descriptors_preserve_parameters_result_and_source_order() {
    let aggregate = <LoanApplication as AggregateType>::DECISION_CONTRACTS[0];
    let entity = <ApplicationRoot as EntityType>::DECISION_CONTRACTS[0];

    assert_eq!(aggregate.len(), 4);
    assert_eq!(aggregate[0].id.local, "assess-affordability");
    assert_eq!(aggregate[1].id.local, "route-automatically");
    assert_eq!(aggregate[0].parameters.len(), 2);
    assert_eq!(aggregate[0].parameters[0].name, "annual_income");
    assert_eq!(
        aggregate[0].parameters[0].input,
        DecisionInputDescriptor::Scalar(ScalarType::U64)
    );
    assert_eq!(
        aggregate[0].output,
        Some(DecisionOutputDescriptor::ValueObject(
            DecisionOutcome::DESCRIPTOR.id
        ))
    );
    assert_eq!(
        aggregate[0].error,
        Some(DecisionOutputDescriptor::ValueObject(
            DecisionDenial::DESCRIPTOR.id
        ))
    );
    assert!(aggregate[2].parameters.is_empty());
    assert_eq!(
        aggregate[2].output,
        Some(DecisionOutputDescriptor::Scalar(ScalarType::Bool))
    );
    assert_eq!(
        aggregate[2].error,
        Some(DecisionOutputDescriptor::Scalar(ScalarType::U8))
    );
    assert_eq!(aggregate[3].parameters[0].name, "type");
    assert_eq!(
        aggregate[3].parameters[0].input,
        DecisionInputDescriptor::Scalar(ScalarType::String)
    );
    assert_eq!(entity[0].id.local, "verify-identity");
    assert_eq!(entity[0].output, None);
}
