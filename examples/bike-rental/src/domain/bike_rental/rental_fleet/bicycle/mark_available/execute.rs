use super::{
    super::{Bicycle, BicycleStatus},
    MarkAvailableAction,
};

impl MarkAvailableAction for Bicycle {
    fn mark_available(&mut self) {
        self.status = BicycleStatus::Available;
    }
}
