#[domain_decision(id = "assess-rental", label = "Assess rental")]
pub trait RentalAssessmentDecision {
    fn assess_rental(&self) -> bool;
}
