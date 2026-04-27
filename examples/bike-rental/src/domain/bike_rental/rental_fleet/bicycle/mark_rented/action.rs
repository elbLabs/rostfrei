use rostfrei::domain_actions;

#[domain_actions(entity)]
pub(in crate::domain::bike_rental::rental_fleet) trait MarkRentedAction {
    #[action(id = "mark-rented", label = "Mark rented")]
    fn mark_rented(&mut self);
}
