use rostfrei::domain_action;

#[domain_action(id = "add-bicycle", label = "Add bicycle")]
pub trait AddBicycleAction {
    fn add_bicycle(&mut self);
}
