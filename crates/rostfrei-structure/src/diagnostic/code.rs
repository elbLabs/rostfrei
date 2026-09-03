use std::fmt;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[non_exhaustive]
pub enum DiagnosticCode {
    SourceParse,
    InvalidStructure,
    ImpureModule,
    WrongPlacement,
    InvalidCardinality,
    TestPlacement,
    ModuleTopology,
    UnexpectedRoleContent,
    InvalidTestMirror,
    MissingDomainCheckTarget,
    CompiledDomainCheckFailed,
}

impl DiagnosticCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SourceParse => "RF000",
            Self::InvalidStructure => "RF001",
            Self::ImpureModule => "RF002",
            Self::WrongPlacement => "RF003",
            Self::InvalidCardinality => "RF004",
            Self::TestPlacement => "RF005",
            Self::ModuleTopology => "RF006",
            Self::UnexpectedRoleContent => "RF007",
            Self::InvalidTestMirror => "RF008",
            Self::MissingDomainCheckTarget => "RF009",
            Self::CompiledDomainCheckFailed => "RF010",
        }
    }
}

impl fmt::Display for DiagnosticCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}
