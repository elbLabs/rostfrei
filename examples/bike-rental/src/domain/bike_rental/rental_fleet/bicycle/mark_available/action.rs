use rostfrei::domain_actions;

#[domain_actions(entity)]
pub(in crate::domain::bike_rental::rental_fleet) trait MarkAvailableAction {
    #[action(id = "mark-available", label = "Mark available")]
    fn mark_available(&mut self);
}
