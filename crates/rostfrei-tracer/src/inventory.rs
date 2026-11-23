//! Host-supplied declarations visible to the inspection capability.

use crate::domain_inventory::{DomainInventory, ReadModelDeclaration};
use serde::Serialize;

/// Logical declarations are separate from broker samples. A host can supply a
/// compiled application inventory or construct one from another catalog adapter.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StudioDomainInventory {
    #[serde(flatten)]
    pub domain: DomainInventory,
    pub read_models: Vec<StudioReadModel>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StudioReadModel {
    #[serde(flatten)]
    pub declaration: ReadModelDeclaration,
    /// Explicit broker source label. A stream name alone is not unique across accounts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub broker_source: Option<String>,
    /// Public workload label, never a directory or service address.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workload: Option<String>,
}

impl StudioReadModel {
    pub const fn new(declaration: ReadModelDeclaration, broker_source: Option<String>) -> Self {
        Self {
            declaration,
            broker_source,
            workload: None,
        }
    }

    /// Explicitly bind this declaration to a shared container workload.
    #[must_use]
    pub fn with_workload(mut self, label: impl Into<String>) -> Self {
        self.workload = Some(label.into());
        self
    }
}

impl StudioDomainInventory {
    pub const fn new(domain: DomainInventory) -> Self {
        Self {
            domain,
            read_models: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_read_model(
        self,
        declaration: ReadModelDeclaration,
        broker_source: Option<String>,
    ) -> Self {
        self.with_studio_read_model(StudioReadModel::new(declaration, broker_source))
    }

    /// Add an enriched declaration to the same inventory as `with_read_model`.
    #[must_use]
    pub fn with_studio_read_model(mut self, model: StudioReadModel) -> Self {
        self.read_models.push(model);
        self
    }
}
