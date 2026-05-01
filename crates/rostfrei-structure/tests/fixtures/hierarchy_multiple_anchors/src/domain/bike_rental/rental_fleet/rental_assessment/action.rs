#[domain_action(id = "assess-rental", label = "Assess rental")]
pub trait RentalAssessmentAction {
    fn assess_rental(&self);
}
