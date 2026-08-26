#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvariantViolation {
    pub path: String,
    pub reason: String,
}

impl InvariantViolation {
    pub fn new(path: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            reason: reason.into(),
        }
    }
}
