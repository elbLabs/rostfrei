impl MarkRentedContract for Bicycle {
    fn mark_rented(&mut self) {
        self.status = BicycleStatus::Rented;
    }
}
