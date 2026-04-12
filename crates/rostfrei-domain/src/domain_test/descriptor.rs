use super::subject::DomainTestSubject;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct DomainTestDescriptor {
    pub package: &'static str,
    pub target: &'static str,
    pub test: &'static str,
    pub file: &'static str,
    pub line: u32,
    pub column: u32,
    pub subject: DomainTestSubject,
}
