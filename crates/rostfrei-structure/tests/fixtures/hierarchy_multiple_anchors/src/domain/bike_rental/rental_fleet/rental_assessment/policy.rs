#[domain_policy(id = "assess-rental", label = "Assess rental")]
pub trait RentalAssessmentPolicy {
    fn assess_rental(&self) -> bool;
}
