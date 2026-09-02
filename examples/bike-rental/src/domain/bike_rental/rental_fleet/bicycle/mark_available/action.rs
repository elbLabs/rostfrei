use rostfrei::domain_action;

#[domain_action(id = "mark-available", label = "Mark available")]
pub trait MarkAvailableAction {
    fn mark_available(&mut self);
}
