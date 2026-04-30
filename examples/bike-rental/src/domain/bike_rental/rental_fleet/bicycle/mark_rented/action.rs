use rostfrei::domain_action;

#[domain_action(id = "mark-rented", label = "Mark rented")]
pub trait MarkRentedAction {
    fn mark_rented(&mut self);
}
