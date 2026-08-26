#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DomainTestKind {
    Action,
    Decision,
    Invariant,
    Lifecycle,
}

impl DomainTestKind {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Action => "action",
            Self::Decision => "decision",
            Self::Invariant => "invariant",
            Self::Lifecycle => "lifecycle",
        }
    }

    pub(crate) const fn accepts_typed_reference(self) -> bool {
        matches!(self, Self::Action | Self::Decision | Self::Invariant)
    }
}
