#[derive(DomainIdentity)]
pub struct FleetId(String);

impl ExternalIdentity {
    pub fn parse(input: &str) -> Self {
        Self(input.to_owned())
    }
}
