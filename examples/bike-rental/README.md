# Bike rental example

This public example models a bicycle rental fleet. It demonstrates rostfrei's
compiled domain metadata without depending on a production application:

- `RentalFleetAggregate` owns the fleet and its bicycles;
- `RentBicycle` is a public aggregate command;
- `AssessRentalEligibility` is a Rust decision;
- `BicycleRented` and `BicycleUnavailable` describe action outcomes; and
- `BicycleAvailabilityQueries` exposes a read-only availability query.

Print the compiled domain model:

```sh
cargo run --locked -p bike-rental
```

Run the example tests:

```sh
cargo test --locked -p bike-rental
```
