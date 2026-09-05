#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DomainTestKind {
    Action,
    Policy,
    Invariant,
    Lifecycle,
}

impl DomainTestKind {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Action => "action",
            Self::Policy => "policy",
            Self::Invariant => "invariant",
            Self::Lifecycle => "lifecycle",
        }
    }
}
