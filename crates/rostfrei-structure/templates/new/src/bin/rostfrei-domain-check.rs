fn main() -> Result<(), rostfrei::DomainModelError> {
    {{module_name}}::domain_model().map(|_| ())
}
