use super::{
    super::{Bicycle, BicycleStatus},
    MarkRentedAction,
};

impl MarkRentedAction for Bicycle {
    fn mark_rented(&mut self) {
        self.status = BicycleStatus::Rented;
    }
}
