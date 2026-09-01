use super::super::{Bicycle, BicycleStatus};
use super::MarkRentedAction;

impl MarkRentedAction for Bicycle {
    fn mark_rented(&mut self) {
        self.status = BicycleStatus::Rented;
    }
}
