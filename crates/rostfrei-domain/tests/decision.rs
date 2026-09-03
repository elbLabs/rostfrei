use domain::{
    DecisionDescriptor, DecisionId, DecisionOutcome, DecisionOutcomeType, domain_decision,
};

#[derive(Debug, DecisionOutcome, Eq, PartialEq)]
enum EligibilityOutcome {
    #[outcome(id = "eligible", label = "Eligible")]
    Eligible,
    #[outcome(id = "rejected", label = "Rejected")]
    Rejected { reason: String },
}

#[domain_decision(id = "assess-eligibility", label = "Assess eligibility")]
trait AssessEligibility {
    fn assess(&self, active: bool) -> EligibilityOutcome;
}

struct RentalFleet;

impl AssessEligibility for RentalFleet {
    fn assess(&self, active: bool) -> EligibilityOutcome {
        if active {
            EligibilityOutcome::Eligible
        } else {
            EligibilityOutcome::Rejected {
                reason: "inactive".to_owned(),
            }
        }
    }
}

struct Preview;

impl AssessEligibility for Preview {
    fn assess(&self, _active: bool) -> EligibilityOutcome {
        EligibilityOutcome::Eligible
    }
}

#[test]
fn annotated_trait_keeps_ordinary_behavior_and_global_metadata() {
    assert_eq!(RentalFleet.assess(true), EligibilityOutcome::Eligible);
    assert_eq!(
        <RentalFleet as AssessEligibility>::DESCRIPTOR,
        DecisionDescriptor {
            id: DecisionId("assess-eligibility"),
            label: "Assess eligibility",
        }
    );
    assert_eq!(
        <RentalFleet as AssessEligibility>::DESCRIPTOR,
        <Preview as AssessEligibility>::DESCRIPTOR
    );
}

#[test]
fn decision_outcomes_retain_intrinsic_ordered_metadata() {
    let outcomes = <EligibilityOutcome as DecisionOutcomeType>::OUTCOMES;

    assert_eq!(outcomes.len(), 2);
    assert_eq!(outcomes[0].local_id, "eligible");
    assert_eq!(outcomes[0].label, "Eligible");
    assert_eq!(outcomes[1].local_id, "rejected");
    assert_eq!(outcomes[1].label, "Rejected");
}
rostfrei_domain_macros::__install_test_macro_support!();
