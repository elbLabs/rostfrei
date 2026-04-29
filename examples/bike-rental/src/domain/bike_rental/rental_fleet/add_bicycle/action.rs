use rostfrei::domain_actions;

#[domain_actions(aggregate(instance = AddBicycleActions))]
pub trait AddBicycleActionContract {
    #[action(id = "add-bicycle", label = "Add bicycle")]
    fn add_bicycle(&mut self);
}
