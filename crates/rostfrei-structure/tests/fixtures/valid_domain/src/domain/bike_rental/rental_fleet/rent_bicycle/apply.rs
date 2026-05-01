impl Apply<BicycleRented> for RentalFleet {
    fn apply(&mut self, event: &BicycleRented) {
        self.bicycle_mut(&event.bicycle_id).mark_rented();
    }
}
