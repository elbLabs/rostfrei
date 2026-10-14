use serde::{Deserialize, Serialize};

/// Technical execution failures are not completed business outcomes.
/// The source kind, rather than diagnostic text, determines recovery policy.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    tag = "source",
    content = "kind",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ExecutionFailure {
    Store(StoreFailureKind),
    Codec(CodecFailureKind),
    Processor(ProcessorFailureKind),
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessorFailureKind {
    InvalidConfiguration,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreFailureKind {
    CorruptHistory,
    CapacityExhausted,
    ConfigurationMismatch,
    Conflict,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CodecFailureKind {
    UnknownEventType,
    UnsupportedSchemaVersion,
    MalformedPayload,
    InvalidEnvelope,
    EncodingFailed,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionRecovery {
    /// No completed outcome exists. Retry unchanged evidence with the same operation ID.
    Retry,
    /// Isolate the trigger durably; operator repair and explicit redrive are required.
    Blocked,
}

impl ExecutionFailure {
    pub const fn recovery(self) -> ExecutionRecovery {
        match self {
            Self::Store(StoreFailureKind::Conflict | StoreFailureKind::Unavailable) => {
                ExecutionRecovery::Retry
            }
            Self::Store(
                StoreFailureKind::CorruptHistory
                | StoreFailureKind::CapacityExhausted
                | StoreFailureKind::ConfigurationMismatch,
            )
            | Self::Codec(_)
            | Self::Processor(_) => ExecutionRecovery::Blocked,
        }
    }

    pub const fn is_retryable(self) -> bool {
        matches!(self.recovery(), ExecutionRecovery::Retry)
    }
}
