use rostfrei::domain_actions;

use super::BicycleAdded;

#[domain_actions(aggregate(instance = AddBicycleActions))]
pub trait AddBicycleActionContract {
    #[action(id = "add-bicycle", label = "Add bicycle", raises = [BicycleAdded])]
    fn add_bicycle(&mut self);
}
