#[domain_action(id = "mark-rented", label = "Mark rented")]
pub trait MarkRentedContract {
    fn mark_rented(&mut self);
}
