#![allow(dead_code)]

use domain::{
    Aggregate, AggregateId, AggregateType, BoundedContext, BoundedContextId, DecisionDescriptor,
    DecisionId, DecisionImplementationDescriptor, DecisionInputDescriptor,
    DecisionOutputDescriptor, DecisionOwnerId, DecisionOwnerType, DomainIdentity, DomainService,
    DomainServiceId, DomainServiceType, Entity, EntityId, EntityType, ValueObject, ValueObjectId,
    ValueObjectOwnerId, ValueObjectType,
};

#[derive(BoundedContext)]
#[domain(id = "lending", label = "Lending")]
struct Lending;

#[derive(DomainIdentity)]
#[domain(owner = ApplicationRoot)]
struct ApplicationId(u64);

#[derive(Entity)]
#[domain(
    id = "application-root",
    label = "Application root",
    owner = LoanApplication,
    decisions = [contracts::ApplicationReview]
)]
struct ApplicationRoot {
    #[domain(identity)]
    id: ApplicationId,
    decision_recorded: bool,
    approved: bool,
    rationale: String,
}

#[derive(Aggregate)]
#[domain(
    id = "loan-application",
    label = "Loan application",
    context = Lending,
    root = ApplicationRoot,
    actions = [contracts::ApplicationActions],
    decisions = [
        contracts::ApplicationEligibility,
        contracts::ApplicationRouting
    ]
)]
struct LoanApplication;

#[derive(ValueObject, Clone, Debug, Eq, PartialEq)]
#[domain(
    id = "underwriting-facts",
    label = "Underwriting facts",
    owner = LoanApplication,
    decisions = [contracts::FactsValidation]
)]
struct UnderwritingFacts {
    annual_income: u64,
    monthly_obligations: u64,
    requested_amount: u64,
    collateral_value: u64,
    identity_verified: bool,
}

#[derive(ValueObject, Clone, Debug, Eq, PartialEq)]
#[domain(
    id = "decision-outcome",
    label = "Decision outcome",
    owner = LoanApplication
)]
struct DecisionOutcome {
    approved: bool,
    rationale: String,
}

#[derive(DomainService)]
#[domain(
    id = "risk-policy",
    label = "Risk policy",
    context = Lending,
    decisions = [contracts::PortfolioPolicy]
)]
struct RiskPolicy;

mod contracts {
    use domain::{domain_actions, domain_decisions};

    #[domain_actions(aggregate)]
    pub trait ApplicationActions {
        #[action(id = "evaluate", label = "Evaluate application")]
        fn evaluate(root: &mut super::ApplicationRoot, input: super::UnderwritingFacts) -> bool;
    }

    #[domain_decisions(aggregate)]
    pub trait ApplicationEligibility {
        #[decision(id = "assess-affordability", label = "Assess affordability")]
        fn assess_affordability(input: super::UnderwritingFacts) -> super::DecisionOutcome;

        #[decision(id = "assess-collateral", label = "Assess collateral")]
        fn assess_collateral(input: super::UnderwritingFacts) -> super::DecisionOutcome;
    }

    #[domain_decisions(aggregate)]
    pub trait ApplicationRouting {
        #[decision(id = "route-automatically", label = "Route automatically")]
        fn route_automatically(input: super::UnderwritingFacts) -> super::DecisionOutcome;
    }

    #[domain_decisions(entity)]
    pub trait ApplicationReview {
        #[decision(id = "verify-identity", label = "Verify identity")]
        fn verify_identity(input: super::UnderwritingFacts) -> super::DecisionOutcome;
    }

    #[domain_decisions(value_object)]
    pub trait FactsValidation {
        #[decision(id = "validate-facts", label = "Validate facts")]
        fn validate_facts(input: super::UnderwritingFacts) -> super::DecisionOutcome;
    }

    #[domain_decisions(domain_service)]
    pub trait PortfolioPolicy {
        #[decision(id = "within-portfolio-limit", label = "Within portfolio limit")]
        fn within_portfolio_limit(input: super::UnderwritingFacts) -> super::DecisionOutcome;
    }
}

impl contracts::ApplicationActions for LoanApplication {
    fn evaluate(root: &mut ApplicationRoot, input: UnderwritingFacts) -> bool {
        let outcome = <Self as contracts::ApplicationEligibility>::assess_affordability(input);
        root.decision_recorded = true;
        root.approved = outcome.approved;
        root.rationale = outcome.rationale;
        root.approved
    }
}

impl contracts::ApplicationEligibility for LoanApplication {
    fn assess_affordability(input: UnderwritingFacts) -> DecisionOutcome {
        evaluated_outcome(
            input.annual_income >= input.monthly_obligations.saturating_mul(36),
            "Income supports the debt burden",
            "Debt burden exceeds policy",
        )
    }

    fn assess_collateral(input: UnderwritingFacts) -> DecisionOutcome {
        evaluated_outcome(
            input.collateral_value >= input.requested_amount,
            "Collateral covers the request",
            "Collateral does not cover the request",
        )
    }
}

impl contracts::ApplicationRouting for LoanApplication {
    fn route_automatically(input: UnderwritingFacts) -> DecisionOutcome {
        evaluated_outcome(
            input.requested_amount <= 250_000,
            "Application can use automatic review",
            "Application requires manual review",
        )
    }
}

impl contracts::ApplicationReview for ApplicationRoot {
    fn verify_identity(input: UnderwritingFacts) -> DecisionOutcome {
        evaluated_outcome(
            input.identity_verified,
            "Applicant identity is verified",
            "Applicant identity is not verified",
        )
    }
}

impl contracts::FactsValidation for UnderwritingFacts {
    fn validate_facts(input: UnderwritingFacts) -> DecisionOutcome {
        evaluated_outcome(
            input.annual_income > 0 && input.requested_amount > 0,
            "Underwriting facts are complete",
            "Underwriting facts are incomplete",
        )
    }
}

impl contracts::PortfolioPolicy for RiskPolicy {
    fn within_portfolio_limit(input: UnderwritingFacts) -> DecisionOutcome {
        evaluated_outcome(
            input.requested_amount <= 500_000,
            "Request is within the portfolio limit",
            "Request exceeds the portfolio limit",
        )
    }
}

fn evaluated_outcome(
    approved: bool,
    approval_rationale: &str,
    denial_rationale: &str,
) -> DecisionOutcome {
    DecisionOutcome {
        approved,
        rationale: if approved {
            approval_rationale.to_owned()
        } else {
            denial_rationale.to_owned()
        },
    }
}

const fn eligible_facts() -> UnderwritingFacts {
    UnderwritingFacts {
        annual_income: 120_000,
        monthly_obligations: 2_000,
        requested_amount: 180_000,
        collateral_value: 220_000,
        identity_verified: true,
    }
}

const fn unaffordable_facts() -> UnderwritingFacts {
    UnderwritingFacts {
        annual_income: 36_000,
        monthly_obligations: 1_500,
        requested_amount: 100_000,
        collateral_value: 120_000,
        identity_verified: true,
    }
}

const fn application_root() -> ApplicationRoot {
    ApplicationRoot {
        id: ApplicationId(41),
        decision_recorded: false,
        approved: false,
        rationale: String::new(),
    }
}

fn local_ids(contract: &[DecisionDescriptor]) -> Vec<&'static str> {
    contract.iter().map(|decision| decision.id.local).collect()
}

const fn expected_descriptor(
    owner: DecisionOwnerId,
    local: &'static str,
    label: &'static str,
) -> DecisionDescriptor {
    DecisionDescriptor {
        id: DecisionId { owner, local },
        label,
        input: DecisionInputDescriptor::ValueObject(UnderwritingFacts::DESCRIPTOR.id),
        output: DecisionOutputDescriptor::ValueObject(DecisionOutcome::DESCRIPTOR.id),
        implementation: DecisionImplementationDescriptor::Rust,
    }
}

#[test]
fn ordinary_trait_calls_execute_for_all_owner_kinds() {
    assert_eq!(
        <LoanApplication as contracts::ApplicationEligibility>::assess_affordability(
            eligible_facts()
        ),
        evaluated_outcome(
            true,
            "Income supports the debt burden",
            "Debt burden exceeds policy"
        )
    );
    assert_eq!(
        <LoanApplication as contracts::ApplicationEligibility>::assess_collateral(eligible_facts()),
        evaluated_outcome(
            true,
            "Collateral covers the request",
            "Collateral does not cover the request"
        )
    );
    assert_eq!(
        <LoanApplication as contracts::ApplicationRouting>::route_automatically(eligible_facts()),
        evaluated_outcome(
            true,
            "Application can use automatic review",
            "Application requires manual review"
        )
    );
    assert_eq!(
        <ApplicationRoot as contracts::ApplicationReview>::verify_identity(eligible_facts()),
        evaluated_outcome(
            true,
            "Applicant identity is verified",
            "Applicant identity is not verified"
        )
    );
    assert_eq!(
        <UnderwritingFacts as contracts::FactsValidation>::validate_facts(eligible_facts()),
        evaluated_outcome(
            true,
            "Underwriting facts are complete",
            "Underwriting facts are incomplete"
        )
    );
    assert_eq!(
        <RiskPolicy as contracts::PortfolioPolicy>::within_portfolio_limit(eligible_facts()),
        evaluated_outcome(
            true,
            "Request is within the portfolio limit",
            "Request exceeds the portfolio limit"
        )
    );
}

#[test]
fn action_interprets_a_negative_structured_decision_without_an_error_return() {
    let mut root = application_root();

    let approved = <LoanApplication as contracts::ApplicationActions>::evaluate(
        &mut root,
        unaffordable_facts(),
    );

    assert!(!approved);
    assert!(root.decision_recorded);
    assert!(!root.approved);
    assert_eq!(root.rationale, "Debt burden exceeds policy");
    assert_eq!(
        <LoanApplication as AggregateType>::ACTION_CONTRACTS[0][0].error,
        None
    );
}

#[test]
fn decision_owner_ids_match_each_domain_owner_descriptor() {
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
        UnderwritingFacts::DESCRIPTOR.id,
        ValueObjectId {
            owner: ValueObjectOwnerId::Aggregate(LoanApplication::DESCRIPTOR.id),
            local: "underwriting-facts",
        }
    );
    assert_eq!(
        RiskPolicy::DESCRIPTOR.id,
        DomainServiceId {
            context: BoundedContextId("lending"),
            local: "risk-policy",
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
    assert_eq!(
        UnderwritingFacts::DECISION_OWNER_ID,
        DecisionOwnerId::ValueObject(UnderwritingFacts::DESCRIPTOR.id)
    );
    assert_eq!(
        RiskPolicy::DECISION_OWNER_ID,
        DecisionOwnerId::DomainService(RiskPolicy::DESCRIPTOR.id)
    );
}

#[test]
fn decision_contract_attachments_preserve_trait_and_method_source_order() {
    let aggregate_contracts = <LoanApplication as AggregateType>::DECISION_CONTRACTS;
    let entity_contracts = <ApplicationRoot as EntityType>::DECISION_CONTRACTS;
    let value_object_contracts = <UnderwritingFacts as ValueObjectType>::DECISION_CONTRACTS;
    let service_contracts = <RiskPolicy as DomainServiceType>::DECISION_CONTRACTS;

    assert_eq!(aggregate_contracts.len(), 2);
    assert_eq!(
        aggregate_contracts[0],
        <LoanApplication as contracts::ApplicationEligibility>::__DOMAIN_DECISIONS
    );
    assert_eq!(
        aggregate_contracts[1],
        <LoanApplication as contracts::ApplicationRouting>::__DOMAIN_DECISIONS
    );
    assert_eq!(
        local_ids(aggregate_contracts[0]),
        ["assess-affordability", "assess-collateral"]
    );
    assert_eq!(local_ids(aggregate_contracts[1]), ["route-automatically"]);
    assert_eq!(
        entity_contracts,
        &[<ApplicationRoot as contracts::ApplicationReview>::__DOMAIN_DECISIONS]
    );
    assert_eq!(local_ids(entity_contracts[0]), ["verify-identity"]);
    assert_eq!(
        value_object_contracts,
        &[<UnderwritingFacts as contracts::FactsValidation>::__DOMAIN_DECISIONS]
    );
    assert_eq!(local_ids(value_object_contracts[0]), ["validate-facts"]);
    assert_eq!(
        service_contracts,
        &[<RiskPolicy as contracts::PortfolioPolicy>::__DOMAIN_DECISIONS]
    );
    assert_eq!(local_ids(service_contracts[0]), ["within-portfolio-limit"]);
}

#[test]
fn generated_descriptors_include_value_object_contracts_and_rust_implementation() {
    let aggregate_contracts = <LoanApplication as AggregateType>::DECISION_CONTRACTS;
    let entity_contracts = <ApplicationRoot as EntityType>::DECISION_CONTRACTS;
    let value_object_contracts = <UnderwritingFacts as ValueObjectType>::DECISION_CONTRACTS;
    let service_contracts = <RiskPolicy as DomainServiceType>::DECISION_CONTRACTS;

    assert_eq!(
        aggregate_contracts[0][0],
        expected_descriptor(
            DecisionOwnerId::Aggregate(LoanApplication::DESCRIPTOR.id),
            "assess-affordability",
            "Assess affordability"
        )
    );
    assert_eq!(
        aggregate_contracts[0][1],
        expected_descriptor(
            DecisionOwnerId::Aggregate(LoanApplication::DESCRIPTOR.id),
            "assess-collateral",
            "Assess collateral"
        )
    );
    assert_eq!(
        aggregate_contracts[1][0],
        expected_descriptor(
            DecisionOwnerId::Aggregate(LoanApplication::DESCRIPTOR.id),
            "route-automatically",
            "Route automatically"
        )
    );
    assert_eq!(
        entity_contracts[0][0],
        expected_descriptor(
            DecisionOwnerId::Entity(ApplicationRoot::DESCRIPTOR.id),
            "verify-identity",
            "Verify identity"
        )
    );
    assert_eq!(
        value_object_contracts[0][0],
        expected_descriptor(
            DecisionOwnerId::ValueObject(UnderwritingFacts::DESCRIPTOR.id),
            "validate-facts",
            "Validate facts"
        )
    );
    assert_eq!(
        service_contracts[0][0],
        expected_descriptor(
            DecisionOwnerId::DomainService(RiskPolicy::DESCRIPTOR.id),
            "within-portfolio-limit",
            "Within portfolio limit"
        )
    );
}
