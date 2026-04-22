use rostfrei_registry::CommandDefinition;
use serde::Serialize;
use serde_json::Value;

pub trait CommandInputOptions<Command>: Send + Sync
where
    Command: CommandDefinition,
{
    fn fields(
        &self,
        state: &<Command::Aggregate as rostfrei_core::Aggregate>::State,
    ) -> Vec<CommandInputField>;
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandInputDocument {
    pub fields: Vec<CommandInputField>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandInputField {
    pub name: String,
    pub label: String,
    pub options: Vec<CommandInputOption>,
}

impl CommandInputField {
    pub fn select(
        name: impl Into<String>,
        label: impl Into<String>,
        options: Vec<CommandInputOption>,
    ) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            options,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandInputOption {
    pub value: Value,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl CommandInputOption {
    pub fn new(value: impl Into<Value>, label: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
            description: None,
        }
    }

    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}
