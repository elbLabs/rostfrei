#[derive(DomainIdentity)]
pub struct FleetId(String);

impl FleetId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for FleetId {
    fn from(value: String) -> Self {
        Self(value)
    }
}
