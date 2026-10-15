use serde::{Deserialize, Serialize};

use super::{ErrorKind, Phase, Result, ToolError};

pub const SKILLS: [&str; 2] = ["rostfrei-develop-feature", "rostfrei-tracer"];

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Contracts {
    pub catalog: u32,
    pub behavioral_test: u32,
    pub message_series: u32,
    pub fixture: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Release {
    pub kit_version: u32,
    pub framework_version: String,
    pub source_revision: String,
    pub source_tree_dirty: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_fingerprint: Option<String>,
    pub contracts: Contracts,
    pub skills: Vec<String>,
}

impl Release {
    pub fn current() -> Self {
        Self {
            kit_version: 1,
            framework_version: env!("CARGO_PKG_VERSION").to_owned(),
            source_revision: env!("ROSTFREI_CLI_REVISION").to_owned(),
            source_tree_dirty: env!("ROSTFREI_CLI_DIRTY") == "true",
            source_fingerprint: Some(env!("ROSTFREI_CLI_FINGERPRINT"))
                .filter(|value| !value.is_empty())
                .map(str::to_owned),
            contracts: Contracts {
                catalog: 1,
                behavioral_test: 2,
                message_series: 2,
                fixture: 1,
            },
            skills: SKILLS.into_iter().map(str::to_owned).collect(),
        }
    }

    pub fn check(&self, phase: Phase) -> Result<()> {
        let cli = Self::current();
        if !self.matches(&cli) {
            return Err(ToolError::new(
                ErrorKind::SetupFailed,
                phase,
                "Kit/CLI release or protocol mismatch; install rostfrei-structure from the kit sourceRevision",
            ));
        }
        Ok(())
    }

    pub fn valid_identity(&self) -> bool {
        let hexadecimal = |value: &str, length| {
            value.len() == length && value.bytes().all(|byte| byte.is_ascii_hexdigit())
        };
        hexadecimal(&self.source_revision, 40)
            && if self.source_tree_dirty {
                self.source_fingerprint
                    .as_deref()
                    .is_some_and(|value| hexadecimal(value, 64))
            } else {
                self.source_fingerprint.is_none()
            }
    }

    pub fn matches(&self, other: &Self) -> bool {
        self.valid_identity() && other.valid_identity() && self == other
    }
}
