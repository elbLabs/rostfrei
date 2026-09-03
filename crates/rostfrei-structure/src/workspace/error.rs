use std::fmt;

#[derive(Debug)]
pub enum CheckError {
    MetadataInvocation(std::io::Error),
    MetadataFailed(String),
    InvalidMetadata(serde_json::Error),
}

impl fmt::Display for CheckError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MetadataInvocation(error) => {
                write!(formatter, "could not run cargo metadata: {error}")
            }
            Self::MetadataFailed(error) => write!(formatter, "cargo metadata failed: {error}"),
            Self::InvalidMetadata(error) => {
                write!(formatter, "invalid cargo metadata output: {error}")
            }
        }
    }
}

impl std::error::Error for CheckError {}
