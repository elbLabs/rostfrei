fn main() -> Result<(), rostfrei::DomainModelError> {
    println!("{}", bike_rental::domain_model()?);
    Ok(())
}
