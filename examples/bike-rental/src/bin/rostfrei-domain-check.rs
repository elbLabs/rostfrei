fn main() -> Result<(), rostfrei::DomainModelError> {
    bike_rental::domain_model().map(|_| ())
}
